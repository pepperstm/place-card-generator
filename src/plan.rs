//! Seating plan & menu PDF for back of house / food running.
//!
//! `analyze` parses menu choices into courses and dishes and counts them;
//! `layout_plan` is a pure function that paginates everything into positioned
//! items (all mm, y measured from the top of the page); `render_plan` draws
//! those items with printpdf. Text is always Helvetica / Helvetica-Bold.

use crate::fonts::Builtin;
use crate::{metrics, table_sort_key, Guest};
use printpdf::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

// ------------------------------------------------------------- analysis

pub struct PGuest {
    pub name: String,
    pub table: String,
    /// One entry per column (courses, then "Other" if used): the dishes chosen.
    pub cells: Vec<Vec<String>>,
    pub has_menu: bool,
    /// Dietary requirements / allergies (canonical, first-seen spelling).
    pub dietary: Vec<String>,
}

pub struct PTable {
    pub name: String,
    pub guests: Vec<usize>,
}

pub struct Analysis {
    /// Course names in order of first appearance, then "Other" if any choice lacks a colon.
    pub columns: Vec<String>,
    pub guests: Vec<PGuest>,
    /// Tables in table order; guests in list order.
    pub tables: Vec<PTable>,
    /// Per column, the dishes in order of first appearance (first-seen spelling).
    pub dishes: Vec<Vec<String>>,
    pub no_menu: usize,
    /// Distinct dietary entries in order of first appearance.
    pub diet: Vec<String>,
}

const OTHER: &str = "Other";
/// Shown for guests who have requirements but no menu choices.
pub const NO_MENU: &str = "No menu choices recorded";

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Which {
    All,
    Standard,
    Dietary,
}

/// One group of identical modified plates.
pub struct DietPortion {
    /// Course column, or None for guests with no menu choices.
    pub col: Option<usize>,
    pub dish: String,
    /// Full combination, e.g. "Gluten free + Shellfish allergy".
    pub combo: String,
    key: String,
    /// (guest name, table)
    pub guests: Vec<(String, String)>,
}

/// Split a choice on its first ':' into (course, dish). `None` course = Other.
pub fn split_choice(choice: &str) -> Option<(Option<String>, String)> {
    let c = choice.trim();
    if c.is_empty() {
        return None;
    }
    match c.split_once(':') {
        Some((course, dish)) if !course.trim().is_empty() => {
            let d = dish.trim();
            (!d.is_empty()).then(|| (Some(course.trim().to_string()), d.to_string()))
        }
        Some((_, dish)) => {
            let d = dish.trim();
            (!d.is_empty()).then(|| (None, d.to_string()))
        }
        None => Some((None, c.to_string())),
    }
}

pub fn analyze(guests: &[Guest]) -> Analysis {
    // Pass 1: columns and canonical spellings.
    let mut courses: Vec<String> = Vec::new();
    let mut has_other = false;
    let parsed: Vec<Vec<(Option<String>, String)>> =
        guests.iter().map(|g| g.menu.iter().filter_map(|c| split_choice(c)).collect()).collect();
    for p in &parsed {
        for (c, _) in p {
            match c {
                Some(c) if !courses.iter().any(|x| x.eq_ignore_ascii_case(c)) => courses.push(c.clone()),
                None => has_other = true,
                _ => {}
            }
        }
    }
    let mut columns = courses.clone();
    if has_other {
        columns.push(OTHER.into());
    }
    let col_of = |c: &Option<String>| -> usize {
        match c {
            Some(c) => courses.iter().position(|x| x.eq_ignore_ascii_case(c)).unwrap(),
            None => columns.len() - 1,
        }
    };
    let mut dishes: Vec<Vec<String>> = vec![Vec::new(); columns.len()];
    let mut pg = Vec::new();
    let mut no_menu = 0;
    let mut diet: Vec<String> = Vec::new();
    for (g, p) in guests.iter().zip(&parsed) {
        let mut cells = vec![Vec::new(); columns.len()];
        for (c, d) in p {
            let col = col_of(c);
            let canon = match dishes[col].iter().find(|x| x.eq_ignore_ascii_case(d)) {
                Some(x) => x.clone(),
                None => {
                    dishes[col].push(d.clone());
                    d.clone()
                }
            };
            cells[col].push(canon);
        }
        let has_menu = !p.is_empty();
        if !has_menu {
            no_menu += 1;
        }
        let mut dietary: Vec<String> = Vec::new();
        for d in g.dietary.iter().map(|d| d.trim()).filter(|d| !d.is_empty()) {
            let canon = match diet.iter().find(|x| x.eq_ignore_ascii_case(d)) {
                Some(x) => x.clone(),
                None => {
                    diet.push(d.to_string());
                    d.to_string()
                }
            };
            if !dietary.contains(&canon) {
                dietary.push(canon);
            }
        }
        pg.push(PGuest { name: g.name.clone(), table: g.table.clone(), cells, has_menu, dietary });
    }
    // Group by table, ordered by the shared table sort key.
    let mut order: Vec<String> = Vec::new();
    let mut by: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, g) in pg.iter().enumerate() {
        by.entry(g.table.clone()).or_insert_with(|| {
            order.push(g.table.clone());
            Vec::new()
        })
        .push(i);
    }
    order.sort_by_cached_key(|t| table_sort_key(&Guest { table: t.clone(), ..Default::default() }));
    let tables = order.into_iter().map(|t| PTable { guests: by.remove(&t).unwrap(), name: t }).collect();
    Analysis { columns, guests: pg, tables, dishes, no_menu, diet }
}

impl Analysis {
    /// Per column, (dish, count) in first-seen order, counting only `subset` guests (all if None).
    pub fn counts(&self, subset: Option<&[usize]>) -> Vec<Vec<(String, usize)>> {
        let mut out: Vec<Vec<(String, usize)>> =
            self.dishes.iter().map(|d| d.iter().map(|x| (x.clone(), 0)).collect()).collect();
        let all: Vec<usize> = (0..self.guests.len()).collect();
        for &gi in subset.unwrap_or(&all) {
            for (col, cell) in self.guests[gi].cells.iter().enumerate() {
                for d in cell {
                    if let Some(e) = out[col].iter_mut().find(|(n, _)| n == d) {
                        e.1 += 1;
                    }
                }
            }
        }
        out
    }

    /// Per-dish counts restricted to guests without (`Which::Standard`) or with
    /// (`Which::Dietary`) dietary entries.
    pub fn counts_which(&self, subset: Option<&[usize]>, which: Which) -> Vec<Vec<(String, usize)>> {
        let all: Vec<usize> = (0..self.guests.len()).collect();
        let keep: Vec<usize> = subset
            .unwrap_or(&all)
            .iter()
            .copied()
            .filter(|&gi| match which {
                Which::All => true,
                Which::Standard => self.guests[gi].dietary.is_empty(),
                Which::Dietary => !self.guests[gi].dietary.is_empty(),
            })
            .collect();
        self.counts(Some(&keep))
    }

