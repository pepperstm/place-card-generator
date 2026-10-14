use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use eframe::egui::{self, Color32, RichText};
use place_cards::fonts::{self, Builtin, Bundled, FontSpec, Fonts, Measurer, SystemFont};
use place_cards::pdf::{self, PlacedText};
use place_cards::{Guest, parse_menu, read_csv, sort_by_table, table_sort_key, write_csv};

const BAD: Color32 = Color32::from_rgb(255, 214, 214);
const ERR: Color32 = Color32::from_rgb(200, 30, 30);

#[derive(Default, Clone)]
struct Row {
    name: String,
    table: String,
    menu: String,
    dietary: String,
}

impl Row {
    fn is_blank(&self) -> bool {
        self.name.trim().is_empty()
            && self.table.trim().is_empty()
            && self.menu.trim().is_empty()
            && self.dietary.trim().is_empty()
    }
    fn is_bad(&self) -> bool {
        !self.is_blank() && (self.name.trim().is_empty() || self.table.trim().is_empty())
    }
    fn to_guest(&self) -> Guest {
        Guest {
            name: self.name.trim().into(),
            table: self.table.trim().into(),
            menu: parse_menu(&self.menu),
            dietary: parse_menu(&self.dietary),
        }
    }
}

struct App {
    title: String,
    rows: Vec<Row>,
    new: Row,
    sort: bool,
    replace_on_import: bool,
    status: String,
    status_is_error: bool,
    focus_name: bool,
    plan_landscape: bool,
    allergy_marker: bool,
    fonts: Fonts,
    sys_fonts: Arc<Mutex<Option<Vec<SystemFont>>>>,
    font_filter: String,
    selected: Option<usize>,
    page_view: bool,
    measurer: Measurer,
    pfonts: PreviewFonts,
    demo: bool,
}

const FONT_KEYS: [&str; 4] = ["font_title", "font_name", "font_table", "font_menu"];

