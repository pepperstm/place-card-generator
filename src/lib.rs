//! Shared code for the place-cards CLI and GUI.

pub mod cff;
pub mod fonts;
pub mod metrics;
pub mod pdf;
pub mod plan;

use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Guest {
    pub name: String,
    pub table: String,
    pub menu: Vec<String>,
    /// Dietary requirements / allergies (never printed on place cards).
    pub dietary: Vec<String>,
}

/// Split a `;`-separated menu field into trimmed, non-empty choices.
pub fn parse_menu(s: &str) -> Vec<String> {
    s.split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

/// Read guests from a CSV file with header `name,table,menu[,dietary]`
/// (columns are found by header name; `menu` and `dietary`/`allergies` are optional).
pub fn read_csv(path: &Path) -> Result<Vec<Guest>, Box<dyn std::error::Error>> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_path(path)?;
    let headers = rdr.headers()?.clone();
    let col = |n: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(n));
    let name_i = col("name").ok_or("CSV header must contain a 'name' column")?;
    let table_i = col("table").ok_or("CSV header must contain a 'table' column")?;
    let menu_i = col("menu");
    let diet_i = ["dietary", "allergies", "dietary/allergies"].iter().find_map(|n| col(n));
    let mut guests = Vec::new();
    for (i, rec) in rdr.records().enumerate() {
        let rec = rec?;
        let line = i + 2;
        let name = rec.get(name_i).unwrap_or("").to_string();
        let table = rec.get(table_i).unwrap_or("").to_string();
        if name.is_empty() && table.is_empty() {
            continue; // blank row
        }
        if name.is_empty() || table.is_empty() {
            return Err(format!("line {line}: both name and table are required").into());
        }
        let menu = menu_i.and_then(|m| rec.get(m)).map(parse_menu).unwrap_or_default();
        let dietary = diet_i.and_then(|d| rec.get(d)).map(parse_menu).unwrap_or_default();
        guests.push(Guest { name, table, menu, dietary });
    }
    Ok(guests)
}

/// Write guests to a CSV file with header `name,table,menu,dietary`.
pub fn write_csv(path: &Path, guests: &[Guest]) -> Result<(), Box<dyn std::error::Error>> {
    let mut w = csv::Writer::from_path(path)?;
    w.write_record(["name", "table", "menu", "dietary"])?;
    for g in guests {
        w.write_record([g.name.as_str(), g.table.as_str(), g.menu.join("; ").as_str(), g.dietary.join("; ").as_str()])?;
    }
    w.flush()?;
    Ok(())
}

/// Sort by table (numeric tables first, in numeric order), then name.
pub fn sort_by_table(guests: &mut [Guest]) {
    guests.sort_by_cached_key(table_sort_key);
}