    /// Plates that need modifying, grouped by course, dish and the guest's FULL
    /// requirement combination (order- and case-insensitive). Guests with
    /// requirements but no menu choices come last under `col == None`.
    pub fn diet_portions(&self) -> Vec<DietPortion> {
        let key = |g: &PGuest| {
            let mut k: Vec<String> = g.dietary.iter().map(|d| d.to_lowercase()).collect();
            k.sort();
            k.join("\u{1}")
        };
        let mut out: Vec<DietPortion> = Vec::new();
        let add = |col: Option<usize>, dish: &str, g: &PGuest, out: &mut Vec<DietPortion>| {
            let k = key(g);
            let who = (g.name.clone(), g.table.clone());
            if let Some(p) = out.iter_mut().find(|p| p.col == col && p.dish == dish && p.key == k) {
                p.guests.push(who);
            } else {
                out.push(DietPortion { col, dish: dish.to_string(), combo: g.dietary.join(" + "), key: k, guests: vec![who] });
            }
        };
        for (col, dishes) in self.dishes.iter().enumerate() {
            for dish in dishes {
                for g in self.guests.iter().filter(|g| !g.dietary.is_empty() && g.cells[col].contains(dish)) {
                    add(Some(col), dish, g, &mut out);
                }
            }
        }
        for g in self.guests.iter().filter(|g| !g.dietary.is_empty() && !g.has_menu) {
            add(None, NO_MENU, g, &mut out);
        }
        out
    }

    /// (dietary entry, count) in first-seen order for `subset` guests (all if None).
    pub fn diet_counts(&self, subset: Option<&[usize]>) -> Vec<(String, usize)> {
        let all: Vec<usize> = (0..self.guests.len()).collect();
        let mut out: Vec<(String, usize)> = self.diet.iter().map(|d| (d.clone(), 0)).collect();
        for &gi in subset.unwrap_or(&all) {
            for d in &self.guests[gi].dietary {
                if let Some(e) = out.iter_mut().find(|(n, _)| n == d) {
                    e.1 += 1;
                }
            }
        }
        out.retain(|(_, n)| *n > 0);
        out
    }

    /// The course part (standard portions only: "Main: Beef x2, Salmon x1 . Dessert: Tart x3")
    /// and the dietary part ("Dietary: Zoe - Beef, Tart - Nut allergy; ..."), if any.
    pub fn summary_parts(&self, subset: &[usize]) -> (String, Option<String>) {
        let counts = self.counts_which(Some(subset), Which::Standard);
        let parts: Vec<String> = counts
            .iter()
            .enumerate()
            .filter_map(|(c, v)| {
                let items: Vec<String> = v.iter().filter(|(_, n)| *n > 0).map(|(d, n)| format!("{d} \u{d7}{n}")).collect();
                (!items.is_empty()).then(|| format!("{}: {}", self.columns[c], items.join(", ")))
            })
            .collect();
        let any_menu = subset.iter().any(|&gi| self.guests[gi].has_menu);
        let any_std = subset.iter().any(|&gi| self.guests[gi].dietary.is_empty());
        let courses = if !parts.is_empty() {
            parts.join(" \u{b7} ")
        } else if any_menu && !any_std {
            "No standard portions".to_string()
        } else {
            "No menu choices".to_string()
        };
        let who: Vec<String> = subset
            .iter()
            .map(|&gi| &self.guests[gi])
            .filter(|g| !g.dietary.is_empty())
            .map(|g| {
                let dishes: Vec<&str> = g.cells.iter().flatten().map(|d| d.as_str()).collect();
                let dishes = if dishes.is_empty() { "no menu choices recorded".to_string() } else { dishes.join(", ") };
                format!("{} \u{2014} {} \u{2014} {}", g.name, dishes, g.dietary.join(" + "))
            })
            .collect();
        (courses, (!who.is_empty()).then(|| format!("Dietary: {}", who.join("; "))))
    }

    /// Full runner summary: courses, then " · Dietary: ..." when anyone has requirements.
    pub fn summary_line(&self, subset: &[usize]) -> String {
        match self.summary_parts(subset) {
            (c, Some(d)) => format!("{c} \u{b7} {d}"),
            (c, None) => c,
        }
    }
}

// --------------------------------------------------------------- layout

#[derive(Clone, Debug, PartialEq)]
pub enum PlanItem {
    Text { text: String, x: f32, y: f32, size: f32, bold: bool, grey: f32 },
    Rect { x: f32, y: f32, w: f32, h: f32, grey: f32 },
    Rule { x1: f32, x2: f32, y: f32, grey: f32 },
    /// A solid black dot (same shape as the place-card allergy marker).
    Dot { x: f32, y: f32, r: f32 },
}

/// Structural markers, so tests can check pagination without reading text positions.
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Totals,
    Dietary,
    DietaryPortions,
    Matrix,
    Section { table: String, cont: bool },
    ColHeader,
    GuestRows { table: String, n: usize },
    Summary { table: String },
}

#[derive(Default, Debug)]
pub struct PlanPage {
    pub items: Vec<PlanItem>,
    pub marks: Vec<Mark>,
}

#[derive(Debug)]
pub struct PlanLayout {
    pub landscape: bool,
    pub w: f32,
    pub h: f32,
    pub pages: Vec<PlanPage>,
    pub matrix_included: bool,
}

const MARGIN: f32 = 12.0;
const TOP: f32 = 30.0; // content starts below the page header
const BOTTOM_PAD: f32 = 18.0; // content ends above the footer
const PT: f32 = 0.352_778;
const BODY: f32 = 11.0;
const HEAD: f32 = 14.0;
const SECTION: f32 = 18.0;
const PAD: f32 = 1.8;
const MAX_LINES: usize = 3;
/// Most tables for which the dish x table matrix is drawn.
pub const MATRIX_MAX_TABLES: usize = 12;

/// Width in mm of `text` in Helvetica(-Bold), covering the few non-ASCII symbols we emit.
fn tw(text: &str, size: f32, bold: bool) -> f32 {
    let font = if bold { Builtin::HelveticaBold } else { Builtin::Helvetica };
    text.chars()
        .map(|c| match c {
            '\u{2014}' | '\u{2026}' => 1000.0 * size * PT / 1000.0,
            _ => metrics::width_mm(&c.to_string(), size, font),
        })
        .sum()
}

fn ellipsize(text: &str, size: f32, bold: bool, max_w: f32) -> String {
    if tw(text, size, bold) <= max_w {
        return text.to_string();
    }
    let mut s: String = text.to_string();
    while !s.is_empty() && tw(&format!("{s}\u{2026}"), size, bold) > max_w {
        s.pop();
    }
    format!("{}\u{2026}", s.trim_end())
}

/// Wrap on spaces (breaking over-long words), at most `max_lines` lines, ellipsised.
pub fn wrap(text: &str, size: f32, bold: bool, max_w: f32, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut overflow = false;
    'outer: for word in text.split_whitespace() {
        let mut word = word.to_string();
        loop {
            let cand = if cur.is_empty() { word.clone() } else { format!("{cur} {word}") };
            if tw(&cand, size, bold) <= max_w {
                cur = cand;
                break;
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                if lines.len() == max_lines {
                    overflow = true;
                    break 'outer;
                }
                continue;
            }
            // A single word wider than the cell: split it by characters.
            let mut head = String::new();
            let mut rest = word.chars().peekable();
            while let Some(&c) = rest.peek() {
                if tw(&format!("{head}{c}"), size, bold) > max_w && !head.is_empty() {
                    break;
                }
                head.push(c);
                rest.next();
            }
            lines.push(head);
            word = rest.collect();
            if lines.len() == max_lines && !word.is_empty() {
                overflow = true;
                break 'outer;
            }
            if word.is_empty() {
                break;
            }
        }
    }
    if !cur.is_empty() && lines.len() < max_lines {
        lines.push(cur);
    } else if !cur.is_empty() {
        overflow = true;
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    if overflow {
        let last = lines.pop().unwrap_or_default();
        lines.push(ellipsize(&format!("{last} \u{2026}"), size, bold, max_w).replace(" \u{2026}", "\u{2026}"));
    }
    lines
}