fn load_saved_fonts(storage: Option<&dyn eframe::Storage>) -> Fonts {
    let mut f = Fonts::default();
    if let Some(st) = storage {
        let slots = [&mut f.title, &mut f.name, &mut f.table, &mut f.menu];
        for (slot, key) in slots.into_iter().zip(FONT_KEYS) {
            if let Some(Ok(spec)) = st.get_string(key).map(|s| FontSpec::parse(&s)) {
                *slot = spec; // silently ignores fonts that no longer exist
            }
        }
    }
    f
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let sys_fonts = Arc::new(Mutex::new(None));
        {
            let sys = sys_fonts.clone();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let list = std::panic::catch_unwind(fonts::list_system_fonts).unwrap_or_default();
                if let Ok(mut g) = sys.lock() {
                    *g = Some(list);
                }
                ctx.request_repaint();
            });
        }
        Self {
            fonts: load_saved_fonts(cc.storage),
            sys_fonts,
            font_filter: String::new(),
            selected: None,
            page_view: false,
            measurer: Measurer::new(),
            pfonts: PreviewFonts::new(),
            demo: false,
            title: String::new(),
            rows: Vec::new(),
            new: Row::default(),
            sort: false,
            replace_on_import: false,
            status: String::new(),
            status_is_error: false,
            focus_name: true,
            plan_landscape: false,
            allergy_marker: cc.storage.and_then(|s| s.get_string("allergy_marker")).map(|v| v != "0").unwrap_or(true),
        }
    }

    /// Hidden debug/screenshot aid: when the env var PLACE_CARDS_DEMO is set,
    /// pre-load the sample guests.csv shipped with the source tree, pick
    /// non-default fonts and select the second guest. Fonts chosen this way are
    /// never persisted. Has no effect when the variable is unset.
    fn apply_demo(&mut self) {
        let csv = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/guests.csv"));
        if let Ok(gs) = read_csv(csv) {
            self.rows = gs
                .into_iter()
                .map(|g| Row { name: g.name, table: g.table, menu: g.menu.join("; "), dietary: g.dietary.join("; ") })
                .collect();
        }
        self.title = "Smith Wedding".into();
        let georgia = std::path::PathBuf::from(
            std::env::var("PLACE_CARDS_DEMO_FONT")
                .unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Georgia.ttf".into()),
        );
        self.fonts = Fonts {
            title: FontSpec::Builtin(Builtin::TimesRoman),
            name: if georgia.exists() { FontSpec::File(georgia) } else { FontSpec::Builtin(Builtin::TimesBold) },
            table: FontSpec::Builtin(Builtin::Courier),
            menu: FontSpec::Builtin(Builtin::TimesItalic),
        };
        self.selected = Some(1);
        self.page_view = std::env::var("PLACE_CARDS_DEMO").as_deref() == Ok("page");
        self.demo = true;
    }

    fn ok(&mut self, s: impl Into<String>) {
        self.status = s.into();
        self.status_is_error = false;
    }
    fn err(&mut self, s: impl Into<String>) {
        self.status = s.into();
        self.status_is_error = true;
    }

    fn valid_guests(&self) -> Vec<Guest> {
        self.rows.iter().filter(|r| !r.is_blank()).map(Row::to_guest).collect()
    }

    fn add_new(&mut self) {
        if self.new.name.trim().is_empty() || self.new.table.trim().is_empty() {
            self.err("Enter both a name and a table number to add a guest.");
        } else {
            self.rows.push(std::mem::take(&mut self.new));
            self.status.clear();
        }
        self.focus_name = true;
    }

    fn import(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).pick_file() else { return };
        match read_csv(&path) {
            Ok(gs) => {
                let n = gs.len();
                if self.replace_on_import {
                    self.rows.clear();
                }
                self.rows.extend(gs.into_iter().map(|g| Row {
                    name: g.name,
                    table: g.table,
                    menu: g.menu.join("; "),
                    dietary: g.dietary.join("; "),
                }));
                self.ok(format!("Imported {n} guests from {}", path.display()));
            }
            Err(e) => self.err(format!("Import failed: {e}")),
        }
    }

    fn save_csv(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("CSV", &["csv"])
            .set_file_name("guests.csv")
            .save_file()
        else {
            return;
        };
        let gs = self.valid_guests();
        match write_csv(&path, &gs) {
            Ok(()) => self.ok(format!("Saved {} guests to {}", gs.len(), path.display())),
            Err(e) => self.err(format!("Save failed: {e}")),
        }
    }

    /// Seating plan & menu PDF for back of house (same validation as the cards).
    fn generate_plan(&mut self) {
        let bad = self.rows.iter().filter(|r| r.is_bad()).count();
        if bad > 0 {
            self.err(format!("Fix {bad} highlighted row(s): every guest needs a name and a table."));
            return;
        }
        let gs = self.valid_guests();
        if gs.is_empty() {
            self.err("Add at least one guest first.");
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name("seating-plan.pdf")
            .save_file()
        else {
            return;
        };
        let title = self.title.trim();
        let title = (!title.is_empty()).then_some(title);
        match place_cards::plan::render_plan_ex(&gs, title, self.plan_landscape, self.allergy_marker, &path) {
            Ok(info) => {
                self.ok(format!("Wrote seating plan ({} page(s)) to {}", info.pages, path.display()));
                if let Err(e) = std::process::Command::new("open").arg(&path).spawn() {
                    self.err(format!("PDF written, but could not open it: {e}"));
                }
            }
            Err(e) => self.err(format!("Seating plan failed: {e}")),
        }
    }

    fn generate(&mut self) {
        let bad = self.rows.iter().filter(|r| r.is_bad()).count();
        if bad > 0 {
            self.err(format!("Fix {bad} highlighted row(s): every guest needs a name and a table."));
            return;
        }
        let mut gs = self.valid_guests();
        if gs.is_empty() {
            self.err("Add at least one guest first.");
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name("cards.pdf")
            .save_file()
        else {
            return;
        };
        if self.sort {
            sort_by_table(&mut gs);
        }
        let title = self.title.trim();
        let title = (!title.is_empty()).then_some(title);
        match pdf::render_opts(&gs, title, &self.fonts, &pdf::CardOptions { allergy_marker: self.allergy_marker }, &path) {
            Ok((pages, warnings)) => {
                let mut msg = format!("Wrote {} cards on {pages} page(s) to {}", gs.len(), path.display());
                if !warnings.is_empty() {
                    msg.push_str(&format!(" (warning: {})", warnings.join("; ")));
                }
                self.ok(msg);
                if let Err(e) = std::process::Command::new("open").arg(&path).spawn() {
                    self.err(format!("PDF written, but could not open it: {e}"));
                }
            }
            Err(e) => self.err(format!("PDF generation failed: {e}")),
        }
    }
}

fn field(ui: &mut egui::Ui, text: &mut String, width: f32, hint: &str, bad: bool) -> egui::Response {
    let mut te = egui::TextEdit::singleline(text).desired_width(width).hint_text(hint);
    if bad {
        te = te.background_color(BAD);
    }
    ui.add_sized([width, 22.0], te)
}

fn spec_label(spec: &FontSpec, sys: &[SystemFont]) -> String {
    match spec {
        FontSpec::Builtin(b) => b.name().to_string(),
        FontSpec::Bundled(b) => b.name().to_string(),
        FontSpec::File(p) => sys
            .iter()
            .find(|s| &s.path == p)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
    }
}

fn font_picker(ui: &mut egui::Ui, label: &str, spec: &mut FontSpec, sys: &[SystemFont], filter: &mut String) {
    ui.label(format!("{label}:"));
    egui::ComboBox::from_id_salt(label)
        .width(150.0)
        .selected_text(spec_label(spec, sys))
        .show_ui(ui, |ui| {
            ui.add(egui::TextEdit::singleline(filter).hint_text("Search fonts\u{2026}"));
            let q = filter.to_lowercase();
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for b in Builtin::ALL {
                    if q.is_empty() || b.name().to_lowercase().contains(&q) {
                        ui.selectable_value(spec, FontSpec::Builtin(b), format!("{} (built-in)", b.name()));
                    }
                }
                for b in Bundled::ALL {
                    if q.is_empty() || b.name().to_lowercase().contains(&q) {
                        ui.selectable_value(spec, FontSpec::Bundled(b), format!("{} (bundled)", b.name()));
                    }
                }
                ui.separator();
                for s in sys {
                    if q.is_empty() || s.name.to_lowercase().contains(&q) {
                        ui.selectable_value(spec, FontSpec::File(s.path.clone()), &s.name);
                    }
                }
            });
        });
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.demo {
            return;
        }
        storage.set_string("allergy_marker", if self.allergy_marker { "1" } else { "0" }.to_string());
        let slots = [&self.fonts.title, &self.fonts.name, &self.fonts.table, &self.fonts.menu];
        for (spec, key) in slots.into_iter().zip(FONT_KEYS) {
            storage.set_string(key, spec.to_text());
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.add_space(4.0);
            let n = self.rows.iter().filter(|r| !r.is_blank()).count();
            let bad = self.rows.iter().filter(|r| r.is_bad()).count();
            let pages = pdf::page_count(n);
            ui.horizontal(|ui| {
                ui.label(format!("{n} guest{} -> {pages} page{}", if n == 1 { "" } else { "s" }, if pages == 1 { "" } else { "s" }));
                if bad > 0 {
                    ui.colored_label(ERR, format!("{bad} row(s) need a name and table"));
                }
            });
            if !self.status.is_empty() {
                let t = RichText::new(&self.status);
                ui.label(if self.status_is_error { t.color(ERR) } else { t });
            }
            ui.add_space(4.0);
        });

        self.pfonts.begin_frame();
        egui::SidePanel::right("preview")
            .resizable(true)
            .default_width(460.0)
            .min_width(250.0)
            .show(ctx, |ui| self.preview_ui(ui));
        self.pfonts.end_frame(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Event title:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.title)
                        .hint_text("e.g. Smith Wedding (optional)")
                        .desired_width(320.0),
                );
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Import CSV\u{2026}").clicked() {
                    self.import();
                }
                ui.checkbox(&mut self.replace_on_import, "replace list");
                if ui.button("Save CSV\u{2026}").clicked() {
                    self.save_csv();
                }
                ui.separator();
                ui.checkbox(&mut self.sort, "Sort by table");
                if ui.button(RichText::new("Generate PDF\u{2026}").strong()).clicked() {
                    self.generate();
                }
                ui.separator();
                if ui.button("Seating plan PDF\u{2026}").on_hover_text("Table-by-table menu sheet and kitchen totals").clicked() {
                    self.generate_plan();
                }
                ui.checkbox(&mut self.plan_landscape, "landscape");
            });
            ui.separator();

            let sys: Vec<SystemFont> = self.sys_fonts.lock().ok().and_then(|g| g.clone()).unwrap_or_default();
            egui::CollapsingHeader::new(RichText::new("Fonts").strong()).default_open(true).show(ui, |ui| {
                let scanning = self.sys_fonts.lock().map(|g| g.is_none()).unwrap_or(false);
                if scanning {
                    ui.weak("Scanning installed fonts\u{2026}");
                }
                let filter = &mut self.font_filter;
                let f = &mut self.fonts;
                egui::Grid::new("fonts").num_columns(4).spacing([10.0, 6.0]).show(ui, |ui| {
                    font_picker(ui, "Title", &mut f.title, &sys, filter);
                    font_picker(ui, "Name", &mut f.name, &sys, filter);
                    ui.end_row();
                    font_picker(ui, "Table", &mut f.table, &sys, filter);
                    font_picker(ui, "Menu", &mut f.menu, &sys, filter);
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    if ui.small_button("Reset fonts").clicked() {
                        *f = Fonts::default();
                    }
                    ui.checkbox(&mut self.allergy_marker, "Allergy marker on cards")
                        .on_hover_text("A small dot on cards of guests with a dietary entry (the requirement itself is never printed)");
                });
            });
            ui.separator();

            ui.label(RichText::new("Add guest").strong());
            let mut submit = false;
            ui.horizontal_wrapped(|ui| {
                let r = field(ui, &mut self.new.name, 200.0, "Name", false);
                if self.focus_name {
                    r.request_focus();
                    self.focus_name = false;
                }
                submit |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let r = field(ui, &mut self.new.table, 60.0, "Table", false);
                submit |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let r = field(ui, &mut self.new.menu, 260.0, "Menu (optional, ; separated)", false);
                submit |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let r = field(ui, &mut self.new.dietary, 180.0, "(none)", false);
                submit |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Add guest").clicked() {
                    submit = true;
                }
            });
            if submit {
                self.add_new();
            }
            ui.separator();

            ui.label(RichText::new("Guests").strong());
            let mut delete = None;
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                if self.rows.is_empty() {
                    ui.weak("No guests yet. Add some above or import a CSV.");
                }
                egui::Grid::new("guests").num_columns(6).spacing([8.0, 6.0]).striped(true).show(ui, |ui| {
                    ui.strong("#");
                    ui.strong("");
                    ui.strong("Name");
                    ui.strong("Table No.");
                    ui.strong("Menu choices");
                    ui.strong("Dietary / allergies");
                    ui.end_row();
                    for (i, r) in self.rows.iter_mut().enumerate() {
                        let bad = r.is_bad();
                        let (bad_name, bad_table) = (bad && r.name.trim().is_empty(), bad && r.table.trim().is_empty());
                        if ui.selectable_label(self.selected == Some(i), format!("{}", i + 1)).on_hover_text("Show in preview").clicked() {
                            self.selected = Some(i);
                        }
                        // Remove sits next to the row number so it is always reachable,
                        // however wide the fields to its right are.
                        // Remove sits next to the row number so it is always reachable,
                        // however wide the fields to its right are.
                        if ui.button("Remove").on_hover_text("Delete guest").clicked() {
                            delete = Some(i);
                        }
                        let mut focus = false;
                        focus |= field(ui, &mut r.name, 200.0, "Name", bad_name).gained_focus();
                        focus |= field(ui, &mut r.table, 60.0, "Table", bad_table).gained_focus();
                        focus |= field(ui, &mut r.menu, 300.0, "(no menu)", false).gained_focus();
                        focus |= field(ui, &mut r.dietary, 180.0, "(none)", false).gained_focus();
                        if focus {
                            self.selected = Some(i);
                        }
                        ui.end_row();
                    }
                });
            });
            if let Some(i) = delete {
                self.rows.remove(i);
                self.selected = match self.selected {
                    Some(s) if s == i => None,
                    Some(s) if s > i => Some(s - 1),
                    o => o,
                };
            }
        });
    }
}

