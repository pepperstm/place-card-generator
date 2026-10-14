//! Shared code for the place-cards CLI and GUI.

pub mod cff;
pub mod courses;
pub mod dietary;
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
    /// Free-text pre-order note for the kitchen (e.g. a dessert swap). Counts as a modified plate.
    pub notes: String,
}

/// A note about a whole table (a "for the table" order, e.g. shared sides).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableNote {
    pub table: String,
    pub text: String,
}

/// Split a `;`-separated menu field into trimmed, non-empty choices.
pub fn parse_menu(s: &str) -> Vec<String> {
    s.split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

/// What a CSV import found.
pub struct CsvImport {
    pub guests: Vec<Guest>,
    /// The event's courses (inferred, or the caller's with any new labels appended).
    pub courses: Vec<String>,
    /// True when no `base` courses were given and the courses were inferred from the file.
    pub inferred: bool,
    /// Problems worth showing (e.g. extra unlabelled menu entries that ended up in "Other").
    pub warnings: Vec<String>,
    /// "For the table" rows (shared orders for a whole table); they are not guests.
    pub table_notes: Vec<TableNote>,
    /// Pre-order notes whose guest name matched nobody.
    pub unmatched_notes: Vec<(String, String)>,
    /// The file has no table column: every guest was put on `default_table`.
    pub no_table: bool,
    pub default_table: String,
    /// A Collins-style food sheet (course columns plus a "For the table" row and/or a notes block).
    pub collins: bool,
    /// Pre-order notes attached to a guest.
    pub notes_attached: usize,
}

impl CsvImport {
    /// "Imported Collins food sheet: 33 guests, courses Starter, Main, Dessert, 1 'for the table' order, 2 pre-order notes"
    pub fn collins_summary(&self) -> String {
        format!(
            "Imported Collins food sheet: {} guests, courses {}, {} 'for the table' order{}, {} pre-order note{}",
            self.guests.len(),
            self.courses.join(", "),
            self.table_notes.len(),
            if self.table_notes.len() == 1 { "" } else { "s" },
            self.notes_attached + self.unmatched_notes.len(),
            if self.notes_attached + self.unmatched_notes.len() == 1 { "" } else { "s" },
        )
    }

    /// "No table numbers in file: 33 guests put on table 4; edit if needed" (only when the file has no table column).
    pub fn table_note(&self) -> Option<String> {
        self.no_table.then(|| format!("No table numbers in file: {} guests put on table {}; edit if needed", self.guests.len(), self.default_table))
    }
}

fn norm_name(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn norm_header(s: &str) -> String {
    s.trim_start_matches('\u{feff}').trim().trim_end_matches('.').trim().to_lowercase()
}

/// The next unused table number for appended guests (largest numeric table + 1, else 1).
pub fn next_free_table(existing: &[String]) -> String {
    let max = existing.iter().filter_map(|t| t.trim().parse::<u32>().ok()).max().unwrap_or(0);
    (max + 1).to_string()
}

/// Read guests from a CSV file (see `read_csv_opts`). Menu entries come out labelled with their course.
pub fn read_csv(path: &Path) -> Result<Vec<Guest>, Box<dyn std::error::Error>> {
    Ok(read_csv_ex(path, None)?.guests)
}

/// `read_csv_opts` with guests on table "1" when the file has no table column.
pub fn read_csv_ex(path: &Path, base: Option<&[String]>) -> Result<CsvImport, Box<dyn std::error::Error>> {
    read_csv_opts(path, base, "1")
}

/// Read guests from a CSV file, finding columns by header name (a leading BOM is ignored):
/// name (`Name`, `Guest`, `Guest name`), table (`Table`, `Table No`, `Table number`), dietary
/// (`dietary`, `allergies`, `dietary/allergies`, `Dietary requirements`), `notes`, and either a single
/// `menu` column or, if there is none, **course columns**: every other column with a header, in header
/// order, named by that header (a column no guest uses is dropped; an empty-header column of row numbers
/// is ignored). In menu mode labelled entries ("Main: Beef") go to that course and unlabelled ones are
/// assigned by position to the event's courses, which are `base` if given or inferred from the file.
///
/// Rows named "For the table" are returned as `table_notes`, not guests. After a blank row, a
/// "CUSTOMER PRE-ORDER NOTES" row introduces `name,note` rows that are attached to the matching guest.
/// Without a table column all guests go on `default_table`.
pub fn read_csv_opts(path: &Path, base: Option<&[String]>, default_table: &str) -> Result<CsvImport, Box<dyn std::error::Error>> {
    let mut bytes = std::fs::read(path)?;
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes.drain(..3);
    }
    let mut rdr = csv::ReaderBuilder::new().has_headers(false).flexible(true).trim(csv::Trim::All).from_reader(&bytes[..]);
    let mut recs: Vec<Vec<String>> = Vec::new();
    for r in rdr.records() {
        recs.push(r?.iter().map(|c| c.to_string()).collect());
    }
    if recs.is_empty() {
        return Err("the CSV file is empty".into());
    }
    let hdr: Vec<String> = recs[0].iter().map(|h| norm_header(h)).collect();
    let find = |names: &[&str]| hdr.iter().position(|h| names.contains(&h.as_str()));
    let name_i = find(&["name", "guest", "guest name"]).ok_or("CSV header must contain a 'name' column")?;
    let table_i = find(&["table", "table no", "table number"]);
    let menu_i = find(&["menu"]);
    let diet_i = find(&["dietary", "allergies", "dietary/allergies", "dietary requirements"]);
    let notes_i = find(&["notes", "note"]);
    let known = |j: usize| Some(j) == Some(name_i) || Some(j) == table_i || Some(j) == menu_i || Some(j) == diet_i || Some(j) == notes_i;
    // An empty-header column of 1, 2, 3... is a running row number: ignored.
    let numbered_col = (0..hdr.len()).any(|j| {
        hdr[j].is_empty()
            && recs[1..].iter().filter_map(|r| r.get(j)).filter(|c| !c.is_empty()).all(|c| c.parse::<u32>().is_ok())
            && recs[1..].iter().any(|r| r.get(j).map(|c| !c.is_empty()).unwrap_or(false))
    });
    let course_cols: Vec<usize> = if menu_i.is_some() { Vec::new() } else { (0..hdr.len()).filter(|j| !known(*j) && !hdr[*j].is_empty()).collect() };
    let course_mode = !course_cols.is_empty();

    struct Raw {
        name: String,
        table: String,
        slots: Vec<String>,
        dishes: Vec<(usize, String)>,
        dietary: Vec<String>,
        notes: String,
    }
    let mut raw: Vec<Raw> = Vec::new();
    let mut table_notes: Vec<TableNote> = Vec::new();
    let mut pre_notes: Vec<(String, String)> = Vec::new();
    let mut in_notes = false;
    let mut notes_block = false;
    let mut no_table = false;
    for (i, rec) in recs.iter().enumerate().skip(1) {
        let line = i + 1;
        let cell = |j: Option<usize>| j.and_then(|j| rec.get(j)).map(|s| s.as_str()).unwrap_or("");
        if rec.iter().all(|c| c.is_empty()) {
            continue;
        }
        if rec.first().map(|c| c.trim().eq_ignore_ascii_case("customer pre-order notes")).unwrap_or(false) {
            in_notes = true;
            notes_block = true;
            continue;
        }
        if in_notes {
            let name = rec.first().map(|s| s.as_str()).unwrap_or("");
            let note = rec.get(1).map(|s| s.as_str()).unwrap_or("");
            if !name.is_empty() && !note.is_empty() {
                pre_notes.push((name.to_string(), note.to_string()));
            }
            continue;
        }
        let name = cell(Some(name_i)).to_string();
        let table = cell(table_i).to_string();
        if norm_name(&name) == "for the table" {
            let text = if course_mode {
                let parts: Vec<(String, String)> = course_cols.iter().filter(|j| !cell(Some(**j)).is_empty()).map(|j| (recs[0][*j].trim().to_string(), cell(Some(*j)).to_string())).collect();
                if parts.len() == 1 { parts[0].1.clone() } else { parts.iter().map(|(h, v)| format!("{h}: {v}")).collect::<Vec<_>>().join(" \u{b7} ") }
            } else {
                let n = cell(notes_i);
                if n.is_empty() { cell(menu_i).to_string() } else { n.to_string() }
            };
            if !text.is_empty() {
                table_notes.push(TableNote { table: if table_i.is_some() && !table.is_empty() { table } else { default_table.to_string() }, text });
            }
            continue;
        }
        if name.is_empty() && table.is_empty() && (table_i.is_some() || course_mode && course_cols.iter().all(|j| cell(Some(*j)).is_empty())) {
            continue;
        }
        let table = if table_i.is_some() {
            if name.is_empty() || table.is_empty() {
                return Err(format!("line {line}: both name and table are required").into());
            }
            table
        } else {
            if name.is_empty() {
                continue;
            }
            no_table = true;
            default_table.to_string()
        };
        let dietary = parse_menu(cell(diet_i));
        let notes = cell(notes_i).to_string();
        if course_mode {
            let dishes = course_cols.iter().filter(|j| !cell(Some(**j)).is_empty()).map(|j| (*j, cell(Some(*j)).to_string())).collect();
            raw.push(Raw { name, table, slots: Vec::new(), dishes, dietary, notes });
        } else {
            raw.push(Raw { name, table, slots: courses::split_slots(cell(menu_i)), dishes: Vec::new(), dietary, notes });
        }
    }

    // courses
    let used: Vec<usize> = course_cols.iter().copied().filter(|j| raw.iter().any(|r| r.dishes.iter().any(|(c, _)| c == j))).collect();
    let course_list: Vec<String> = if course_mode {
        let mut list: Vec<String> = base.map(|b| b.to_vec()).unwrap_or_default();
        for j in &used {
            let h = recs[0][*j].trim().to_string();
            if !list.iter().any(|x| courses::same_name(x, &h)) {
                list.push(h);
            }
        }
        if list.is_empty() {
            courses::default_names(3)
        } else {
            list
        }
    } else {
        let all_slots: Vec<Vec<String>> = raw.iter().map(|r| r.slots.clone()).collect();
        courses::infer_courses(&all_slots, base)
    };
    let mut guests = Vec::new();
    let mut warnings = Vec::new();
    for r in raw {
        let menu = if course_mode {
            let mut m = Vec::new();
            for (j, dish) in &r.dishes {
                let h = recs[0][*j].trim();
                let canon = course_list.iter().find(|c| courses::same_name(c, h)).cloned().unwrap_or_else(|| h.to_string());
                m.push(format!("{canon}: {dish}"));
            }
            m
        } else {
            let a = courses::assign_row(&r.slots, &course_list);
            if !a.overflow.is_empty() {
                warnings.push(format!("{}: more menu entries than courses; \"{}\" kept under Other", r.name, a.overflow.join("\", \"")));
            }
            a.entries
        };
        let dietary = dietary::Dietary::from_entries(&r.dietary).to_entries();
        guests.push(Guest { name: r.name, table: r.table, menu, dietary, notes: r.notes });
    }
    // pre-order notes -> the guest with the same name (case and whitespace insensitive)
    let mut unmatched = Vec::new();
    let mut attached = 0;
    for (name, note) in pre_notes {
        let key = norm_name(&name);
        let hits: Vec<usize> = guests.iter().enumerate().filter(|(_, g)| norm_name(&g.name) == key).map(|(i, _)| i).collect();
        match hits.first() {
            Some(&i) => {
                if hits.len() > 1 {
                    warnings.push(format!("Pre-order note for \"{name}\": {} guests share that name; attached to the first", hits.len()));
                }
                let g = &mut guests[i];
                if g.notes.is_empty() {
                    g.notes = note;
                } else {
                    g.notes = format!("{}\n{}", g.notes, note);
                }
                attached += 1;
            }
            None => {
                warnings.push(format!("Pre-order note for \"{name}\" matches no guest (kept as an unmatched note)"));
                unmatched.push((name, note));
            }
        }
    }
    let collins = course_mode && (numbered_col || !table_notes.is_empty() || notes_block);
    Ok(CsvImport {
        guests,
        courses: course_list,
        inferred: base.is_none(),
        warnings,
        table_notes,
        unmatched_notes: unmatched,
        no_table,
        default_table: default_table.to_string(),
        collins,
        notes_attached: attached,
    })
}

impl Guest {
    /// Typed view of the dietary entries.
    pub fn dietary_view(&self) -> dietary::Dietary {
        dietary::Dietary::from_entries(&self.dietary)
    }
}

/// Write guests to a CSV file with header `name,table,menu,dietary,notes`.
pub fn write_csv(path: &Path, guests: &[Guest]) -> Result<(), Box<dyn std::error::Error>> {
    write_csv_ex(path, guests, &[])
}

/// Like `write_csv`; table notes are written as rows named "For the table" (table in `table`, text in `notes`).
pub fn write_csv_ex(path: &Path, guests: &[Guest], table_notes: &[TableNote]) -> Result<(), Box<dyn std::error::Error>> {
    let mut w = csv::Writer::from_path(path)?;
    w.write_record(["name", "table", "menu", "dietary", "notes"])?;
    for g in guests {
        w.write_record([
            g.name.as_str(),
            g.table.as_str(),
            g.menu.join("; ").as_str(),
            dietary::Dietary::from_entries(&g.dietary).to_entries().join("; ").as_str(),
            g.notes.as_str(),
        ])?;
    }
    for n in table_notes {
        w.write_record(["For the table", n.table.as_str(), "", "", n.text.as_str()])?;
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
            .map(|i| Guest { name: format!("Guest {i}"), table: (9 - i).to_string(), menu: parse_menu("A: x; B: y"), dietary: vec![], ..Default::default() })
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
        // canonical labels in the fixed order (Tree nuts comes before Peanuts)
        assert_eq!(back[1].dietary, vec!["Gluten", "Crustaceans", "Molluscs"]);
        assert_eq!(back[2].menu.len(), 0);
        assert_eq!(back[2].dietary, vec!["Tree nuts", "Peanuts"]); // allergies but no menu
        assert_eq!(back[5].dietary, vec!["no pork"]); // Other-only entry
        assert_eq!(back[7].dietary, vec!["Milk", "no coriander"]);
        // legacy free text is mapped on import; ambiguous entries stay in Other unchanged
        let old = dir.join("pc_legacy.csv");
        std::fs::write(&old, "name,table,dietary\nA,1,\"Gluten free; Nut allergy; Shellfish allergy; milk allergy; Vegan\"\n").unwrap();
        assert_eq!(read_csv(&old).unwrap()[0].dietary, vec!["Gluten", "Milk", "Vegan", "Nut allergy", "Shellfish allergy"]);
        let out2 = dir.join("pc_legacy_out.csv");
        write_csv(&out2, &read_csv(&old).unwrap()).unwrap();
        assert!(std::fs::read_to_string(&out2).unwrap().contains("Gluten; Milk; Vegan; Nut allergy; Shellfish allergy"));
    }

    fn diet_guest(name: &str, menu: &str) -> Guest {
        Guest { name: name.into(), table: "1".into(), menu: parse_menu(menu), dietary: parse_menu("Nut allergy"), ..Default::default() }
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
        // Bob, Charlotte, Rajesh, Frank (Other-only), Maria (Vegan) and Zoe: 6 of the 8 sample guests
        assert_eq!(with, 6);
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
    fn csv_positional_import_and_roundtrip() {
        let dir = std::env::temp_dir();
        let p = dir.join("pc_pos.csv");
        std::fs::write(
            &p,
            "name,table,menu\nA,1,Soup; Beef; Tart\nB,1,; Fish; Pud\nC,2,Soup;;Tart\nD,2,Soup; Chicken\nE,3,\nF,3,Main: Lamb; Soup\nG,3,a; b; c; d; e\n",
        )
        .unwrap();
        // no labels anywhere: course count from the longest row (5 entries) -> the 5-course defaults
        let nolabel = dir.join("pc_nolabel.csv");
        std::fs::write(&nolabel, "name,table,menu\nA,1,Soup; Beef; Tart\nB,1,a; b; c; d; e\n").unwrap();
        let imp = read_csv_ex(&nolabel, None).unwrap();
        assert_eq!(imp.courses, courses::default_names(5));
        assert!(imp.inferred && imp.warnings.is_empty());
        assert_eq!(imp.guests[0].menu, ["Starter: Soup", "Soup: Beef", "Fish: Tart"]); // positional on 5 courses
        // a labelled entry (row F) must not shrink the count implied by positional rows (G has 5)
        assert_eq!(read_csv_ex(&p, None).unwrap().courses, courses::default_names(5));
        // the same file with the user's 3 courses: overflow goes to Other with a warning
        let three = courses::default_names(3);
        let imp = read_csv_ex(&p, Some(&three)).unwrap();
        assert!(!imp.inferred && imp.courses == three);
        let m = |i: usize| imp.guests[i].menu.clone();
        assert_eq!(m(0), ["Starter: Soup", "Main: Beef", "Dessert: Tart"]);
        assert_eq!(m(1), ["Main: Fish", "Dessert: Pud"]); // skipped starter
        assert_eq!(m(2), ["Starter: Soup", "Dessert: Tart"]); // skipped middle course
        assert_eq!(m(3), ["Starter: Soup", "Main: Chicken"]); // trailing omitted
        assert!(m(4).is_empty());
        assert_eq!(m(5), ["Main: Lamb", "Starter: Soup"]); // mixed: labelled claims Main
        assert_eq!(m(6), ["Starter: a", "Main: b", "Dessert: c", "d", "e"]); // overflow kept as Other
        assert_eq!(imp.warnings.len(), 1);
        assert!(imp.warnings[0].contains("G") && imp.warnings[0].contains("d"), "{:?}", imp.warnings);
        // Save CSV writes labelled entries, and re-importing gives identical data
        let out = dir.join("pc_pos_out.csv");
        write_csv(&out, &imp.guests).unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("Starter: Soup; Main: Beef; Dessert: Tart"));
        let again = read_csv_ex(&out, Some(&three)).unwrap();
        for (a, b) in imp.guests.iter().zip(&again.guests) {
            assert_eq!((&a.name, &a.table, &a.menu, &a.dietary), (&b.name, &b.table, &b.menu, &b.dietary));
        }
        // unknown labels are added after the known courses, not lost
        let u = dir.join("pc_unknown.csv");
        std::fs::write(&u, "name,table,menu\nA,1,Cheese: Brie; Soup\n").unwrap();
        let imp = read_csv_ex(&u, Some(&three)).unwrap();
        assert_eq!(imp.courses, ["Starter", "Main", "Dessert", "Cheese"]);
        assert_eq!(imp.guests[0].menu, ["Cheese: Brie", "Starter: Soup"]);
    }

    #[test]
    fn old_labelled_csv_parses_as_before() {
        let imp = read_csv_ex(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/guests.csv")), None).unwrap();
        assert_eq!(imp.courses, ["Starter", "Main", "Dessert"]);
        assert!(imp.warnings.is_empty());
        let g = &imp.guests;
        assert_eq!(g[0].menu, ["Starter: Soup", "Main: Beef", "Dessert: Tart"]);
        assert_eq!(g[3].menu, ["Main: Vegetarian curry"]);
        assert_eq!(g[6].menu, ["Starter: Pate", "Main: Lamb", "Dessert: Sticky toffee pudding"]);
        assert!(g[2].menu.is_empty() && g[5].menu.is_empty());
    }

    #[test]
    fn sample_plan_is_exactly_two_pages() {
        let imp = read_csv_ex(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/guests.csv")), None).unwrap();
        let l = plan::layout_plan_courses(&imp.guests, Some("Smith Wedding"), "d", false, true, &imp.courses);
        assert_eq!(l.pages.len(), 2);
        assert_eq!((l.pages[0].sheet, l.pages[1].sheet), ("Kitchen", "Tables"));
        assert!(l.warnings.is_empty());
        assert!(l.pages[0].marks.contains(&plan::Mark::Totals));
        assert!(l.pages[1].marks.iter().any(|m| matches!(m, plan::Mark::Section { .. })));
    }

    fn collins() -> CsvImport {
        read_csv_ex(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/collins_sample.csv")), None).unwrap()
    }

    #[test]
    fn collins_sheet_course_columns_and_for_the_table() {
        let imp = collins();
        assert!(imp.collins);
        // the BOM and the empty first header (running row numbers) are ignored; the unused Side column is dropped
        assert_eq!(imp.courses, ["Starter", "Main", "Dessert"]);
        // "For the table" is not a guest
        assert_eq!(imp.guests.len(), 6);
        assert!(imp.guests.iter().all(|g| !g.name.eq_ignore_ascii_case("for the table")));
        // its text is kept verbatim, commas and all, on the table
        assert_eq!(imp.table_notes, vec![TableNote { table: "1".into(), text: "Pork, apple & leek stuffing, Roast potatoes, Honey roast parsnips".into() }]);
        // dishes come from the headers, commas inside a dish are kept
        let g = &imp.guests;
        assert_eq!(g[0].menu, ["Starter: Soup of the day", "Main: Roast beef", "Dessert: Sticky toffee pudding"]);
        assert_eq!(g[1].menu, ["Starter: Pate & toast", "Main: Roast chicken, with gravy", "Dessert: Apple crumble"]);
        assert_eq!(g[2].menu, ["Main: Nut roast", "Dessert: Fruit salad"]); // empty starter = no choice
        // "Halal" maps to the Halal preference
        assert_eq!(g[1].dietary, ["Halal"]);
        assert!(g[1].dietary_view().prefs[3]);
        // no table column: everyone on table 1
        assert!(imp.no_table && g.iter().all(|x| x.table == "1"));
    }

    #[test]
    fn collins_pre_order_notes_are_matched_by_name() {
        let imp = collins();
        let g = &imp.guests;
        // multi-line note, exact name
        assert_eq!(g[1].notes, "Swap the dessert for fruit salad.\nPlease avoid apple.");
        // case and whitespace insensitive
        assert_eq!(g[2].notes, "Nut roast without the cranberries");
        assert_eq!(g[0].notes, "Beef medium rare");
        // two guests share a name: the note goes to the first, with a warning
        assert_eq!(g[4].notes, "Gravy on the side");
        assert!(g[5].notes.is_empty());
        assert!(imp.warnings.iter().any(|w| w.contains("eleanor vance") && w.contains("attached to the first")), "{:?}", imp.warnings);
        // a name that matches nobody is kept as an unmatched note, with a warning
        assert_eq!(imp.unmatched_notes, vec![("Nobody Here".to_string(), "This name is not on the guest list".to_string())]);
        assert!(imp.warnings.iter().any(|w| w.contains("Nobody Here") && w.contains("matches no guest")));
        assert_eq!(imp.notes_attached, 4);
        // detection messages
        assert_eq!(
            imp.collins_summary(),
            "Imported Collins food sheet: 6 guests, courses Starter, Main, Dessert, 1 'for the table' order, 5 pre-order notes"
        );
        assert_eq!(imp.table_note().as_deref(), Some("No table numbers in file: 6 guests put on table 1; edit if needed"));
    }

    #[test]
    fn collins_default_table_and_next_free_table() {
        let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/collins_sample.csv"));
        let imp = read_csv_opts(path, None, "4").unwrap();
        assert!(imp.guests.iter().all(|g| g.table == "4"));
        assert_eq!(imp.table_notes[0].table, "4");
        assert!(imp.table_note().unwrap().contains("put on table 4"));
        // the library function the GUI uses in append mode
        assert_eq!(next_free_table(&[]), "1");
        assert_eq!(next_free_table(&["1".into(), "2".into(), "3".into()]), "4");
        assert_eq!(next_free_table(&["A".into(), " 7 ".into()]), "8");
        assert_eq!(next_free_table(&["Head".into()]), "1");
    }

    #[test]
    fn header_aliases_bom_and_row_number_column() {
        let dir = std::env::temp_dir();
        let p = dir.join("pc_alias2.csv");
        // BOM + empty numbered first column + Guest / Table No. / Dietary requirements aliases + single menu column
        std::fs::write(&p, "\u{feff},Guest,Table No.,menu,Dietary requirements,Notes\n1,Ann Lee,5,Soup; Beef,Halal; Milk,\"Well done\"\n").unwrap();
        let imp = read_csv_ex(&p, None).unwrap();
        assert!(!imp.collins && !imp.no_table);
        assert_eq!((imp.guests[0].name.as_str(), imp.guests[0].table.as_str()), ("Ann Lee", "5"));
        assert_eq!(imp.guests[0].menu, ["Main: Soup", "Dessert: Beef"]); // menu column: positional inference as before (2 entries -> 2 courses)
        assert_eq!(imp.guests[0].dietary, ["Milk", "Halal"]);
        assert_eq!(imp.guests[0].notes, "Well done");
    }

    #[test]
    fn collins_round_trip_keeps_notes_and_table_notes() {
        let imp = collins();
        let out = std::env::temp_dir().join("pc_collins_rt.csv");
        write_csv_ex(&out, &imp.guests, &imp.table_notes).unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.starts_with("name,table,menu,dietary,notes"));
        assert!(text.contains("For the table"), "the shared order is written as a row named For the table");
        let back = read_csv_opts(&out, None, "9").unwrap();
        assert!(!back.collins && !back.no_table);
        assert_eq!(back.guests.len(), imp.guests.len());
        for (a, b) in imp.guests.iter().zip(&back.guests) {
            assert_eq!((&a.name, &a.table, &a.menu, &a.dietary, &a.notes), (&b.name, &b.table, &b.menu, &b.dietary, &b.notes));
        }
        assert_eq!(back.table_notes, imp.table_notes);
        // (unmatched notes are not part of the guest list and are not written)
        assert!(back.unmatched_notes.is_empty());
    }

    #[test]
    fn a_pre_order_note_alone_marks_the_card() {
        let mut m = fonts::Measurer::new();
        let f = fonts::Fonts::default();
        let (cx, cy) = pdf::cell_center(0);
        let plain = Guest { name: "A".into(), table: "1".into(), ..Default::default() };
        let noted = Guest { notes: "no onion".into(), ..plain.clone() };
        assert!(pdf::layout_cell_ex(&plain, None, &f, &mut m, cx, cy, true).markers.is_empty());
        assert_eq!(pdf::layout_cell_ex(&noted, None, &f, &mut m, cx, cy, true).markers.len(), 2);
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