struct Flow {
    h: f32,
    pages: Vec<PlanPage>,
    y: f32,
    /// Reserve room at the bottom of page 1 for the allergy-marker key.
    key: bool,
}

impl Flow {
    fn new(h: f32, key: bool) -> Self {
        Flow { h, pages: vec![PlanPage::default()], y: TOP, key }
    }
    fn page(&mut self) -> &mut PlanPage {
        self.pages.last_mut().unwrap()
    }
    fn new_page(&mut self) {
        self.pages.push(PlanPage::default());
        self.y = TOP;
    }
    fn limit(&self) -> f32 {
        self.h - BOTTOM_PAD - if self.key && self.pages.len() == 1 { 9.0 } else { 0.0 }
    }
    fn room(&self) -> f32 {
        self.limit() - self.y
    }
    fn text(&mut self, text: &str, x: f32, y: f32, size: f32, bold: bool, grey: f32) {
        self.page().items.push(PlanItem::Text { text: text.into(), x, y, size, bold, grey });
    }
    fn mark(&mut self, m: Mark) {
        self.page().marks.push(m);
    }
}

fn line_h(size: f32) -> f32 {
    size * PT * 1.25
}

struct Col {
    x: f32,
    w: f32,
}

/// A prepared row: wrapped cell lines, height and style.
struct Row {
    cells: Vec<Vec<String>>,
    h: f32,
    size: f32,
    bolds: Vec<bool>,
    /// Draw a black bar in the left margin (dietary rows: visible in black and white).
    bar: bool,
    fill: Option<f32>,
    /// Per-column right alignment (numbers).
    right: Vec<bool>,
}

fn make_row(cols: &[Col], texts: &[String], size: f32, bold: bool, fill: Option<f32>, right: Vec<bool>, max_lines: usize) -> Row {
    make_row_ex(cols, texts, size, vec![bold; cols.len()], false, fill, right, max_lines)
}

#[allow(clippy::too_many_arguments)]
fn make_row_ex(cols: &[Col], texts: &[String], size: f32, bolds: Vec<bool>, bar: bool, fill: Option<f32>, right: Vec<bool>, max_lines: usize) -> Row {
    let cells: Vec<Vec<String>> = cols
        .iter()
        .zip(texts)
        .enumerate()
        .map(|(i, (c, t))| wrap(t, size, bolds[i], c.w - 2.0 * PAD, max_lines))
        .collect();
    let lines = cells.iter().map(|c| c.len()).max().unwrap_or(1);
    Row { cells, h: lines as f32 * line_h(size) + 2.0 * PAD, size, bolds, bar, fill, right }
}

fn draw_row(f: &mut Flow, cols: &[Col], r: &Row) {
    let y0 = f.y;
    if let Some(g) = r.fill {
        let x = cols[0].x;
        let w = cols.last().map(|c| c.x + c.w - x).unwrap_or(0.0);
        f.page().items.push(PlanItem::Rect { x, y: y0, w, h: r.h, grey: g });
    }
    if r.bar {
        f.page().items.push(PlanItem::Rect { x: cols[0].x - 2.6, y: y0, w: 1.8, h: r.h, grey: 0.0 });
    }
    for (ci, (c, lines)) in cols.iter().zip(&r.cells).enumerate() {
        let bold = r.bolds[ci];
        for (li, l) in lines.iter().enumerate() {
            let base = y0 + PAD + li as f32 * line_h(r.size) + r.size * PT * 0.95;
            let x = if r.right.get(ci).copied().unwrap_or(false) {
                c.x + c.w - PAD - tw(l, r.size, bold)
            } else {
                c.x + PAD
            };
            f.text(l, x, base, r.size, bold, 0.0);
        }
    }
    let x1 = cols[0].x;
    let x2 = cols.last().map(|c| c.x + c.w).unwrap_or(x1);
    f.page().items.push(PlanItem::Rule { x1, x2, y: y0 + r.h, grey: if r.bar { 0.35 } else { 0.75 } });
    f.y += r.h;
}

/// Draw rows with the column header at the top and repeated on every new page.
fn flow_rows(f: &mut Flow, cols: &[Col], hdr: &Row, rows: &[Row]) {
    for (i, r) in rows.iter().enumerate() {
        if i == 0 && f.room() < hdr.h + r.h {
            f.new_page();
        }
        if i == 0 || f.room() < r.h {
            if i != 0 {
                f.new_page();
            }
            f.mark(Mark::ColHeader);
            draw_row(f, cols, hdr);
        }
        draw_row(f, cols, r);
    }
}

fn heading(f: &mut Flow, text: &str, size: f32) {
    f.y += 1.0;
    let base = f.y + size * PT;
    f.text(text, MARGIN, base, size, true, 0.0);
    f.y = base + 2.5;
}

/// Lay out the whole plan. `date` is printed in each page header.
pub fn layout_plan(guests: &[Guest], title: Option<&str>, date: &str, force_landscape: bool) -> PlanLayout {
    layout_plan_ex(guests, title, date, force_landscape, false)
}