/// The key `sort_by_table` orders by (exposed so callers can sort index lists identically).
pub fn table_sort_key(g: &Guest) -> (bool, u32, String, String) {
    let n = g.table.parse::<u32>().ok();
    (n.is_none(), n.unwrap_or(0), g.table.to_lowercase(), g.name.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_roundtrip_and_pdf() {
        let dir = std::env::temp_dir();
        let csv = dir.join("pc_test.csv");
        let pdf_path = dir.join("pc_test.pdf");
        let guests: Vec<Guest> = (1..=8)
            .map(|i| Guest { name: format!("Guest {i}"), table: (9 - i).to_string(), menu: parse_menu("A: x; B: y"), dietary: vec![] })
            .collect();
        write_csv(&csv, &guests).unwrap();
        let mut back = read_csv(&csv).unwrap();
        assert_eq!(back.len(), 8);
        assert_eq!(back[0].menu, vec!["A: x", "B: y"]);
        sort_by_table(&mut back);
        assert_eq!(back[0].table, "1");
        assert_eq!(pdf::render(&back, Some("T"), &pdf_path).unwrap(), 2);
        let bytes = std::fs::read(&pdf_path).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    fn stable(bytes: &[u8]) -> Vec<Vec<u8>> {
        bytes
            .split(|b| *b == b'\n')
            .filter(|l| {
                let t = String::from_utf8_lossy(l).to_lowercase();
                !(t.contains("date") || t.contains("/id") || t.contains("instanceid") || t.contains("documentid"))
            })
            .map(|l| l.to_vec())
            .collect()
    }

    fn sample() -> Vec<Guest> {
        read_csv(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/guests.csv"))).unwrap()
    }

    #[test]
    fn defaults_match_original_output() {
        let mut gs = sample();
        sort_by_table(&mut gs);
        let out = std::env::temp_dir().join("pc_default.pdf");
        // marker OFF: dietary markers are a deliberate addition; the old fixture must still match
        let off = pdf::CardOptions { allergy_marker: false };
        pdf::render_opts(&gs, Some("Smith Wedding"), &fonts::Fonts::default(), &off, &out).unwrap();
        let new = std::fs::read(&out).unwrap();
        let old = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/sample.pdf")).unwrap();
        assert_eq!(stable(&new), stable(&old));
    }

    #[test]
    fn mixed_builtin_fonts_match_baseline() {
        use fonts::{Builtin::*, FontSpec::Builtin as B, Fonts};
        let mut gs = sample();
        sort_by_table(&mut gs);
        let out = std::env::temp_dir().join("pc_mixed.pdf");
        let f = Fonts { title: B(CourierOblique), name: B(TimesBold), table: B(TimesItalic), menu: B(Courier) };
        let off = pdf::CardOptions { allergy_marker: false };
        pdf::render_opts(&gs, Some("Smith Wedding"), &f, &off, &out).unwrap();
        let new = std::fs::read(&out).unwrap();
        let old = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mixed.pdf")).unwrap();
        assert_eq!(stable(&new), stable(&old));
    }

    #[test]
    fn layout_is_symmetric_and_inside_cell() {
        let mut m = fonts::Measurer::new();
        let g = &sample()[6]; // very long name
        let (cx, cy) = pdf::cell_center(0);
        let items = pdf::layout_cell(g, Some("T"), &fonts::Fonts::default(), &mut m, cx, cy);
        let n = items.len() / 2;
        for k in 0..n {
            let (a, b) = (&items[k], &items[k + n]);
            assert!(!a.rotated && b.rotated);
            assert!((a.x_mm + b.x_mm - 2.0 * cx).abs() < 1e-3);
            assert!((a.y_mm + b.y_mm - 2.0 * cy).abs() < 1e-3);
            let w = m.width_mm(&a.font, &a.text, a.size_pt);
            assert!(w <= pdf::CELL_W - 10.0 + 0.01, "{} too wide", a.text);
        }
    }

    #[test]
    fn csv_dietary_column_and_compat() {
        let dir = std::env::temp_dir();
        // old 3-column file
        let old = dir.join("pc_old.csv");
        std::fs::write(&old, "name,table,menu\nAnn,1,Main: x\n").unwrap();
        let g = read_csv(&old).unwrap();
        assert!(g[0].dietary.is_empty() && g[0].menu == vec!["Main: x"]);
        // no menu column at all
        let nomenu = dir.join("pc_nomenu.csv");
        std::fs::write(&nomenu, "name,table\nAnn,1\n").unwrap();
        assert!(read_csv(&nomenu).unwrap()[0].menu.is_empty());
        // header aliases, any case/order
        for h in ["dietary", "Allergies", "DIETARY/ALLERGIES"] {
            let p = dir.join("pc_alias.csv");
            std::fs::write(&p, format!("{h},table,name\n\"Vegan; Nut allergy\",2,Bo\n")).unwrap();
            let g = read_csv(&p).unwrap();
            assert_eq!((g[0].name.as_str(), g[0].table.as_str()), ("Bo", "2"));
            assert_eq!(g[0].dietary, vec!["Vegan", "Nut allergy"], "{h}");
        }
        // writing always includes the column and round-trips
        let out = dir.join("pc_diet_rt.csv");
        write_csv(&out, &sample()).unwrap();
        assert!(std::fs::read_to_string(&out).unwrap().starts_with("name,table,menu,dietary"));
        let back = read_csv(&out).unwrap();
        assert_eq!(back[1].dietary, vec!["Gluten free", "Shellfish allergy"]);
        assert_eq!(back[2].menu.len(), 0);
        assert_eq!(back[2].dietary, vec!["Nut allergy"]); // allergy but no menu
    }

    fn diet_guest(name: &str, menu: &str) -> Guest {
        Guest { name: name.into(), table: "1".into(), menu: parse_menu(menu), dietary: parse_menu("Nut allergy") }
    }

    #[test]
    fn marker_only_for_guests_with_dietary_entries() {
        let mut m = fonts::Measurer::new();
        let f = fonts::Fonts::default();
        let (cx, cy) = pdf::cell_center(0);
        let mut with = 0;
        for g in sample() {
            let on = pdf::layout_cell_ex(&g, Some("T"), &f, &mut m, cx, cy, true);
            let off = pdf::layout_cell_ex(&g, Some("T"), &f, &mut m, cx, cy, false);
            assert!(off.markers.is_empty());
            assert_eq!(on.texts, off.texts, "marker must not move any text");
            if g.dietary.is_empty() {
                assert!(on.markers.is_empty(), "{}", g.name);
            } else {
                with += 1;
                assert_eq!(on.markers.len(), 2, "{}", g.name);
                // one per half, mirrored about the cell centre
                let (a, b) = (&on.markers[0], &on.markers[1]);
                assert!(!a.rotated && b.rotated);
                assert!((a.x_mm + b.x_mm - 2.0 * cx).abs() < 1e-3 && (a.y_mm + b.y_mm - 2.0 * cy).abs() < 1e-3);
                assert!(a.y_mm < cy && b.y_mm > cy, "upright half is the bottom half");
            }
        }
        assert_eq!(with, 4);
    }

    #[test]
    fn marker_lies_in_padding_and_clear_of_text() {
        use fonts::{Builtin, FontSpec, Fonts};
        let mut m = fonts::Measurer::new();
        let (cx, cy) = pdf::cell_center(3);
        let long = "Maria Fernanda de los Angeles Montgomery-Wentworth-Smythe III Esquire of Somewhere Far Away";
        let guests = [
            diet_guest("Bo", ""),
            diet_guest(long, "Starter: Soup of the day with a long description; Main: Beef; Dessert: Sticky toffee pudding"),
            diet_guest("Zoë Müller", "Starter: A; Main: B; Dessert: C; Cheese: D"),
        ];
        let fonts_set = [
            Fonts::default(),
            Fonts { name: FontSpec::Builtin(Builtin::CourierBold), menu: FontSpec::Builtin(Builtin::TimesItalic), ..Fonts::default() },
        ];
        for f in &fonts_set {
            for g in &guests {
                let c = pdf::layout_cell_ex(g, Some("A Title For The Event"), f, &mut m, cx, cy, true);
                for mk in &c.markers {
                    // at least 4.5 mm from the cut lines (right and bottom edges of the upright
                    // half; mirrored for the rotated half), and well away from the fold
                    let (l, r, b, t) = (cx - pdf::CELL_W / 2.0, cx + pdf::CELL_W / 2.0, cy - pdf::CELL_H / 2.0, cy + pdf::CELL_H / 2.0);
                    for d in [mk.x_mm - mk.r_mm - l, r - (mk.x_mm + mk.r_mm), mk.y_mm - mk.r_mm - b, t - (mk.y_mm + mk.r_mm)] {
                        assert!(d >= 4.5 || d > 20.0, "clearance {d} mm from a cut line");
                    }
                    assert!((mk.y_mm - cy).abs() - mk.r_mm > 15.0, "clear of the fold line");
                    // no overlap with any text item's bounding box
                    for t in c.texts.iter().filter(|t| t.rotated == mk.rotated) {
                        let w = m.width_mm(&t.font, &t.text, t.size_pt);
                        let h = t.size_pt * pdf::PT_MM;
                        let (x0, x1) = if t.rotated { (t.x_mm - w, t.x_mm) } else { (t.x_mm, t.x_mm + w) };
                        let (y0, y1) = if t.rotated { (t.y_mm - h, t.y_mm + h * 0.3) } else { (t.y_mm - h * 0.3, t.y_mm + h) };
                        let overlap = mk.x_mm + mk.r_mm > x0 && mk.x_mm - mk.r_mm < x1 && mk.y_mm + mk.r_mm > y0 && mk.y_mm - mk.r_mm < y1;
                        assert!(!overlap, "marker overlaps '{}'", t.text);
                    }
                }
            }
        }
    }

    #[test]
    fn cards_pdf_with_marker_loads_and_differs() {
        let dir = std::env::temp_dir();
        let (on, off) = (dir.join("pc_mk_on.pdf"), dir.join("pc_mk_off.pdf"));
        let f = fonts::Fonts::default();
        pdf::render_opts(&sample(), None, &f, &pdf::CardOptions { allergy_marker: true }, &on).unwrap();
        pdf::render_opts(&sample(), None, &f, &pdf::CardOptions { allergy_marker: false }, &off).unwrap();
        assert_eq!(printpdf::lopdf::Document::load(&on).unwrap().get_pages().len(), 2);
        assert_ne!(std::fs::read(&on).unwrap().len(), std::fs::read(&off).unwrap().len());
        // plan key only when someone has dietary entries
        let l = plan::layout_plan_ex(&sample(), None, "d", false, true);
        assert!(l.pages[0].items.iter().any(|i| matches!(i, plan::PlanItem::Dot { .. })));
        let l = plan::layout_plan_ex(&sample()[..1], None, "d", false, true);
        assert!(!l.pages[0].items.iter().any(|i| matches!(i, plan::PlanItem::Dot { .. })));
        let l = plan::layout_plan_ex(&sample(), None, "d", false, false);
        assert!(!l.pages[0].items.iter().any(|i| matches!(i, plan::PlanItem::Dot { .. })));
    }

    #[test]
    fn bundled_eb_garamond() {
        use fonts::{Bundled, FontSpec, Fonts};
        assert_eq!(FontSpec::parse("eb garamond bold italic").unwrap(), FontSpec::Bundled(Bundled::BoldItalic));
        for b in Bundled::ALL {
            let face = ttf_parser::Face::parse(b.bytes(), 0).unwrap();
            let fam = face.names().into_iter().find(|n| n.name_id == ttf_parser::name_id::FAMILY).and_then(|n| n.to_string());
            assert_eq!(fam.as_deref(), Some("EB Garamond"));
        }
        let out = std::env::temp_dir().join("pc_garamond.pdf");
        let f = Fonts {
            name: FontSpec::Bundled(Bundled::Bold),
            menu: FontSpec::Bundled(Bundled::Italic),
            title: FontSpec::Bundled(Bundled::Bold),
            table: FontSpec::Bundled(Bundled::Regular),
        };
        let (pages, warn) = pdf::render_with(&sample(), Some("T"), &f, &out).unwrap();
        assert_eq!((pages, warn.len()), (2, 0));
        let doc = printpdf::lopdf::Document::load(&out).unwrap();
        assert_eq!(doc.get_pages().len(), 2);
        assert!(String::from_utf8_lossy(&std::fs::read(&out).unwrap()).contains("CIDFontType2"));
    }

    #[test]
    fn cff_otf_font() {
        use fonts::{Bundled, FontSpec, Fonts};
        let p = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join("Library/Fonts/GaramondPremrPro-Bd.otf");
        if !p.exists() {
            return;
        }
        let data = std::fs::read(&p).unwrap();
        assert!(fonts::is_cff(&data) && !fonts::is_truetype(&data));
        let out = std::env::temp_dir().join("pc_cff.pdf");
        let f = Fonts {
            name: FontSpec::File(p.clone()),
            title: FontSpec::File(p),
            menu: FontSpec::Bundled(Bundled::Italic),
            table: FontSpec::Builtin(fonts::Builtin::Helvetica),
        };
        let (pages, warn) = pdf::render_with(&sample(), Some("Smith"), &f, &out).unwrap();
        assert_eq!((pages, warn.len()), (2, 0), "{warn:?}");
        let doc = printpdf::lopdf::Document::load(&out).unwrap();
        assert_eq!(doc.get_pages().len(), 2);
        let raw = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
        assert!(raw.contains("CIDFontType0") && raw.contains("OpenType") && raw.contains("GaramondPremrPro-Bd"));
        // The CFF font is listed in the system font scan too.
        assert!(fonts::list_system_fonts().iter().any(|f| f.name.to_lowercase().contains("garamond")));
    }

    #[test]
    fn bogus_otf_falls_back() {
        use fonts::{FontSpec, Fonts};
        let bad = std::env::temp_dir().join("pc_bad.otf");
        std::fs::write(&bad, b"OTTO not really").unwrap();
        let out = std::env::temp_dir().join("pc_bad_otf.pdf");
        let f = Fonts { name: FontSpec::File(bad), ..Fonts::default() };
        let (_, warn) = pdf::render_with(&sample(), None, &f, &out).unwrap();
        assert_eq!(warn.len(), 1);
    }

    #[test]
    fn builtin_times_bold_name() {
        use fonts::{Builtin, FontSpec, Fonts};
        let out = std::env::temp_dir().join("pc_times.pdf");
        let f = Fonts { name: FontSpec::Builtin(Builtin::TimesBold), menu: FontSpec::Builtin(Builtin::CourierOblique), ..Fonts::default() };
        let (pages, warn) = pdf::render_with(&sample(), None, &f, &out).unwrap();
        assert_eq!(pages, 2);
        assert!(warn.is_empty());
        let b = std::fs::read(&out).unwrap();
        assert!(String::from_utf8_lossy(&b).contains("Times-Bold"));
        assert!(fonts::Builtin::from_name("times-bold").is_some());
    }

    #[test]
    fn system_ttf_and_bad_font() {
        use fonts::{FontSpec, Fonts};
        let out = std::env::temp_dir().join("pc_ttf.pdf");
        let p = std::path::PathBuf::from("/System/Library/Fonts/Supplemental/Georgia.ttf");
        if p.exists() {
            let f = Fonts { name: FontSpec::File(p.clone()), title: FontSpec::File(p), ..Fonts::default() };
            let (pages, warn) = pdf::render_with(&sample(), Some("T"), &f, &out).unwrap();
            assert_eq!(pages, 2);
            assert!(warn.is_empty(), "{warn:?}");
            let w = fonts::ttf_width_em(&std::fs::read("/System/Library/Fonts/Supplemental/Georgia.ttf").unwrap(), "Hello").unwrap();
            assert!(w > 1.5 && w < 4.0);
        }
        // A non-font file must fall back with a warning, not fail.
        let bad = std::env::temp_dir().join("pc_bad.ttf");
        std::fs::write(&bad, b"not a font").unwrap();
        let f = Fonts { name: FontSpec::File(bad), ..Fonts::default() };
        let (_, warn) = pdf::render_with(&sample(), None, &f, &out).unwrap();
        assert_eq!(warn.len(), 1);
    }

    #[test]
    fn lists_system_fonts() {
        let l = fonts::list_system_fonts();
        eprintln!("{} system fonts", l.len());
        for f in l.iter().filter(|f| f.name.to_lowercase().contains("garamond")).take(6) { eprintln!("  {} -> {}", f.name, f.path.display()); }
        let mut names: Vec<_> = l.iter().map(|f| f.name.to_lowercase()).collect();
        let n = names.len();
        names.dedup();
        assert_eq!(n, names.len());
    }
}