/// Window / dock icon (also used by `cargo run`).
fn app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon/icon-256.png")).unwrap_or_default()
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 720.0])
            .with_min_inner_size([760.0, 420.0])
            .with_title("Place Cards")
            .with_icon(std::sync::Arc::new(app_icon())),
        ..Default::default()
    };
    eframe::run_native("Place Cards", options, Box::new(|cc| {
            let mut app = App::new(cc);
            if std::env::var_os("PLACE_CARDS_DEMO").is_some() {
                app.apply_demo();
            }
            Ok(Box::new(app))
        }))
}

// ---------------------------------------------------------------- preview

enum FState {
    Pending(egui::FontFamily),
    Ready(egui::FontFamily),
    Failed,
}

/// Loads font files into egui at runtime (once per font) for the preview.
/// Families only become usable the frame after `set_fonts` was called.
struct PreviewFonts {
    defs: egui::FontDefinitions,
    state: HashMap<FontSpec, FState>,
    pending: Vec<FontSpec>,
    applied: Vec<FontSpec>,
}

fn ps_name(face: &ttf_parser::Face) -> Option<String> {
    face.names()
        .into_iter()
        .filter(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
        .find_map(|n| n.to_string())
}

/// Find the macOS system font matching a built-in PDF font (usually inside a .ttc).
fn find_builtin_face(b: Builtin) -> Option<(Vec<u8>, u32)> {
    let name = b.name();
    let family = name.split('-').next().unwrap_or(name);
    for dir in ["/System/Library/Fonts", "/System/Library/Fonts/Supplemental", "/Library/Fonts"] {
        let path = format!("{dir}/{family}.ttc");
        let Ok(data) = std::fs::read(&path) else { continue };
        let n = ttf_parser::fonts_in_collection(&data).unwrap_or(1);
        for i in 0..n {
            if let Ok(face) = ttf_parser::Face::parse(&data, i) {
                if ps_name(&face).as_deref() == Some(name) {
                    return Some((data, i));
                }
            }
        }
    }
    // Look-alike TrueType families shipped with macOS.
    let (base, style) = match b {
        Builtin::TimesRoman | Builtin::TimesBold | Builtin::TimesItalic | Builtin::TimesBoldItalic => ("Times New Roman", name.trim_start_matches("Times-")),
        Builtin::Courier | Builtin::CourierBold | Builtin::CourierOblique | Builtin::CourierBoldOblique => ("Courier New", name.trim_start_matches("Courier").trim_start_matches('-')),
        _ => ("Arial", name.trim_start_matches("Helvetica").trim_start_matches('-')),
    };
    let suffix = match style {
        "Bold" => " Bold",
        "Italic" | "Oblique" => " Italic",
        "BoldItalic" | "BoldOblique" => " Bold Italic",
        _ => "",
    };
    let data = std::fs::read(format!("/System/Library/Fonts/Supplemental/{base}{suffix}.ttf")).ok()?;
    Some((data, 0))
}

fn load_face(spec: &FontSpec) -> Option<(Vec<u8>, u32)> {
    match spec {
        FontSpec::Builtin(b) => find_builtin_face(*b),
        FontSpec::Bundled(b) => Some((b.bytes().to_vec(), 0)),
        FontSpec::File(p) => {
            let d = std::fs::read(p).ok()?;
            fonts::embeddable(&d).then_some((d, 0))
        }
    }
}

impl PreviewFonts {
    fn new() -> Self {
        Self { defs: egui::FontDefinitions::default(), state: HashMap::new(), pending: Vec::new(), applied: Vec::new() }
    }

    fn begin_frame(&mut self) {
        for spec in self.applied.drain(..) {
            if let Some(FState::Pending(f)) = self.state.remove(&spec) {
                self.state.insert(spec, FState::Ready(f));
            }
        }
    }

    fn end_frame(&mut self, ctx: &egui::Context) {
        if !self.pending.is_empty() {
            ctx.set_fonts(self.defs.clone());
            self.applied.append(&mut self.pending);
            ctx.request_repaint();
        }
    }

    /// The egui family for `spec`, or None while loading / if it cannot be loaded.
    fn family(&mut self, spec: &FontSpec) -> Option<egui::FontFamily> {
        if !self.state.contains_key(spec) {
            let loaded = std::panic::catch_unwind(|| load_face(spec)).ok().flatten();
            match loaded {
                Some((bytes, index)) => {
                    let key = format!("pc:{}", spec.to_text());
                    let mut data = egui::FontData::from_owned(bytes);
                    data.index = index;
                    self.defs.font_data.insert(key.clone(), Arc::new(data));
                    let mut chain = vec![key.clone()];
                    // Glyphs the font lacks fall back to egui's defaults.
                    chain.extend(self.defs.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default());
                    let fam = egui::FontFamily::Name(key.into());
                    self.defs.families.insert(fam.clone(), chain);
                    self.state.insert(spec.clone(), FState::Pending(fam));
                    self.pending.push(spec.clone());
                }
                None => {
                    self.state.insert(spec.clone(), FState::Failed);
                }
            }
        }
        match self.state.get(spec) {
            Some(FState::Ready(f)) => Some(f.clone()),
            _ => None,
        }
    }
}

impl App {
    /// The guest to preview and its position in the generated order (for page view).
    fn preview_guests(&self) -> (Vec<Guest>, usize) {
        let mut idx: Vec<(usize, Guest)> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.is_blank())
            .map(|(i, r)| (i, r.to_guest()))
            .collect();
        if self.sort {
            idx.sort_by_cached_key(|(_, g)| table_sort_key(g));
        }
        if idx.is_empty() {
            let n = &self.new;
            let g = if n.is_blank() {
                Guest { name: "Guest Name".into(), table: "1".into(), menu: parse_menu("Starter: Soup; Main: Beef"), dietary: vec![] }
            } else {
                n.to_guest()
            };
            return (vec![g], 0);
        }
        let pos = self.selected.and_then(|s| idx.iter().position(|(i, _)| *i == s)).unwrap_or(0);
        (idx.into_iter().map(|(_, g)| g).collect(), pos)
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Preview").strong());
            ui.selectable_value(&mut self.page_view, false, "Card");
            ui.selectable_value(&mut self.page_view, true, "Page");
        });
        let (mut guests, pos) = self.preview_guests();
        for g in &mut guests {
            if g.name.is_empty() {
                g.name = "Guest Name".into();
            }
            if g.table.is_empty() {
                g.table = "?".into();
            }
        }
        let title = self.title.trim().to_string();
        let title = (!title.is_empty()).then_some(title.as_str());
        let (eff, warns) = self.measurer.resolve(&self.fonts);

        // Which guests and slots to draw, and the visible region (page mm).
        let (slots, x0, y_top, w_mm, h_mm): (Vec<(usize, &Guest)>, f32, f32, f32, f32) = if self.page_view {
            let page = pos / (pdf::COLS * pdf::ROWS);
            let start = page * pdf::COLS * pdf::ROWS;
            let s = guests.iter().skip(start).take(pdf::COLS * pdf::ROWS).enumerate().collect();
            (s, 0.0, pdf::PAGE_H, pdf::PAGE_W, pdf::PAGE_H)
        } else {
            (vec![(0, &guests[pos])], pdf::MARGIN, pdf::PAGE_H - pdf::MARGIN, pdf::CELL_W, pdf::CELL_H)
        };
        if self.page_view {
            ui.weak(format!("Page {} of {}", pos / (pdf::COLS * pdf::ROWS) + 1, pdf::page_count(guests.len())));
        } else {
            ui.weak(format!("Card for {}", guests[pos].name));
        }

        let avail = ui.available_size();
        let reserve = 70.0;
        let scale = ((avail.x - 8.0) / w_mm).min((avail.y - reserve) / h_mm).max(0.5); // points per mm
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w_mm * scale, h_mm * scale), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::WHITE);
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0_f32, Color32::from_gray(200)), egui::StrokeKind::Inside);
        let to_screen = |x: f32, y: f32| egui::pos2(rect.left() + (x - x0) * scale, rect.top() + (y_top - y) * scale);
        let cut = egui::Stroke::new(1.0_f32, Color32::from_gray(178));
        let fold = egui::Stroke::new(1.0_f32, Color32::from_gray(217));
        let line = |a: (f32, f32), b: (f32, f32), st: egui::Stroke, dash: f32, gap: f32| {
            painter.extend(egui::Shape::dashed_line(&[to_screen(a.0, a.1), to_screen(b.0, b.1)], st, dash * scale, gap * scale));
        };
        if self.page_view {
            for c in 0..=pdf::COLS {
                let x = pdf::MARGIN + c as f32 * pdf::CELL_W;
                line((x, pdf::MARGIN), (x, pdf::PAGE_H - pdf::MARGIN), cut, 3.0, 2.0);
            }
            for r in 0..=pdf::ROWS {
                let y = pdf::MARGIN + r as f32 * pdf::CELL_H;
                line((pdf::MARGIN, y), (pdf::PAGE_W - pdf::MARGIN, y), cut, 3.0, 2.0);
            }
            for r in 0..pdf::ROWS {
                let y = pdf::MARGIN + (r as f32 + 0.5) * pdf::CELL_H;
                line((pdf::MARGIN, y), (pdf::PAGE_W - pdf::MARGIN, y), fold, 1.0, 2.0);
            }
        } else {
            let (l, r) = (pdf::MARGIN, pdf::MARGIN + pdf::CELL_W);
            let (b, t) = (pdf::PAGE_H - pdf::MARGIN - pdf::CELL_H, pdf::PAGE_H - pdf::MARGIN);
            line((l, b), (r, b), cut, 3.0, 2.0);
            line((r, b), (r, t), cut, 3.0, 2.0);
            line((r, t), (l, t), cut, 3.0, 2.0);
            line((l, t), (l, b), cut, 3.0, 2.0);
            line((l, (b + t) / 2.0), (r, (b + t) / 2.0), fold, 1.0, 2.0);
        }

        let mut approx: Vec<String> = Vec::new();
        for (slot, g) in slots {
            let (cx, cy) = if self.page_view { pdf::cell_center(slot) } else { pdf::cell_center(0) };
            let cell = pdf::layout_cell_ex(g, title, &eff, &mut self.measurer, cx, cy, self.allergy_marker);
            for mk in &cell.markers {
                painter.circle_filled(to_screen(mk.x_mm, mk.y_mm), mk.r_mm * scale, Color32::BLACK);
            }
            let items: Vec<PlacedText> = cell.texts;
            for t in &items {
                let fam = self.pfonts.family(&t.font);
                let exact = fam.is_some();
                if !exact && !approx.contains(&t.font.to_text()) {
                    approx.push(spec_label(&t.font, &[]));
                }
                let family = fam.unwrap_or(match &t.font {
                    FontSpec::Builtin(Builtin::Courier | Builtin::CourierBold | Builtin::CourierOblique | Builtin::CourierBoldOblique) => egui::FontFamily::Monospace,
                    _ => egui::FontFamily::Proportional,
                });
                let fid = egui::FontId::new(t.size_pt * pdf::PT_MM * scale, family);
                let col = Color32::from_gray((t.grey * 255.0) as u8);
                let galley = painter.layout_no_wrap(t.text.clone(), fid, col);
                let asc = galley
                    .rows
                    .first()
                    .and_then(|r| r.glyphs.first())
                    .map(|g| g.pos.y)
                    .unwrap_or(t.size_pt * pdf::PT_MM * scale * 0.8);
                // Exact fonts use the layout's start point; approximate ones are re-centred.
                let x_mm = if exact {
                    t.x_mm
                } else if t.rotated {
                    cx + galley.size().x / scale / 2.0
                } else {
                    cx - galley.size().x / scale / 2.0
                };
                let p = to_screen(x_mm, t.y_mm);
                if t.rotated {
                    painter.add(egui::epaint::TextShape::new(p + egui::vec2(0.0, asc), galley, col).with_angle(std::f32::consts::PI));
                } else {
                    painter.add(egui::epaint::TextShape::new(p - egui::vec2(0.0, asc), galley, col));
                }
            }
        }
        if !approx.is_empty() {
            ui.weak(format!("Preview is approximate for: {} (font not previewable).", approx.join(", ")));
        }
        for w in warns {
            ui.colored_label(ERR, w);
        }
    }
}