/// Like `layout_plan`; with `marker_key` a one-line key for the place-card
/// allergy marker is added at the bottom of page 1 (only if anyone has dietary entries).
pub fn layout_plan_ex(guests: &[Guest], title: Option<&str>, date: &str, force_landscape: bool, marker_key: bool) -> PlanLayout {
    let a = analyze(guests);
    let marker_key = marker_key && a.guests.iter().any(|g| !g.dietary.is_empty());
    let n_tables = a.tables.len();
    let landscape = force_landscape || a.columns.len() > 3 || n_tables > 8;
    let (w, h) = if landscape { (297.0, 210.0) } else { (210.0, 297.0) };
    let cw = w - 2.0 * MARGIN;
    let mut f = Flow::new(h, marker_key);

    // ---- Dietary requirements & allergies (first, so the kitchen cannot miss it)
    f.mark(Mark::Dietary);
    heading(&mut f, "Dietary requirements & allergies", SECTION);
    let mut flagged: Vec<(&str, &PGuest)> = Vec::new();
    for t in &a.tables {
        for &gi in &t.guests {
            if !a.guests[gi].dietary.is_empty() {
                flagged.push((t.name.as_str(), &a.guests[gi]));
            }
        }
    }
    if flagged.is_empty() {
        let base = f.y + (BODY + 1.0) * PT;
        f.text("No dietary requirements recorded.", MARGIN, base, BODY + 1.0, true, 0.0);
        f.y = base + 5.0;
    } else {
        let totals = a.diet_counts(None).iter().map(|(d, n)| format!("{d} \u{d7}{n}")).collect::<Vec<_>>().join(" \u{b7} ");
        for l in wrap(&format!("Totals: {totals}"), BODY + 1.0, true, cw, 4) {
            let base = f.y + (BODY + 1.0) * PT;
            f.text(&l, MARGIN, base, BODY + 1.0, true, 0.0);
            f.y = base + 2.0;
        }
        f.y += 2.0;
        let dcols = vec![
            Col { x: MARGIN, w: cw * 0.16 },
            Col { x: MARGIN + cw * 0.16, w: cw * 0.38 },
            Col { x: MARGIN + cw * 0.54, w: cw * 0.46 },
        ];
        let dh = make_row(&dcols, &["Table".into(), "Guest".into(), "Requirements".into()], HEAD, true, Some(0.82), vec![false; 3], 1);
        for (i, (tname, g)) in flagged.iter().enumerate() {
            let row = make_row_ex(
                &dcols,
                &[tname.to_string(), g.name.clone(), format!("! {}", g.dietary.join(", "))],
                BODY + 1.0,
                vec![true, false, true],
                true,
                Some(if i % 2 == 0 { 0.86 } else { 0.78 }),
                vec![false; 3],
                MAX_LINES,
            );
            if i == 0 || f.room() < row.h {
                if f.room() < dh.h + row.h {
                    f.new_page();
                }
                f.mark(Mark::ColHeader);
                draw_row(&mut f, &dcols, &dh);
            }
            draw_row(&mut f, &dcols, &row);
        }
        f.y += 4.0;
    }
    if f.room() < 40.0 {
        f.new_page();
    } else {
        f.y += 6.0;
    }

    // ---- Kitchen totals
    f.mark(Mark::Totals);
    heading(&mut f, "Kitchen totals", SECTION);
    let n = a.guests.len();
    let n_diet = a.guests.iter().filter(|g| !g.dietary.is_empty()).count();
    let line = format!(
        "Total guests: {n}  \u{b7}  Tables: {n_tables}  \u{b7}  Guests with no menu choices: {}  \u{b7}  Guests with dietary requirements: {n_diet}",
        a.no_menu
    );
    for l in wrap(&line, BODY + 1.0, true, cw, 2) {
        let base = f.y + (BODY + 1.0) * PT;
        f.text(&l, MARGIN, base, BODY + 1.0, true, 0.0);
        f.y = base + 2.0;
    }
    f.y += 2.0;
    // Group 1: standard portions (guests with no dietary entries), reconciled with the dietary plates.
    let std = a.counts_which(None, Which::Standard);
    let dietc = a.counts_which(None, Which::Dietary);
    let portions = a.diet_portions();
    if f.room() < 40.0 {
        f.new_page();
    }
    heading(&mut f, "Standard portions", HEAD + 2.0);
    let tot_cols = vec![
        Col { x: MARGIN, w: cw * 0.17 },
        Col { x: MARGIN + cw * 0.17, w: cw * 0.30 },
        Col { x: MARGIN + cw * 0.47, w: cw * 0.17 },
        Col { x: MARGIN + cw * 0.64, w: cw * 0.20 },
        Col { x: MARGIN + cw * 0.84, w: cw * 0.16 },
    ];
    let right5 = vec![false, false, true, true, true];
    let hdr = make_row(
        &tot_cols,
        &["Course", "Dish", "Standard", "+ Dietary", "= Total"].map(String::from),
        HEAD,
        true,
        Some(0.82),
        right5.clone(),
        2,
    );
    let mut rows = Vec::new();
    let mut course_idx = 0;
    for (ci, list) in std.iter().enumerate() {
        for (di, (dish, cnt)) in list.iter().enumerate() {
            let d = dietc[ci][di].1;
            rows.push(make_row(
                &tot_cols,
                &[
                    if di == 0 { a.columns[ci].clone() } else { String::new() },
                    dish.clone(),
                    cnt.to_string(),
                    d.to_string(),
                    (cnt + d).to_string(),
                ],
                BODY + 1.0,
                false,
                (course_idx % 2 == 1).then_some(0.95),
                right5.clone(),
                MAX_LINES,
            ));
        }
        if !list.is_empty() {
            course_idx += 1;
        }
    }
    if rows.is_empty() {
        let base = f.y + BODY * PT;
        f.text("No menu choices have been entered.", MARGIN, base, BODY, false, 0.3);
        f.y = base + 4.0;
    } else {
        flow_rows(&mut f, &tot_cols, &hdr, &rows);
    }

    // Group 2: dietary / allergy portions, highlighted like the dietary rows.
    f.y += 5.0;
    if f.room() < 45.0 {
        f.new_page();
    }
    f.mark(Mark::DietaryPortions);
    heading(&mut f, "Dietary / allergy portions", HEAD + 2.0);
    if portions.is_empty() {
        let base = f.y + (BODY + 1.0) * PT;
        f.text("No dietary portions.", MARGIN, base, BODY + 1.0, true, 0.0);
        f.y = base + 5.0;
    } else {
        let pcols = vec![
            Col { x: MARGIN, w: cw * 0.13 },
            Col { x: MARGIN + cw * 0.13, w: cw * 0.21 },
            Col { x: MARGIN + cw * 0.34, w: cw * 0.26 },
            Col { x: MARGIN + cw * 0.60, w: cw * 0.11 },
            Col { x: MARGIN + cw * 0.71, w: cw * 0.29 },
        ];
        let ph = make_row(
            &pcols,
            &["Course", "Dish", "Requirements", "Count", "Guests (Table)"].map(String::from),
            HEAD,
            true,
            Some(0.82),
            vec![false, false, false, true, false],
            2,
        );
        let mut prows = Vec::new();
        let mut last: (Option<usize>, String) = (Some(usize::MAX), String::new());
        for (i, p) in portions.iter().enumerate() {
            let same_course = last.0 == p.col;
            let same_dish = same_course && last.1 == p.dish;
            last = (p.col, p.dish.clone());
            let guests = p.guests.iter().map(|(n, t)| format!("{n} (T{t})")).collect::<Vec<_>>().join(", ");
            prows.push(make_row_ex(
                &pcols,
                &[
                    if same_course { String::new() } else { p.col.map(|c| a.columns[c].clone()).unwrap_or_else(|| "\u{2014}".into()) },
                    if same_dish { String::new() } else { p.dish.clone() },
                    format!("! {}", p.combo),
                    p.guests.len().to_string(),
                    guests,
                ],
                BODY + 1.0,
                vec![true, true, true, true, false],
                true,
                Some(if i % 2 == 0 { 0.86 } else { 0.78 }),
                vec![false, false, false, true, false],
                MAX_LINES,
            ));
        }
        flow_rows(&mut f, &pcols, &ph, &prows);
    }

    // ---- Dish x table matrix
    let all_dishes: Vec<(usize, &String)> = a.dishes.iter().enumerate().flat_map(|(c, v)| v.iter().map(move |d| (c, d))).collect();
    let matrix_included = n_tables <= MATRIX_MAX_TABLES && n_tables > 0 && !all_dishes.is_empty();
    if matrix_included {
        let label_w = if landscape { 80.0 } else { 62.0 };
        let total_w = 18.0;
        let tw_each = ((cw - label_w - total_w) / n_tables as f32).min(24.0);
        let mut cols = vec![Col { x: MARGIN, w: label_w }];
        for i in 0..n_tables {
            cols.push(Col { x: MARGIN + label_w + i as f32 * tw_each, w: tw_each });
        }
        cols.push(Col { x: MARGIN + label_w + n_tables as f32 * tw_each, w: total_w });
        let right = vec![false].into_iter().chain((0..=n_tables).map(|_| true)).collect::<Vec<_>>();
        let mut head_txt = vec!["Dish".to_string()];
        head_txt.extend(a.tables.iter().map(|t| ellipsize(&t.name, HEAD, true, tw_each - 2.0 * PAD)));
        head_txt.push("Total".into());
        let hr = make_row(&cols, &head_txt, HEAD, true, Some(0.82), right.clone(), 1);
        let per_table: Vec<Vec<Vec<(String, usize)>>> = a.tables.iter().map(|t| a.counts(Some(&t.guests))).collect();
        let per_table_diet: Vec<Vec<Vec<(String, usize)>>> =
            a.tables.iter().map(|t| a.counts_which(Some(&t.guests), Which::Dietary)).collect();
        // Heading + header + at least two rows must fit, else start a new page.
        let est_rows = (all_dishes.len() + a.columns.len()) as f32 * (line_h(BODY + 1.0) + 2.0 * PAD);
        let est = 14.0 + hr.h + est_rows;
        // Keep the matrix together on one page when it would fit on a fresh page.
        if f.room() < 14.0 + hr.h + 2.0 * (BODY * PT * 1.25 + 2.0 * PAD) || (est > f.room() && est <= f.limit() - TOP) {
            f.new_page();
        } else {
            f.y += 6.0;
        }
        f.mark(Mark::Matrix);
        heading(&mut f, "Dishes by table (all portions, incl. dietary)", SECTION - 2.0);
        f.mark(Mark::ColHeader);
        draw_row(&mut f, &cols, &hr);
        let mut last_course = usize::MAX;
        let mut shade = false;
        for (ci, dish) in &all_dishes {
            if *ci != last_course {
                last_course = *ci;
                let mut ct = vec![a.columns[*ci].clone()];
                ct.extend(std::iter::repeat(String::new()).take(n_tables + 1));
                let r = make_row(&cols, &ct, BODY + 1.0, true, Some(0.9), right.clone(), 1);
                if f.room() < r.h + 8.0 {
                    f.new_page();
                    f.mark(Mark::ColHeader);
                    draw_row(&mut f, &cols, &hr);
                }
                draw_row(&mut f, &cols, &r);
            }
            let mut txt = vec![(*dish).clone()];
            let mut total = 0;
            for (pt, pd) in per_table.iter().zip(&per_table_diet) {
                let c = pt[*ci].iter().find(|(d, _)| d == *dish).map(|x| x.1).unwrap_or(0);
                let dc = pd[*ci].iter().find(|(d, _)| d == *dish).map(|x| x.1).unwrap_or(0);
                total += c;
                txt.push(if c == 0 { "\u{b7}".into() } else if dc > 0 { format!("{c}*") } else { c.to_string() });
            }
            txt.push(total.to_string());
            shade = !shade;
            let r = make_row(&cols, &txt, BODY + 1.0, false, shade.then_some(0.96), right.clone(), 2);
            if f.room() < r.h {
                f.new_page();
                f.mark(Mark::ColHeader);
                draw_row(&mut f, &cols, &hr);
            }
            draw_row(&mut f, &cols, &r);
        }
        if f.room() < 8.0 {
            f.new_page();
        }
        let base = f.y + 2.0 + 10.0 * PT;
        f.text("* includes at least one dietary / allergy portion", MARGIN, base, 10.0, false, 0.3);
        f.y = base + 2.0;
    } else if n_tables > MATRIX_MAX_TABLES {
        f.y += 4.0;
        let base = f.y + BODY * PT;
        f.text(&format!("(Dish-by-table matrix omitted: more than {MATRIX_MAX_TABLES} tables. See the per-table summaries.)"), MARGIN, base, BODY - 1.0, false, 0.35);
        f.y = base + 2.0;
    }

    // ---- Table sections, starting on a fresh page
    f.new_page();
    // Columns: guest name | one per course (incl. Other) | Dietary / allergies.
    let n_cols = a.columns.len();
    let diet_w = if landscape { 62.0 } else { 42.0 };
    let name_w = if n_cols == 0 { cw - diet_w } else if landscape { 55.0 } else { 38.0 };
    let course_w = if n_cols == 0 { 0.0 } else { (cw - name_w - diet_w) / n_cols as f32 };
    let mut cols = vec![Col { x: MARGIN, w: name_w }];
    for i in 0..n_cols {
        cols.push(Col { x: MARGIN + name_w + i as f32 * course_w, w: course_w });
    }
    cols.push(Col { x: MARGIN + name_w + n_cols as f32 * course_w, w: diet_w });
    let mut head_txt = vec!["Guest name".to_string()];
    head_txt.extend(a.columns.iter().cloned());
    head_txt.push("Dietary / allergies".into());
    let hr = make_row(&cols, &head_txt, HEAD, true, Some(0.82), vec![false; cols.len()], 2);
    for t in &a.tables {
        let rows: Vec<Row> = t
            .guests
            .iter()
            .enumerate()
            .map(|(i, &gi)| {
                let g = &a.guests[gi];
                let mut txt = vec![g.name.clone()];
                for cell in &g.cells {
                    txt.push(if cell.is_empty() { "\u{2014}".into() } else { cell.join(", ") });
                }
                let flagged = !g.dietary.is_empty();
                txt.push(if flagged { format!("! {}", g.dietary.join(", ")) } else { "\u{2014}".into() });
                let mut bolds = vec![false; cols.len()];
                if flagged {
                    // bold name and requirements, heavier band and a black bar: clear in black and white
                    bolds[0] = true;
                    *bolds.last_mut().unwrap() = true;
                }
                let fill = if flagged { Some(0.8) } else { (i % 2 == 1).then_some(0.95) };
                make_row_ex(&cols, &txt, BODY, bolds, flagged, fill, vec![false; cols.len()], MAX_LINES)
            })
            .collect();
        let (c_text, d_text) = a.summary_parts(&t.guests);
        let c_lines = wrap(&c_text, BODY, true, cw - 2.0 * PAD, 6);
        let c_h = c_lines.len() as f32 * line_h(BODY) + 2.0 * PAD + 2.0;
        let d_lines = d_text.map(|d| wrap(&format!("! {d}"), BODY, true, cw - 2.0 * PAD, 6));
        let d_h = d_lines.as_ref().map(|l| l.len() as f32 * line_h(BODY) + 2.0 * PAD + 2.0);
        let sum_h = c_h + d_h.map(|h| h + 1.5).unwrap_or(0.0);
        let head_h = SECTION * PT + 4.5;
        let total_h = head_h + hr.h + rows.iter().map(|r| r.h).sum::<f32>() + sum_h + 6.0;
        let page_room = f.limit() - TOP;
        if f.y > TOP && total_h > f.room() && total_h <= page_room {
            f.new_page();
        }
        let label = format!("Table {} \u{b7} {} guest{}", t.name, t.guests.len(), if t.guests.len() == 1 { "" } else { "s" });
        let mut cont = false;
        // section start (needs heading + header + first row)
        let need_first = head_h + hr.h + rows.first().map(|r| r.h).unwrap_or(0.0);
        if f.room() < need_first {
            f.new_page();
        }
        f.y += 2.0;
        f.mark(Mark::Section { table: t.name.clone(), cont });
        heading(&mut f, &label, SECTION);
        f.mark(Mark::ColHeader);
        draw_row(&mut f, &cols, &hr);
        let mut n_on_page = 0;
        for r in &rows {
            if f.room() < r.h {
                f.mark(Mark::GuestRows { table: t.name.clone(), n: n_on_page });
                n_on_page = 0;
                f.new_page();
                cont = true;
                f.mark(Mark::Section { table: t.name.clone(), cont });
                heading(&mut f, &format!("Table {} (cont.)", t.name), SECTION);
                f.mark(Mark::ColHeader);
                draw_row(&mut f, &cols, &hr);
            }
            draw_row(&mut f, &cols, r);
            n_on_page += 1;
        }
        f.mark(Mark::GuestRows { table: t.name.clone(), n: n_on_page });
        if f.room() < sum_h {
            f.new_page();
            f.mark(Mark::Section { table: t.name.clone(), cont: true });
            heading(&mut f, &format!("Table {} (cont.)", t.name), SECTION);
        }
        f.mark(Mark::Summary { table: t.name.clone() });
        f.y += 1.0;
        let ry = f.y;
        f.page().items.push(PlanItem::Rect { x: MARGIN, y: ry, w: cw, h: c_h, grey: 0.9 });
        for (i, l) in c_lines.iter().enumerate() {
            let base = ry + PAD + 1.0 + i as f32 * line_h(BODY) + BODY * PT * 0.95;
            f.text(l, MARGIN + PAD, base, BODY, true, 0.0);
        }
        if let (Some(dl), Some(dh)) = (&d_lines, d_h) {
            let dy = ry + c_h + 1.5;
            f.page().items.push(PlanItem::Rect { x: MARGIN, y: dy, w: cw, h: dh, grey: 0.78 });
            f.page().items.push(PlanItem::Rect { x: MARGIN - 2.6, y: dy, w: 1.8, h: dh, grey: 0.0 });
            for (i, l) in dl.iter().enumerate() {
                let base = dy + PAD + 1.0 + i as f32 * line_h(BODY) + BODY * PT * 0.95;
                f.text(l, MARGIN + PAD, base, BODY, true, 0.0);
            }
        }
        f.y += sum_h + 6.0;
    }
    // Drop a trailing empty page (e.g. when there are no guests at all).
    if f.pages.len() > 1 && f.pages.last().map(|p| p.items.is_empty()).unwrap_or(false) {
        f.pages.pop();
    }

    // ---- Page headers and footers
    let total = f.pages.len();
    let head = match title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => format!("{t} \u{2014} Seating plan & menu"),
        None => "Seating plan & menu".to_string(),
    };
    let date_txt = format!("Generated {date}");
    for (i, p) in f.pages.iter_mut().enumerate() {
        let head = ellipsize(&head, 15.0, true, w - 2.0 * MARGIN - tw(&date_txt, 10.0, false) - 6.0);
        p.items.push(PlanItem::Text { text: head, x: MARGIN, y: 17.0, size: 15.0, bold: true, grey: 0.0 });
        p.items.push(PlanItem::Text { text: date_txt.clone(), x: w - MARGIN - tw(&date_txt, 10.0, false), y: 17.0, size: 10.0, bold: false, grey: 0.35 });
        p.items.push(PlanItem::Rule { x1: MARGIN, x2: w - MARGIN, y: 21.0, grey: 0.4 });
        let foot = format!("Page {} of {}", i + 1, total);
        p.items.push(PlanItem::Text { text: foot.clone(), x: w / 2.0 - tw(&foot, 10.0, false) / 2.0, y: h - 9.0, size: 10.0, bold: false, grey: 0.35 });
    }
    if marker_key {
        let p = &mut f.pages[0];
        p.items.push(PlanItem::Dot { x: MARGIN + 1.8, y: h - 17.6, r: crate::pdf::MARKER_R });
        p.items.push(PlanItem::Text {
            text: "on a place card = guest has a dietary requirement / allergy (see list above)".into(),
            x: MARGIN + 5.5,
            y: h - 16.2,
            size: 10.0,
            bold: false,
            grey: 0.2,
        });
    }
    PlanLayout { landscape, w, h, pages: f.pages, matrix_included }
}

// ------------------------------------------------------------- drawing

/// Today's date (UTC) as YYYY-MM-DD, without external crates.
pub fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let z = secs / 86_400 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

pub struct PlanInfo {
    pub pages: usize,
    pub landscape: bool,
}

/// Write the seating plan PDF.
pub fn render_plan(
    guests: &[Guest],
    title: Option<&str>,
    force_landscape: bool,
    out: &Path,
) -> Result<PlanInfo, Box<dyn std::error::Error>> {
    render_plan_ex(guests, title, force_landscape, true, out)
}

/// Write the seating plan PDF; `marker_key` adds the allergy-marker key on page 1.
pub fn render_plan_ex(
    guests: &[Guest],
    title: Option<&str>,
    force_landscape: bool,
    marker_key: bool,
    out: &Path,
) -> Result<PlanInfo, Box<dyn std::error::Error>> {
    let lay = layout_plan_ex(guests, title, &today(), force_landscape, marker_key);
    let (doc, p0, l0) = PdfDocument::new("Seating plan", Mm(lay.w), Mm(lay.h), "plan");
    let reg = doc.add_builtin_font(BuiltinFont::Helvetica)?;
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold)?;
    for (pi, page) in lay.pages.iter().enumerate() {
        let layer = if pi == 0 {
            doc.get_page(p0).get_layer(l0)
        } else {
            let (p, l) = doc.add_page(Mm(lay.w), Mm(lay.h), "plan");
            doc.get_page(p).get_layer(l)
        };
        let fy = |y: f32| Mm(lay.h - y);
        for it in &page.items {
            match it {
                PlanItem::Rect { x, y, w, h, grey } => {
                    layer.set_fill_color(Color::Greyscale(Greyscale::new(*grey, None)));
                    layer.add_polygon(Polygon {
                        rings: vec![vec![
                            (Point::new(Mm(*x), fy(*y)), false),
                            (Point::new(Mm(*x + *w), fy(*y)), false),
                            (Point::new(Mm(*x + *w), fy(*y + *h)), false),
                            (Point::new(Mm(*x), fy(*y + *h)), false),
                        ]],
                        mode: path::PaintMode::Fill,
                        winding_order: path::WindingOrder::NonZero,
                    });
                }
                PlanItem::Dot { x, y, r } => {
                    layer.set_fill_color(Color::Greyscale(Greyscale::new(0.0, None)));
                    layer.add_polygon(Polygon {
                        rings: vec![crate::pdf::circle_points(*x, lay.h - *y, *r)
                            .into_iter()
                            .map(|(px, py)| (Point::new(Mm(px), Mm(py)), false))
                            .collect()],
                        mode: path::PaintMode::Fill,
                        winding_order: path::WindingOrder::NonZero,
                    });
                }
                PlanItem::Rule { x1, x2, y, grey } => {
                    layer.set_outline_color(Color::Greyscale(Greyscale::new(*grey, None)));
                    layer.set_outline_thickness(0.4);
                    layer.add_line(Line {
                        points: vec![(Point::new(Mm(*x1), fy(*y)), false), (Point::new(Mm(*x2), fy(*y)), false)],
                        is_closed: false,
                    });
                }
                PlanItem::Text { text, x, y, size, bold: b, grey } => {
                    layer.set_fill_color(Color::Greyscale(Greyscale::new(*grey, None)));
                    layer.use_text(text.clone(), *size, Mm(*x), fy(*y), if *b { &bold } else { &reg });
                }
            }
        }
    }
    doc.save(&mut BufWriter::new(File::create(out)?))?;
    Ok(PlanInfo { pages: lay.pages.len(), landscape: lay.landscape })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_menu;

    fn g(name: &str, table: &str, menu: &str) -> Guest {
        Guest { name: name.into(), table: table.into(), menu: parse_menu(menu), dietary: vec![] }
    }

    #[test]
    fn course_parsing() {
        assert_eq!(split_choice("Starter: Soup"), Some((Some("Starter".into()), "Soup".into())));
        // only the FIRST colon splits
        assert_eq!(split_choice("Main: Beef: rare"), Some((Some("Main".into()), "Beef: rare".into())));
        assert_eq!(split_choice("Cake"), Some((None, "Cake".into())));
        assert_eq!(split_choice("  "), None);
        let a = analyze(&[
            g("A", "1", "Starter: Soup; Main: Beef"),
            g("B", "1", "starter:  soup ; MAIN: Fish; Cake"),
            g("C", "2", ""),
        ]);
        assert_eq!(a.columns, vec!["Starter", "Main", "Other"]); // first-seen spelling, Other last
        assert_eq!(a.dishes[0], vec!["Soup"]); // merged case-insensitively
        assert_eq!(a.guests[1].cells[0], vec!["Soup"]);
        assert_eq!(a.guests[1].cells[2], vec!["Cake"]);
        assert!(!a.guests[2].has_menu);
        assert_eq!(a.no_menu, 1);
        // no Other column unless used
        assert_eq!(analyze(&[g("A", "1", "Main: x")]).columns, vec!["Main"]);
    }

    #[test]
    fn counts_and_summary() {
        let a = analyze(&[
            g("A", "10", "Main: Beef; Dessert: Tart"),
            g("B", "2", "Main: Beef"),
            g("C", "2", "Main: Salmon; Dessert: Tart"),
            g("D", "Head", ""),
        ]);
        // tables: numeric first (2, 10), then alphabetical
        let names: Vec<_> = a.tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["2", "10", "Head"]);
        let tot = a.counts(None);
        assert_eq!(tot[0], vec![("Beef".to_string(), 2), ("Salmon".to_string(), 1)]);
        assert_eq!(tot[1], vec![("Tart".to_string(), 2)]);
        assert_eq!(a.summary_line(&a.tables[0].guests), "Main: Beef \u{d7}1, Salmon \u{d7}1 \u{b7} Dessert: Tart \u{d7}1");
        assert_eq!(a.summary_line(&a.tables[2].guests), "No menu choices");
    }

    fn big(n: usize, tables: usize) -> Vec<Guest> {
        let mains = ["Beef", "Salmon", "Vegetarian risotto with seasonal greens"];
        (0..n)
            .map(|i| g(&format!("Guest {i} Fairly Long Surname"), &(i % tables + 1).to_string(),
                &format!("Starter: Soup; Main: {}; Dessert: Tart", mains[i % 3])))
            .collect()
    }

    fn check_bounds(l: &PlanLayout) {
        for p in &l.pages {
            for it in &p.items {
                if let PlanItem::Text { text, x, y, size, bold, .. } = it {
                    assert!(*x >= MARGIN - 0.01, "{text}");
                    assert!(x + tw(text, *size, *bold) <= l.w - MARGIN + 0.5, "'{text}' overflows");
                    assert!(*y <= l.h, "{text}");
                }
            }
        }
    }

    #[test]
    fn pagination_no_split_and_headers_repeat() {
        let guests = big(120, 15);
        let l = layout_plan(&guests, Some("Big Do"), "2026-01-01", false);
        check_bounds(&l);
        assert!(l.pages.len() > 3);
        // every table is a single section on one page, with all 8 guests
        for t in 1..=15 {
            let name = t.to_string();
            let sections: Vec<_> = l.pages.iter().enumerate().flat_map(|(pi, p)| p.marks.iter().map(move |m| (pi, m)))
                .filter(|(_, m)| matches!(m, Mark::Section { table, .. } if *table == name)).collect();
            assert_eq!(sections.len(), 1, "table {t} split");
            let rows: usize = l.pages.iter().flat_map(|p| &p.marks).filter_map(|m| match m {
                Mark::GuestRows { table, n } if *table == name => Some(*n), _ => None }).sum();
            assert_eq!(rows, 8);
        }
        // every section heading is directly followed by a column header
        for p in &l.pages {
            for (i, m) in p.marks.iter().enumerate() {
                if matches!(m, Mark::Section { .. }) && !matches!(p.marks.get(i + 1), Some(Mark::Summary { .. })) {
                    assert_eq!(p.marks.get(i + 1), Some(&Mark::ColHeader));
                }
            }
        }
        // more than 12 tables: no matrix
        assert!(!l.matrix_included);
        // headers/footers on every page
        for (i, p) in l.pages.iter().enumerate() {
            let foot = format!("Page {} of {}", i + 1, l.pages.len());
            assert!(p.items.iter().any(|it| matches!(it, PlanItem::Text { text, .. } if *text == foot)));
        }
    }

    #[test]
    fn long_table_continues_with_repeated_header() {
        let guests = big(70, 1);
        let l = layout_plan(&guests, None, "2026-01-01", false);
        check_bounds(&l);
        let conts = l.pages.iter().flat_map(|p| &p.marks).filter(|m| matches!(m, Mark::Section { cont: true, .. })).count();
        assert!(conts >= 1);
        let total: usize = l.pages.iter().flat_map(|p| &p.marks).filter_map(|m| if let Mark::GuestRows { n, .. } = m { Some(*n) } else { None }).sum();
        assert_eq!(total, 70);
        // each page that has guest rows starts them under a column header
        for p in &l.pages {
            if p.marks.iter().any(|m| matches!(m, Mark::GuestRows { n, .. } if *n > 0)) {
                assert!(p.marks.contains(&Mark::ColHeader));
            }
        }
    }

    #[test]
    fn orientation_and_matrix() {
        let few = layout_plan(&big(6, 3), None, "d", false);
        assert!(!few.landscape && few.matrix_included);
        let many_tables = layout_plan(&big(40, 10), None, "d", false);
        assert!(many_tables.landscape && many_tables.matrix_included);
        let forced = layout_plan(&big(6, 3), None, "d", true);
        assert!(forced.landscape);
        let cols = layout_plan(&[g("A", "1", "A: x; B: y; C: z; D: w")], None, "d", false);
        assert!(cols.landscape);
        // empty list does not panic
        let _ = layout_plan(&[], None, "d", false);
    }

    fn gd(name: &str, table: &str, menu: &str, diet: &str) -> Guest {
        Guest { dietary: parse_menu(diet), ..g(name, table, menu) }
    }

    fn all_text(l: &PlanLayout) -> Vec<String> {
        l.pages.iter().flat_map(|p| p.items.iter()).filter_map(|i| if let PlanItem::Text { text, .. } = i { Some(text.clone()) } else { None }).collect()
    }

    #[test]
    fn dietary_parsing_and_counts() {
        let a = analyze(&[
            gd("A", "1", "Main: x", "Nut allergy; Vegan"),
            gd("B", "1", "", "NUT ALLERGY"),
            gd("C", "2", "Main: y", " ; "),
            gd("D", "2", "", "Vegan; vegan"),
        ]);
        assert_eq!(a.diet, vec!["Nut allergy", "Vegan"]); // first-seen spelling, case-insensitive merge
        assert_eq!(a.guests[1].dietary, vec!["Nut allergy"]);
        assert!(a.guests[2].dietary.is_empty()); // only separators -> nothing
        assert_eq!(a.guests[3].dietary, vec!["Vegan"]); // duplicate within one guest counted once
        assert_eq!(a.diet_counts(None), vec![("Nut allergy".to_string(), 2), ("Vegan".to_string(), 2)]);
        // Table 1: A (Main x, dietary) and B (no menu, dietary) -> no standard plates.
        // (Changed deliberately: the course line now counts STANDARD portions only,
        // and the dietary band lists each modified plate per guest.)
        let (c, d) = a.summary_parts(&a.tables[0].guests);
        assert_eq!(c, "No standard portions");
        assert_eq!(
            d.as_deref(),
            Some("Dietary: A \u{2014} x \u{2014} Nut allergy + Vegan; B \u{2014} no menu choices recorded \u{2014} Nut allergy")
        );
        // Table 2: C is standard (Main y); D has no menu but a requirement
        let (c, d) = a.summary_parts(&a.tables[1].guests);
        assert_eq!(c, "Main: y \u{d7}1");
        assert_eq!(d.as_deref(), Some("Dietary: D \u{2014} no menu choices recorded \u{2014} Vegan"));
    }

    fn mixed() -> Vec<Guest> {
        vec![
            gd("Alice", "1", "Starter: Soup; Main: Beef", ""),
            gd("Zoe", "4", "Main: Beef; Dessert: Tart", "Nut allergy"),
            gd("Bob", "1", "Starter: Soup; Main: Salmon", "Gluten free; Shellfish allergy"),
            gd("Cy", "2", "Main: Salmon", "shellfish ALLERGY; gluten free"),
            gd("Di", "2", "", "Vegetarian"),
            gd("Ed", "3", "Main: Beef", ""),
        ]
    }

    #[test]
    fn standard_plus_dietary_reconciles_to_totals() {
        let a = analyze(&mixed());
        let all = a.counts(None);
        let std = a.counts_which(None, Which::Standard);
        let dt = a.counts_which(None, Which::Dietary);
        for c in 0..all.len() {
            for d in 0..all[c].len() {
                assert_eq!(std[c][d].1 + dt[c][d].1, all[c][d].1, "{}", all[c][d].0);
            }
        }
        // by hand: Beef 3 = 2 standard (Alice, Ed) + 1 Nut allergy (Zoe)
        let main = a.columns.iter().position(|c| c == "Main").unwrap();
        let beef = a.dishes[main].iter().position(|d| d == "Beef").unwrap();
        assert_eq!((std[main][beef].1, dt[main][beef].1, all[main][beef].1), (2, 1, 3));
        // dishes whose every diner has a requirement keep a row with standard 0
        let salmon = a.dishes[main].iter().position(|d| d == "Salmon").unwrap();
        assert_eq!((std[main][salmon].1, dt[main][salmon].1), (0, 2));
        // the plan's text carries the reconciliation headers
        let l = layout_plan(&mixed(), None, "d", false);
        let t = all_text(&l);
        assert!(t.iter().any(|x| x == "Standard portions") && t.iter().any(|x| x == "Dietary / allergy portions"));
        let joined = t.join(" ");
        assert!(joined.contains("dietary requirements: 4"), "{joined}");
    }

    #[test]
    fn dietary_combinations_merge_regardless_of_order_and_case() {
        let a = analyze(&mixed());
        let p = a.diet_portions();
        let salmon: Vec<_> = p.iter().filter(|x| x.dish == "Salmon").collect();
        assert_eq!(salmon.len(), 1, "same combination must merge into one row");
        assert_eq!(salmon[0].guests.len(), 2);
        assert_eq!(salmon[0].combo, "Gluten free + Shellfish allergy"); // first-seen spelling and order
        // a guest's whole combination is one portion, not split
        assert!(!p.iter().any(|x| x.combo == "Gluten free"));
        // Zoe's Beef and Tart are separate dish rows
        assert_eq!(p.iter().filter(|x| x.combo == "Nut allergy").count(), 2);
    }

    #[test]
    fn dietary_guest_without_menu_is_listed() {
        let a = analyze(&mixed());
        let p = a.diet_portions();
        let last = p.last().unwrap();
        assert_eq!((last.col, last.dish.as_str(), last.combo.as_str()), (None, NO_MENU, "Vegetarian"));
        assert_eq!(last.guests, vec![("Di".to_string(), "2".to_string())]);
        let t = all_text(&layout_plan(&mixed(), None, "d", false));
        assert!(t.join(" ").contains("No menu choices"));
    }

    #[test]
    fn nobody_with_dietary_needs() {
        let guests = vec![g("A", "1", "Main: x"), g("B", "2", "Main: x; Dessert: y")];
        let a = analyze(&guests);
        assert!(a.diet_portions().is_empty());
        assert_eq!(a.counts_which(None, Which::Standard), a.counts(None));
        let t = all_text(&layout_plan(&guests, None, "d", false));
        assert!(t.iter().any(|x| x == "No dietary portions."));
        assert!(t.iter().any(|x| x == "No dietary requirements recorded."));
    }

    #[test]
    fn plan_dietary_section() {
        // with entries: the section comes first on page 1, above Kitchen totals
        let l = layout_plan(&[gd("A", "1", "Main: x", "Nut allergy"), g("B", "1", "Main: y")], None, "d", false);
        let marks = &l.pages[0].marks;
        let di = marks.iter().position(|m| *m == Mark::Dietary).unwrap();
        let ti = marks.iter().position(|m| *m == Mark::Totals).unwrap();
        assert!(di < ti);
        let t = all_text(&l);
        assert!(t.iter().any(|x| x == "Dietary requirements & allergies"));
        assert!(!t.iter().any(|x| x == "No dietary requirements recorded."));
        assert!(t.iter().any(|x| x.contains("! Nut allergy")));
        assert!(t.iter().any(|x| x.starts_with("Dietary /")));
        // with none: one explicit line, and the column is still present
        let l = layout_plan(&[g("A", "1", "Main: x")], None, "d", false);
        let t = all_text(&l);
        assert!(t.iter().any(|x| x == "No dietary requirements recorded."));
        assert!(t.iter().any(|x| x.starts_with("Dietary /")));
        assert!(!t.iter().any(|x| x.starts_with("! ")));
    }

    #[test]
    fn long_allergy_text_does_not_overflow() {
        let long = "Severe tree nut and peanut allergy (carries EpiPen); coeliac - strictly no cross contamination; \
                    shellfish allergy; Supercalifragilisticexpialidocious-intolerance-with-an-extremely-long-hyphenated-word";
        let mut guests: Vec<Guest> = (0..30)
            .map(|i| gd(&format!("Guest {i} Fairly-Long-Surname"), &(i % 4 + 1).to_string(), "Starter: Soup; Main: Beef; Dessert: Tart", if i % 2 == 0 { long } else { "" }))
            .collect();
        guests.push(gd("Edge", "9", "A: x; B: y; C: z; D: w", long));
        for landscape in [false, true] {
            let l = layout_plan(&guests, Some("A rather long event title that goes on and on for quite a while"), "d", landscape);
            check_bounds(&l);
        }
        let l = layout_plan(&guests[..8], None, "d", false);
        assert!(!l.landscape);
        check_bounds(&l);
    }

    #[test]
    fn plan_pdf_loads() {
        let out = std::env::temp_dir().join("pc_plan.pdf");
        let guests = big(40, 10);
        let info = render_plan(&guests, Some("T"), false, &out).unwrap();
        let doc = printpdf::lopdf::Document::load(&out).unwrap();
        assert_eq!(doc.get_pages().len(), info.pages);
        assert!(info.pages >= 2);
        assert_eq!(today().len(), 10);
    }
}
