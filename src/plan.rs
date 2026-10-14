//! Seating plan & menu PDF for back of house / food running.
//!
//! `analyze` parses menu choices into courses and dishes and counts them;
//! `layout_plan` is a pure function that paginates everything into positioned
//! items (all mm, y measured from the top of the page); `render_plan` draws
//! those items with printpdf. Text is always Helvetica / Helvetica-Bold.

use crate::dietary::{classify, Dietary, Item};
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
    /// Typed view of `dietary` (allergens / preferences / other).
    pub diet: Dietary,
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
    analyze_ordered(guests, &[])
}

/// Like `analyze`; course columns follow `order` (the event's courses, case-insensitive) and
/// any other labels come after, in first-seen order. Courses nobody chose get no column.
pub fn analyze_ordered(guests: &[Guest], order: &[String]) -> Analysis {
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
    // event course order first (stable), then any other labels as first seen
    {
        let pos = |c: &str| order.iter().position(|o| crate::courses::same_name(o, c)).unwrap_or(usize::MAX);
        courses.sort_by_key(|c| pos(c)); // stable: unknown labels keep first-seen order after the known ones
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
    let mut other_canon: Vec<String> = Vec::new();
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
        // Map entries to the allergen / preference checkboxes; Other entries keep the
        // first-seen spelling across all guests.
        let mut d = Dietary::from_entries(&g.dietary);
        let mut other: Vec<String> = Vec::new();
        for o in &d.other {
            let canon = match other_canon.iter().find(|x| x.eq_ignore_ascii_case(o)) {
                Some(x) => x.clone(),
                None => {
                    other_canon.push(o.clone());
                    o.clone()
                }
            };
            if !other.contains(&canon) {
                other.push(canon);
            }
        }
        d.other = other;
        let dietary = d.to_entries();
        for e in &dietary {
            if !diet.contains(e) {
                diet.push(e.clone());
            }
        }
        pg.push(PGuest { name: g.name.clone(), table: g.table.clone(), cells, has_menu, dietary, diet: d });
    }
    // Allergens first (fixed order), then preferences, then Other (first seen).
    diet.sort_by_key(|e| match classify(e) {
        Some(Item::Allergen(i)) => (0, i),
        Some(Item::Pref(i)) => (1, i),
        None => (2, 0),
    });
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
            g.diet.key()
        };
        let mut out: Vec<DietPortion> = Vec::new();
        let add = |col: Option<usize>, dish: &str, g: &PGuest, out: &mut Vec<DietPortion>| {
            let k = key(g);
            let who = (g.name.clone(), g.table.clone());
            if let Some(p) = out.iter_mut().find(|p| p.col == col && p.dish == dish && p.key == k) {
                p.guests.push(who);
            } else {
                out.push(DietPortion { col, dish: dish.to_string(), combo: g.diet.describe(), key: k, guests: vec![who] });
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

    /// "Allergens: Gluten ×1, Milk ×2 · Diet: Vegan ×1 · Other: no coriander ×1": counts per
    /// entry, allergens first, then preferences, then Other entries.
    pub fn diet_totals(&self) -> String {
        let counts = self.diet_counts(None);
        let mut groups: [Vec<String>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for (label, n) in &counts {
            let g = match classify(label) {
                Some(Item::Allergen(_)) => 0,
                Some(Item::Pref(_)) => 1,
                None => 2,
            };
            groups[g].push(format!("{label} \u{d7}{n}"));
        }
        let names = ["Allergens", "Diet", "Other"];
        groups
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_empty())
            .map(|(i, v)| format!("{}: {}", names[i], v.join(", ")))
            .collect::<Vec<_>>()
            .join(" \u{b7} ")
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
                format!("{} \u{2014} {} \u{2014} {}", g.name, dishes, g.diet.describe())
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
    /// Page size in mm and the sheet it belongs to ("Kitchen" / "Tables").
    pub w: f32,
    pub h: f32,
    pub sheet: &'static str,
}

#[derive(Debug)]
pub struct PlanLayout {
    /// Orientation of the first (Kitchen) page; each page carries its own size.
    pub landscape: bool,
    pub w: f32,
    pub h: f32,
    pub pages: Vec<PlanPage>,
    pub matrix_included: bool,
    /// E.g. "Tables sheet needs 2 pages even at 6 pt".
    pub warnings: Vec<String>,
    /// Body font size reached by each sheet (pt).
    pub kitchen_pt: f32,
    pub tables_pt: f32,
}

const MARGIN: f32 = 10.0;
/// Content starts below the one-line page header.
const TOP: f32 = 17.0;
/// Content ends this far above the page bottom (footer line), plus `KEY_ROOM` on page 1 when the key is shown.
const BOTTOM: f32 = 9.5;
const KEY_ROOM: f32 = 5.5;
const PT: f32 = 0.352_778;
/// Body text is 9 pt at full size and shrinks to a 6 pt floor; headings scale with it and stay <= 12 pt.
pub const BODY_MAX_PT: f32 = 9.0;
pub const BODY_MIN_PT: f32 = 6.0;
const GUTTER: f32 = 4.0;
const MAX_LINES: usize = 4;
const DIET_LINES: usize = 40;

thread_local! {
    static SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}
fn scale() -> f32 {
    SCALE.with(|c| c.get())
}
fn body() -> f32 {
    BODY_MAX_PT * scale()
}
fn head() -> f32 {
    9.0 * scale()
}
fn sub() -> f32 {
    9.0 * scale()
}
fn section() -> f32 {
    11.0 * scale()
}
/// Horizontal padding inside table cells (a little wider than the vertical padding).
fn hpad() -> f32 {
    1.0 * scale()
}
fn pad() -> f32 {
    0.3 * scale()
}
fn gap() -> f32 {
    2.0 * scale()
}

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
            // A single word wider than the cell: split it after a hyphen if possible, else by characters.
            if let Some(cut) = word.char_indices().filter(|(_, c)| *c == '-').map(|(i, _)| i + 1).filter(|i| *i < word.len() && tw(&word[..*i], size, bold) <= max_w).last() {
                lines.push(word[..cut].to_string());
                word = word[cut..].to_string();
                if lines.len() == max_lines && !word.is_empty() {
                    overflow = true;
                    break 'outer;
                }
                continue;
            }
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
    w: f32,
    h: f32,
    pages: Vec<PlanPage>,
    y: f32,
    /// Reserve room at the bottom of page 1 for the allergy-marker key.
    key: bool,
    /// Whether the current column already shows the table column header row.
    header_on_page: bool,
    /// Newspaper columns of the current zone: how many, the current one and its width.
    ncols: usize,
    col: usize,
    col_w: f32,
    zone_top: f32,
    zone_bottom: Option<f32>,
    max_y: f32,
    sheet: &'static str,
}

impl Flow {
    fn new(w: f32, h: f32, key: bool, sheet: &'static str) -> Self {
        Flow {
            w,
            h,
            pages: vec![PlanPage { w, h, sheet, ..Default::default() }],
            y: TOP,
            key,
            header_on_page: false,
            ncols: 1,
            col: 0,
            col_w: w - 2.0 * MARGIN,
            zone_top: TOP,
            zone_bottom: None,
            max_y: TOP,
            sheet,
        }
    }
    fn full_w(&self) -> f32 {
        self.w - 2.0 * MARGIN
    }
    /// Start a zone of `ncols` equal columns below the current position (optionally ending at `bottom`).
    fn begin_zone(&mut self, ncols: usize, bottom: Option<f32>) {
        self.ncols = ncols;
        self.col = 0;
        self.col_w = (self.full_w() - (ncols - 1) as f32 * GUTTER) / ncols as f32;
        self.zone_top = self.y;
        self.zone_bottom = bottom;
        self.max_y = self.y;
    }
    /// Back to one full-width column, below everything the zone drew on this page.
    fn end_zone(&mut self) {
        self.y = self.y.max(self.max_y);
        self.ncols = 1;
        self.col = 0;
        self.col_w = self.full_w();
        self.zone_bottom = None;
        self.header_on_page = false;
    }
    fn dx(&self) -> f32 {
        self.col as f32 * (self.col_w + GUTTER)
    }
    fn page(&mut self) -> &mut PlanPage {
        self.pages.last_mut().unwrap()
    }
    /// Next column of the zone, or a new page when the columns are used up.
    fn new_page(&mut self) {
        self.max_y = self.max_y.max(self.y);
        if self.col + 1 < self.ncols {
            self.col += 1;
            self.y = self.zone_top;
        } else {
            self.pages.push(PlanPage { w: self.w, h: self.h, sheet: self.sheet, ..Default::default() });
            self.col = 0;
            self.y = TOP;
            self.zone_top = TOP;
            self.max_y = TOP;
            self.zone_bottom = None;
        }
        self.header_on_page = false;
    }
    /// Start a new column/page unless `h` mm still fit (keeps headings with their first rows).
    fn ensure(&mut self, h: f32) {
        if self.room() < h {
            self.new_page();
        }
    }
    fn page_limit(&self) -> f32 {
        self.h - BOTTOM - if self.key && self.pages.len() == 1 { KEY_ROOM } else { 0.0 }
    }
    fn limit(&self) -> f32 {
        let base = self.page_limit();
        self.zone_bottom.map(|z| z.min(base)).unwrap_or(base)
    }
    fn room(&self) -> f32 {
        self.limit() - self.y
    }
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, grey: f32) {
        let dx = self.dx();
        self.page().items.push(PlanItem::Rect { x: x + dx, y, w, h, grey });
    }
    fn rule(&mut self, x1: f32, x2: f32, y: f32, grey: f32) {
        let dx = self.dx();
        self.page().items.push(PlanItem::Rule { x1: x1 + dx, x2: x2 + dx, y, grey });
    }
    fn text(&mut self, text: &str, x: f32, y: f32, size: f32, bold: bool, grey: f32) {
        let dx = self.dx();
        self.page().items.push(PlanItem::Text { text: text.into(), x: x + dx, y, size, bold, grey });
    }
    fn mark(&mut self, m: Mark) {
        self.page().marks.push(m);
    }
}

fn line_h(size: f32) -> f32 {
    size * PT * 1.05
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
        .map(|(i, (c, t))| wrap(t, size, bolds[i], c.w - 2.0 * hpad(), max_lines))
        .collect();
    let lines = cells.iter().map(|c| c.len()).max().unwrap_or(1);
    Row { cells, h: lines as f32 * line_h(size) + 2.0 * pad(), size, bolds, bar, fill, right }
}

fn draw_row(f: &mut Flow, cols: &[Col], r: &Row) {
    let y0 = f.y;
    let x1 = cols[0].x;
    let x2 = cols.last().map(|c| c.x + c.w).unwrap_or(x1);
    if let Some(g) = r.fill {
        f.rect(x1, y0, x2 - x1, r.h, g);
    }
    if r.bar {
        f.rect(x1 - 2.6, y0, 1.8, r.h, 0.0);
    }
    for (ci, (c, lines)) in cols.iter().zip(&r.cells).enumerate() {
        let bold = r.bolds[ci];
        for (li, l) in lines.iter().enumerate() {
            let base = y0 + pad() + li as f32 * line_h(r.size) + r.size * PT * 0.95;
            let x = if r.right.get(ci).copied().unwrap_or(false) {
                c.x + c.w - hpad() - tw(l, r.size, bold)
            } else {
                c.x + hpad()
            };
            f.text(l, x, base, r.size, bold, 0.0);
        }
    }
    f.rule(x1, x2, y0 + r.h, if r.bar { 0.35 } else { 0.75 });
    f.y += r.h;
}

/// Draw rows with the column header at the top and repeated on every new column/page; at least two rows
/// follow the header (no orphaned headers).
fn flow_rows(f: &mut Flow, cols: &[Col], hdr: &Row, rows: &[Row]) {
    for (i, r) in rows.iter().enumerate() {
        let next = rows.get(i + 1).map(|n| n.h).unwrap_or(0.0);
        if i == 0 {
            f.ensure(hdr.h + r.h + next);
            f.mark(Mark::ColHeader);
            draw_row(f, cols, hdr);
        } else if f.room() < r.h {
            f.new_page();
            f.mark(Mark::ColHeader);
            draw_row(f, cols, hdr);
        }
        draw_row(f, cols, r);
    }
}

/// Like `flow_rows`, for rows drawn with different column sets (`sets[row.0]`), e.g. full-width group rows
/// between table rows. A group row (set 1) is never left alone at the bottom of a column.
fn flow_rows_multi(f: &mut Flow, sets: &[&[Col]], hdr_set: usize, hdr: &Row, rows: &[(usize, Row)]) {
    for (i, (set, r)) in rows.iter().enumerate() {
        let next = rows.get(i + 1).map(|n| n.1.h).unwrap_or(0.0);
        let after = if *set == 1 { rows.get(i + 2).map(|n| n.1.h).unwrap_or(0.0) } else { 0.0 };
        if i == 0 {
            f.ensure(hdr.h + r.h + next + after);
            f.mark(Mark::ColHeader);
            draw_row(f, sets[hdr_set], hdr);
        } else if f.room() < r.h + if *set == 1 { next + after } else { 0.0 } {
            f.new_page();
            f.mark(Mark::ColHeader);
            draw_row(f, sets[hdr_set], hdr);
        }
        draw_row(f, sets[*set], r);
    }
}

fn heading(f: &mut Flow, text: &str, size: f32) {
    f.y += 0.8 * scale();
    let base = f.y + size * PT;
    f.text(text, MARGIN, base, size, true, 0.0);
    f.y = base + 1.2 * scale();
}

fn heading_h(size: f32) -> f32 {
    2.0 * scale() + size * PT
}

/// A shaded one-row sub-heading ("Table 3 · 4 guests").
fn group_row(f: &mut Flow, text: &str) {
    let h = group_row_h();
    let (y0, w) = (f.y, f.col_w);
    f.rect(MARGIN, y0, w, h, 0.72);
    let base = y0 + pad() + sub() * PT * 0.95;
    f.text(text, MARGIN + hpad(), base, sub(), true, 0.0);
    f.y += h;
}

fn group_row_h() -> f32 {
    line_h(sub()) + 2.0 * pad()
}

/// A wrapped text band under a table group.
fn band_h(lines: &[String]) -> f32 {
    lines.len() as f32 * line_h(body()) + 2.0 * pad()
}

fn draw_band(f: &mut Flow, lines: &[String], grey: f32) {
    let h = band_h(lines);
    let y0 = f.y;
    let w = f.col_w;
    f.rect(MARGIN, y0, w, h, grey);
    for (i, l) in lines.iter().enumerate() {
        let base = y0 + pad() + i as f32 * line_h(body()) + body() * PT * 0.95;
        f.text(l, MARGIN + hpad(), base, body(), true, 0.0);
    }
    f.y += h;
}

fn page_size(landscape: bool) -> (f32, f32) {
    if landscape { (297.0, 210.0) } else { (210.0, 297.0) }
}

/// Lay out the whole plan. `date` is printed in each page header.
pub fn layout_plan(guests: &[Guest], title: Option<&str>, date: &str, force_landscape: bool) -> PlanLayout {
    layout_plan_ex(guests, title, date, force_landscape, false)
}

/// Like `layout_plan`; with `marker_key` a one-line key for the place-card
/// allergy marker is added at the bottom of page 1 (only if anyone has dietary entries).
pub fn layout_plan_ex(guests: &[Guest], title: Option<&str>, date: &str, force_landscape: bool, marker_key: bool) -> PlanLayout {
    layout_plan_courses(guests, title, date, force_landscape, marker_key, &[])
}

fn with_scale<T>(s: f32, f: impl FnOnce() -> T) -> T {
    let old = scale();
    SCALE.with(|c| c.set(s));
    let r = f();
    SCALE.with(|c| c.set(old));
    r
}

/// Body font sizes tried, from the largest down to the floor.
fn body_steps() -> Vec<f32> {
    let mut v = Vec::new();
    let mut pt = BODY_MAX_PT;
    while pt >= BODY_MIN_PT - 0.01 {
        v.push(pt);
        pt -= 0.5;
    }
    v
}

/// One candidate sheet layout.
struct Sheet {
    pages: Vec<PlanPage>,
    pt: f32,
    landscape: bool,
    matrix: bool,
}

/// Pick the best layout of one sheet: the largest body size at which it fits one page (then portrait,
/// then fewer columns); if it never fits, the fewest pages at the floor size.
fn best_sheet(orients: &[bool], max_cols: usize, min_col_mm: f32, build: &dyn Fn(bool, usize, f32) -> Option<Sheet>) -> Sheet {
    let steps = body_steps();
    let mut fallback: Option<Sheet> = None;
    for (si, pt) in steps.iter().enumerate() {
        let s = pt / BODY_MAX_PT;
        let mut best: Option<Sheet> = None;
        for &landscape in orients {
            let (w, _) = page_size(landscape);
            for ncols in 1..=max_cols {
                let col_w = (w - 2.0 * MARGIN - (ncols - 1) as f32 * GUTTER) / ncols as f32;
                if ncols > 1 && col_w < min_col_mm * s {
                    continue;
                }
                if let Some(sh) = with_scale(s, || build(landscape, ncols, s)) {
                    if sh.pages.len() == 1 {
                        if best.is_none() {
                            best = Some(sh);
                        }
                    } else if si + 1 == steps.len() {
                        let better = fallback.as_ref().map(|f| sh.pages.len() < f.pages.len()).unwrap_or(true);
                        if better {
                            fallback = Some(sh);
                        }
                    }
                }
            }
            if best.is_some() {
                break; // portrait first; landscape only if portrait does not fit at this size
            }
        }
        if let Some(mut b) = best {
            b.pt = *pt;
            return b;
        }
    }
    let mut f = fallback.expect("a layout is always produced at the floor size");
    f.pt = BODY_MIN_PT;
    f
}

/// Smallest column width (mm, at full size) for the dish x table matrix: room for "10*".
fn matrix_min_col() -> f32 {
    1.9 * body() * PT + 2.0 * hpad()
}

fn matrix_fixed(landscape: bool) -> (f32, f32) {
    ((if landscape { 66.0 } else { 48.0 }) * scale(), 14.0 * scale())
}

fn matrix_fits(landscape: bool, n_tables: usize) -> bool {
    let (w, _) = page_size(landscape);
    let (label_w, total_w) = matrix_fixed(landscape);
    (w - 2.0 * MARGIN - label_w - total_w) / n_tables.max(1) as f32 >= matrix_min_col()
}

/// Layout of the two sheets: Kitchen (dietary list, totals, portions, matrix) and Tables (every table's
/// guests and runner summary), each scaled to fit one page (body 9 pt down to a 6 pt floor); a sheet that
/// still does not fit at the floor continues onto more pages and a warning is returned.
///
/// The orientation is chosen per sheet; `force_landscape` makes both landscape.
pub fn layout_plan_courses(
    guests: &[Guest],
    title: Option<&str>,
    date: &str,
    force_landscape: bool,
    marker_key: bool,
    courses: &[String],
) -> PlanLayout {
    let a = analyze_ordered(guests, courses);
    let marker_key = marker_key && a.guests.iter().any(|g| !g.dietary.is_empty());
    let n_tables = a.tables.len();
    let wanted = n_tables > 0 && a.dishes.iter().any(|d| !d.is_empty());
    let orients: Vec<bool> = if force_landscape { vec![true] } else { vec![false, true] };

    let kitchen = best_sheet(&orients, 3, 85.0, &|landscape, ncols, _s| {
        // the matrix is drawn whenever it fits at this size; a size where it cannot is skipped, except
        // at the floor, where it is left out with a note
        let fits = !wanted || matrix_fits(landscape, n_tables);
        if !fits && body() > BODY_MIN_PT + 0.01 {
            return None;
        }
        let matrix = wanted && fits;
        let pages = kitchen_sheet(&a, landscape, ncols, matrix, wanted && !fits, marker_key);
        Some(Sheet { pages, pt: body(), landscape, matrix })
    });
    let tables = best_sheet(&orients, 4, 62.0, &|landscape, ncols, _s| {
        let mut pages = tables_sheet(&a, landscape, ncols, false)?;
        // Only at the 6 pt floor, and only when it saves a page: let a table continue in the next column.
        if pages.len() > 1 && body() <= BODY_MIN_PT + 0.01 {
            if let Some(alt) = tables_sheet(&a, landscape, ncols, true) {
                if alt.len() < pages.len() {
                    pages = alt;
                }
            }
        }
        Some(Sheet { pages, pt: body(), landscape, matrix: false })
    });

    let mut warnings = Vec::new();
    if kitchen.pages.len() > 1 {
        warnings.push(format!("Kitchen sheet needs {} pages even at {} pt", kitchen.pages.len(), BODY_MIN_PT));
    }
    if tables.pages.len() > 1 {
        warnings.push(format!("Tables sheet needs {} pages even at {} pt", tables.pages.len(), BODY_MIN_PT));
    }
    let (landscape, w, h) = (kitchen.landscape, kitchen.pages[0].w, kitchen.pages[0].h);
    let (matrix_included, kitchen_pt, tables_pt) = (kitchen.matrix, kitchen.pt, tables.pt);
    let mut pages = kitchen.pages;
    pages.extend(tables.pages);

    // ---- Allergy-marker key (page 1, bottom), page headers and footers
    if marker_key {
        let ph = pages[0].h;
        let p = &mut pages[0];
        p.items.push(PlanItem::Dot { x: MARGIN + 1.4, y: ph - 10.0, r: 1.2 });
        p.items.push(PlanItem::Text {
            text: "on a place card = guest has a dietary requirement / allergy (see list above)".into(),
            x: MARGIN + 4.2,
            y: ph - 9.0,
            size: 8.0,
            bold: false,
            grey: 0.2,
        });
    }
    let total = pages.len();
    let head = match title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => format!("{t} \u{2014} Seating plan & menu"),
        None => "Seating plan & menu".to_string(),
    };
    let date_txt = format!("Generated {date}");
    for (i, p) in pages.iter_mut().enumerate() {
        let (pw, ph) = (p.w, p.h);
        let head = format!("{head} \u{2014} {}", p.sheet);
        let head = ellipsize(&head, 12.0, true, pw - 2.0 * MARGIN - tw(&date_txt, 8.0, false) - 6.0);
        p.items.push(PlanItem::Text { text: head, x: MARGIN, y: MARGIN + 3.6, size: 12.0, bold: true, grey: 0.0 });
        p.items.push(PlanItem::Text { text: date_txt.clone(), x: pw - MARGIN - tw(&date_txt, 8.0, false), y: MARGIN + 3.6, size: 8.0, bold: false, grey: 0.35 });
        p.items.push(PlanItem::Rule { x1: MARGIN, x2: pw - MARGIN, y: MARGIN + 5.2, grey: 0.4 });
        let foot = format!("Page {} of {}", i + 1, total);
        p.items.push(PlanItem::Text { text: foot.clone(), x: pw / 2.0 - tw(&foot, 8.0, false) / 2.0, y: ph - 5.0, size: 8.0, bold: false, grey: 0.35 });
    }
    PlanLayout { landscape, w, h, pages, matrix_included, warnings, kitchen_pt, tables_pt }
}

/// The dish x table matrix block (heading text, header row, rows, columns) for the full page width.
fn matrix_block(a: &Analysis, landscape: bool, cw: f32) -> (Vec<Col>, Row, Vec<Row>) {
    let n_tables = a.tables.len();
    let all_dishes: Vec<(usize, &String)> = a.dishes.iter().enumerate().flat_map(|(c, v)| v.iter().map(move |d| (c, d))).collect();
    let (label_w, total_w) = matrix_fixed(landscape);
    let tw_each = ((cw - label_w - total_w) / n_tables as f32).min(22.0 * scale());
    let mut cols = vec![Col { x: MARGIN, w: label_w }];
    for i in 0..n_tables {
        cols.push(Col { x: MARGIN + label_w + i as f32 * tw_each, w: tw_each });
    }
    cols.push(Col { x: MARGIN + label_w + n_tables as f32 * tw_each, w: total_w });
    let right: Vec<bool> = std::iter::once(false).chain((0..=n_tables).map(|_| true)).collect();
    let mut head_txt = vec!["Dish".to_string()];
    head_txt.extend(a.tables.iter().map(|t| ellipsize(&t.name, head(), true, tw_each - 2.0 * hpad())));
    head_txt.push("Total".into());
    let hr = make_row(&cols, &head_txt, head(), true, Some(0.82), right.clone(), 1);
    let per_table: Vec<Vec<Vec<(String, usize)>>> = a.tables.iter().map(|t| a.counts(Some(&t.guests))).collect();
    let per_table_diet: Vec<Vec<Vec<(String, usize)>>> = a.tables.iter().map(|t| a.counts_which(Some(&t.guests), Which::Dietary)).collect();
    let mut mrows: Vec<Row> = Vec::new();
    let mut last_course = usize::MAX;
    let mut shade = false;
    for (ci, dish) in &all_dishes {
        if *ci != last_course {
            last_course = *ci;
            let mut ct = vec![a.columns[*ci].clone()];
            ct.extend(std::iter::repeat(String::new()).take(n_tables + 1));
            mrows.push(make_row(&cols, &ct, body(), true, Some(0.9), right.clone(), 1));
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
        mrows.push(make_row(&cols, &txt, body(), false, shade.then_some(0.96), right.clone(), 2));
    }
    (cols, hr, mrows)
}

const MATRIX_TITLE: &str = "Dishes by table (all portions, incl. dietary)";

/// Kitchen sheet: dietary list, kitchen totals, dietary portions (in `ncols` columns) and the matrix below.
fn kitchen_sheet(a: &Analysis, landscape: bool, ncols: usize, matrix: bool, matrix_omitted: bool, marker_key: bool) -> Vec<PlanPage> {
    let n_tables = a.tables.len();
    let (w, h) = page_size(landscape);
    let mut f = Flow::new(w, h, marker_key, "Kitchen");
    let cw_full = f.full_w();
    let note_h = 1.0 + 8.0 * PT + 1.0;
    let mblock = matrix.then(|| matrix_block(a, landscape, cw_full));
    let matrix_h = mblock
        .as_ref()
        .map(|(_, hr, rows)| gap() + heading_h(sub()) + hr.h + rows.iter().map(|r| r.h).sum::<f32>() + note_h)
        .unwrap_or(0.0);
    let zone_bottom = (matrix_h > 0.0 && matrix_h < f.page_limit() - TOP - 25.0).then(|| f.page_limit() - matrix_h);
    f.begin_zone(ncols, zone_bottom);
    let cw = f.col_w;
    let flagged: Vec<(&str, &PGuest)> = a
        .tables
        .iter()
        .flat_map(|t| t.guests.iter().map(move |&gi| (t.name.as_str(), gi)))
        .filter(|(_, gi)| !a.guests[*gi].dietary.is_empty())
        .map(|(t, gi)| (t, &a.guests[gi]))
        .collect();

    // ---- Dietary requirements & allergies (first, so the kitchen cannot miss it)
    f.mark(Mark::Dietary);
    f.ensure(heading_h(section()) + 3.0 * line_h(body()) + 2.0 * (line_h(body()) + 2.0 * pad()));
    heading(&mut f, "Dietary requirements & allergies", section());
    if flagged.is_empty() {
        let base = f.y + body() * PT;
        f.text("No dietary requirements recorded.", MARGIN, base, body(), true, 0.0);
        f.y = base + 2.0 * scale();
    } else {
        for l in wrap(&format!("Totals: {}", a.diet_totals()), body(), true, cw, 6) {
            let base = f.y + body() * PT;
            f.text(&l, MARGIN, base, body(), true, 0.0);
            f.y = base + 1.2 * scale();
        }
        f.y += 0.8 * scale();
        let dcols = vec![
            Col { x: MARGIN, w: cw * 0.11 },
            Col { x: MARGIN + cw * 0.11, w: cw * 0.34 },
            Col { x: MARGIN + cw * 0.45, w: cw * 0.55 },
        ];
        let dh = make_row(&dcols, &["Table".into(), "Guest".into(), "Requirements".into()], head(), true, Some(0.82), vec![false; 3], 1);
        let rows: Vec<Row> = flagged
            .iter()
            .enumerate()
            .map(|(i, (tname, g))| {
                make_row_ex(
                    &dcols,
                    &[tname.to_string(), g.name.clone(), format!("! {}", g.diet.describe())],
                    body(),
                    vec![true, false, true],
                    true,
                    Some(if i % 2 == 0 { 0.86 } else { 0.78 }),
                    vec![false; 3],
                    DIET_LINES,
                )
            })
            .collect();
        flow_rows(&mut f, &dcols, &dh, &rows);
    }
    f.y += gap();

    // ---- Kitchen totals
    f.mark(Mark::Totals);
    f.ensure(heading_h(section()) + 2.0 * line_h(body()) + heading_h(sub()) + 3.0 * (line_h(body()) + 2.0 * pad()));
    heading(&mut f, "Kitchen totals", section());
    let n = a.guests.len();
    let n_diet = a.guests.iter().filter(|g| !g.dietary.is_empty()).count();
    let line = format!(
        "Total guests: {n}  \u{b7}  Tables: {n_tables}  \u{b7}  Guests with no menu choices: {}  \u{b7}  Guests with dietary requirements: {n_diet}",
        a.no_menu
    );
    for l in wrap(&line, body(), true, cw, 3) {
        let base = f.y + body() * PT;
        f.text(&l, MARGIN, base, body(), true, 0.0);
        f.y = base + 1.2 * scale();
    }
    f.y += 0.8 * scale();
    let std = a.counts_which(None, Which::Standard);
    let dietc = a.counts_which(None, Which::Dietary);
    let portions = a.diet_portions();
    heading(&mut f, "Standard portions", sub());
    let tot_cols = vec![
        Col { x: MARGIN, w: cw * 0.20 },
        Col { x: MARGIN + cw * 0.20, w: cw * 0.32 },
        Col { x: MARGIN + cw * 0.52, w: cw * 0.15 },
        Col { x: MARGIN + cw * 0.67, w: cw * 0.17 },
        Col { x: MARGIN + cw * 0.84, w: cw * 0.16 },
    ];
    let right5 = vec![false, false, true, true, true];
    let fit = |i: usize, long: &str, short: &str| if tw(long, head(), true) <= tot_cols[i].w - 2.0 * hpad() { long.to_string() } else { short.to_string() };
    let hdr = make_row(
        &tot_cols,
        &["Course".to_string(), "Dish".to_string(), fit(2, "Standard", "Std"), fit(3, "+ Dietary", "+Diet"), fit(4, "= Total", "=Tot")],
        head(),
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
                &[if di == 0 { a.columns[ci].clone() } else { String::new() }, dish.clone(), cnt.to_string(), d.to_string(), (cnt + d).to_string()],
                body(),
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
        let base = f.y + body() * PT;
        f.text("No menu choices have been entered.", MARGIN, base, body(), false, 0.3);
        f.y = base + 2.0 * scale();
    } else {
        flow_rows(&mut f, &tot_cols, &hdr, &rows);
    }

    f.y += gap();
    f.mark(Mark::DietaryPortions);
    f.ensure(heading_h(sub()) + 3.0 * (line_h(body()) + 2.0 * pad()));
    heading(&mut f, "Dietary / allergy portions", sub());
    if portions.is_empty() {
        let base = f.y + body() * PT;
        f.text("No dietary portions.", MARGIN, base, body(), true, 0.0);
        f.y = base + 2.0 * scale();
    } else {
        // Count | Requirements | Guests (Table); each course/dish is a shaded group row above its lines
        let pcols = vec![
            Col { x: MARGIN, w: cw * 0.10 },
            Col { x: MARGIN + cw * 0.10, w: cw * 0.36 },
            Col { x: MARGIN + cw * 0.46, w: cw * 0.54 },
        ];
        let full = vec![Col { x: MARGIN, w: cw }];
        let cnt = if tw("Count", head(), true) <= pcols[0].w - 2.0 * hpad() { "Count" } else { "#" };
        let ph = make_row(&pcols, &[cnt.into(), "Requirements".into(), "Guests (Table)".into()], head(), true, Some(0.82), vec![true, false, false], 2);
        let mut prows: Vec<(usize, Row)> = Vec::new();
        let mut last: (Option<usize>, String) = (Some(usize::MAX), String::new());
        for (i, p) in portions.iter().enumerate() {
            if last != (p.col, p.dish.clone()) {
                last = (p.col, p.dish.clone());
                let course = p.col.map(|c| a.columns[c].clone()).unwrap_or_else(|| "\u{2014}".into());
                prows.push((1, make_row_ex(&full, &[format!("{course} \u{b7} {}", p.dish)], body(), vec![true], false, Some(0.68), vec![false], 2)));
            }
            let guests = p.guests.iter().map(|(n, t)| format!("{n} (T{t})")).collect::<Vec<_>>().join(", ");
            prows.push((
                0,
                make_row_ex(
                    &pcols,
                    &[p.guests.len().to_string(), format!("! {}", p.combo), guests],
                    body(),
                    vec![true, true, false],
                    true,
                    Some(if i % 2 == 0 { 0.86 } else { 0.78 }),
                    vec![true, false, false],
                    DIET_LINES,
                ),
            ));
        }
        flow_rows_multi(&mut f, &[&pcols, &full], 0, &ph, &prows);
    }
    f.end_zone();

    // ---- Dish x table matrix (all portions), full width below the columns
    if let Some((cols, hr, mrows)) = mblock {
        f.y += gap();
        f.mark(Mark::Matrix);
        f.ensure(heading_h(sub()) + hr.h + 3.0 * (line_h(body()) + 2.0 * pad()));
        heading(&mut f, MATRIX_TITLE, sub());
        flow_rows(&mut f, &cols, &hr, &mrows);
        if f.room() < note_h {
            f.new_page();
        }
        let base = f.y + 1.0 + 8.0 * PT;
        f.text("* includes at least one dietary / allergy portion", MARGIN, base, 8.0, false, 0.3);
        f.y = base + 1.0;
    } else if matrix_omitted {
        f.y += 1.0;
        let base = f.y + 8.0 * PT;
        f.text(&format!("(Dish-by-table matrix omitted: {n_tables} tables are too many for the page width.)"), MARGIN, base, 8.0, false, 0.35);
    }
    if f.pages.len() > 1 && f.pages.last().map(|p| p.items.is_empty()).unwrap_or(false) {
        f.pages.pop();
    }
    f.pages
}

/// Tables sheet: every table's guests (course dishes and Dietary / allergies next to the name) and the
/// runner summary of standard portions, in `ncols` newspaper columns. A table is not split across columns
/// unless it is taller than a column or `split_ok` (used at the floor size to save a page).
fn tables_sheet(a: &Analysis, landscape: bool, ncols: usize, split_ok: bool) -> Option<Vec<PlanPage>> {
    let (w, h) = page_size(landscape);
    let mut f = Flow::new(w, h, false, "Tables");
    f.begin_zone(ncols, None);
    let cw = f.col_w;
    let n_cols = a.columns.len();
    // Column widths follow the content: every column is at least as wide as its longest word (nothing
    // breaks mid-word), grows towards its longest single-line text (capped, so long names and dishes
    // wrap), and the widths are then fitted to the column.
    let nc = n_cols + 2;
    let mut line = vec![0.0_f32; nc];
    let mut word = vec![0.0_f32; nc];
    let mut note = |i: usize, text: &str, bold: bool, size: f32| {
        line[i] = line[i].max(tw(text, size, bold));
        for wd in text.split_whitespace() {
            // a hyphenated word may break after its hyphens, so only its longest part must fit
            for part in wd.split_inclusive('-') {
                word[i] = word[i].max(tw(part, size, bold));
            }
        }
    };
    for g in &a.guests {
        note(0, &g.name, false, body());
        for (i, cell) in g.cells.iter().enumerate() {
            note(1 + i, &cell.join(", "), false, body());
        }
        if !g.dietary.is_empty() {
            note(nc - 1, &format!("! {}", g.diet.describe()), true, body());
        }
    }
    for (i, c) in a.columns.iter().enumerate() {
        note(1 + i, c, true, head());
    }
    note(nc - 1, "Dietary", true, head());
    let extra_pad = 2.0 * hpad() + 0.4;
    let lo: Vec<f32> = word.iter().map(|w| w + extra_pad).collect();
    let caps: Vec<f32> = (0..nc).map(|i| if i == 0 { 0.30 } else if i == nc - 1 { 0.36 } else { 0.22 } * cw).collect();
    let hi: Vec<f32> = (0..nc).map(|i| (line[i] + extra_pad).min(caps[i]).max(lo[i])).collect();
    let lo_sum: f32 = lo.iter().sum();
    // several columns are only used when every word fits its cell uncut
    if ncols > 1 && lo_sum > cw {
        return None;
    }
    let widths: Vec<f32> = if lo_sum >= cw {
        lo.iter().map(|l| l * cw / lo_sum).collect()
    } else {
        let want: f32 = hi.iter().zip(&lo).map(|(h, l)| h - l).sum();
        let room = cw - lo_sum;
        if want <= room {
            let hi_sum: f32 = hi.iter().sum();
            hi.iter().map(|h| h * cw / hi_sum).collect()
        } else {
            lo.iter().zip(&hi).map(|(l, h)| l + (h - l) * room / want).collect()
        }
    };
    let mut cols = Vec::new();
    let mut x = MARGIN;
    for wd in &widths {
        cols.push(Col { x, w: *wd });
        x += wd;
    }
    let mut head_txt = vec!["Guest name".to_string()];
    for c in &a.columns {
        // abbreviate course headers that do not fit their column
        head_txt.push(if tw(c, head(), true) > cols[1 + head_txt.len() - 1].w - 2.0 * hpad() { format!("{}.", c.chars().take(3).collect::<String>()) } else { c.clone() });
    }
    head_txt.push("Dietary / allergies".into());
    let hr = make_row(&cols, &head_txt, head(), true, Some(0.82), vec![false; cols.len()], 3);
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
                txt.push(if flagged { format!("! {}", g.diet.describe()) } else { "\u{2014}".into() });
                let mut bolds = vec![false; cols.len()];
                if flagged {
                    bolds[0] = true;
                    *bolds.last_mut().unwrap() = true;
                }
                let fill = if flagged { Some(0.8) } else { (i % 2 == 1).then_some(0.95) };
                // the dietary text is safety-critical: it wraps fully (no ellipsis)
                make_row_ex(&cols, &txt, body(), bolds, flagged, fill, vec![false; cols.len()], if flagged { 40 } else { MAX_LINES })
            })
            .collect();
        // runner summary: standard portions only (the dietary text is already in each guest's row)
        let (c_text, _) = a.summary_parts(&t.guests);
        let c_lines = wrap(&c_text, body(), true, cw - 2.0 * hpad(), 6);
        let sum_h = band_h(&c_lines) + 0.6 * scale();
        let label = format!("Table {} \u{b7} {} guest{}", t.name, t.guests.len(), if t.guests.len() == 1 { "" } else { "s" });
        let mut cont = false;
        let total_h = group_row_h() + hr.h + rows.iter().map(|r| r.h).sum::<f32>() + sum_h;
        let col_h = f.limit() - f.zone_top;
        // never split a table across columns unless it is taller than a whole column
        if !split_ok && f.y > f.zone_top + 0.01 && total_h <= col_h && f.room() < total_h {
            f.new_page();
        }
        let need_hdr = !f.header_on_page;
        let need = group_row_h() + if need_hdr { hr.h } else { 0.0 } + rows[0].h + if rows.len() == 1 { sum_h } else { 0.0 };
        f.ensure(need);
        let need_hdr = !f.header_on_page;
        f.mark(Mark::Section { table: t.name.clone(), cont });
        group_row(&mut f, &label);
        if need_hdr {
            f.mark(Mark::ColHeader);
            draw_row(&mut f, &cols, &hr);
            f.header_on_page = true;
        }
        let mut n_on_page = 0;
        for (k, r) in rows.iter().enumerate() {
            let required = r.h + if k + 1 == rows.len() { sum_h } else { 0.0 };
            if f.room() < required {
                f.mark(Mark::GuestRows { table: t.name.clone(), n: n_on_page });
                n_on_page = 0;
                f.new_page();
                cont = true;
                f.mark(Mark::Section { table: t.name.clone(), cont });
                group_row(&mut f, &format!("Table {} (cont.)", t.name));
                f.mark(Mark::ColHeader);
                draw_row(&mut f, &cols, &hr);
                f.header_on_page = true;
            }
            draw_row(&mut f, &cols, r);
            n_on_page += 1;
        }
        f.mark(Mark::GuestRows { table: t.name.clone(), n: n_on_page });
        f.mark(Mark::Summary { table: t.name.clone() });
        f.y += 0.6 * scale();
        draw_band(&mut f, &c_lines, 0.9);
        f.y += 1.4 * scale();
    }
    f.end_zone();
    if f.pages.len() > 1 && f.pages.last().map(|p| p.items.is_empty()).unwrap_or(false) {
        f.pages.pop();
    }
    Some(f.pages)
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
    /// Orientation of the Kitchen sheet.
    pub landscape: bool,
    pub warnings: Vec<String>,
    /// Body font size reached by each sheet (pt).
    pub kitchen_pt: f32,
    pub tables_pt: f32,
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
    render_plan_courses(guests, title, force_landscape, marker_key, &[], out)
}

/// Write the seating plan PDF with course columns in the event's course order.
pub fn render_plan_courses(
    guests: &[Guest],
    title: Option<&str>,
    force_landscape: bool,
    marker_key: bool,
    courses: &[String],
    out: &Path,
) -> Result<PlanInfo, Box<dyn std::error::Error>> {
    let lay = layout_plan_courses(guests, title, &today(), force_landscape, marker_key, courses);
    let (doc, p0, l0) = PdfDocument::new("Seating plan", Mm(lay.pages[0].w), Mm(lay.pages[0].h), "plan");
    let reg = doc.add_builtin_font(BuiltinFont::Helvetica)?;
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold)?;
    for (pi, page) in lay.pages.iter().enumerate() {
        let layer = if pi == 0 {
            doc.get_page(p0).get_layer(l0)
        } else {
            let (p, l) = doc.add_page(Mm(page.w), Mm(page.h), "plan");
            doc.get_page(p).get_layer(l)
        };
        let fy = |y: f32| Mm(page.h - y);
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
                        rings: vec![crate::pdf::circle_points(*x, page.h - *y, *r)
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
    Ok(PlanInfo { pages: lay.pages.len(), landscape: lay.landscape, warnings: lay.warnings.clone(), kitchen_pt: lay.kitchen_pt, tables_pt: lay.tables_pt })
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
        // adapted: each page has its own size now (sheets choose their own orientation)
        for p in &l.pages {
            for it in &p.items {
                if let PlanItem::Text { text, x, y, size, bold, .. } = it {
                    assert!(*x >= MARGIN - 0.01, "{text}");
                    assert!(x + tw(text, *size, *bold) <= p.w - MARGIN + 0.5, "'{text}' overflows");
                    assert!(*y <= p.h, "{text}");
                }
            }
        }
    }

    /// No text item may be larger than 12 pt, outside the page margins, or overlap another text item.
    fn check_layout_sane(l: &PlanLayout) {
        check_bounds(l);
        for (pi, p) in l.pages.iter().enumerate() {
            let boxes: Vec<(f32, f32, f32, f32, &str)> = p
                .items
                .iter()
                .filter_map(|it| match it {
                    PlanItem::Text { text, x, y, size, bold, .. } => {
                        assert!(*size <= 12.0, "'{text}' is {size} pt");
                        assert!(*size >= 6.0 - 0.01, "'{text}' is {size} pt (too small)");
                        let w = tw(text, *size, *bold);
                        Some((*x, x + w, y - 0.75 * size * PT, y + 0.2 * size * PT, text.as_str()))
                    }
                    _ => None,
                })
                .collect();
            for (a, b) in boxes.iter().enumerate().flat_map(|(i, a)| boxes[i + 1..].iter().map(move |b| (a, b))) {
                let overlap = a.0 < b.1 - 0.2 && b.0 < a.1 - 0.2 && a.2 < b.3 - 0.2 && b.2 < a.3 - 0.2;
                assert!(!overlap, "page {}: '{}' overlaps '{}'", pi + 1, a.4, b.4);
            }
        }
    }

    /// Structural pagination rules shared by the tests below.
    fn check_pagination(l: &PlanLayout, tables: usize, per_table: usize) {
        let mut rows_per_table: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for p in &l.pages {
            let mut header_seen = false;
            for (i, m) in p.marks.iter().enumerate() {
                match m {
                    Mark::ColHeader => header_seen = true,
                    Mark::Section { cont, .. } => {
                        if *cont {
                            // continued sections repeat their column headers
                            assert_eq!(p.marks.get(i + 1), Some(&Mark::ColHeader), "cont. section without header");
                        }
                    }
                    Mark::GuestRows { table, n } => {
                        // no orphan heading / heading + header at the bottom of a page
                        assert!(*n > 0, "table {table}: heading without any guest row");
                        assert!(header_seen, "guest rows without a column header on the page");
                        *rows_per_table.entry(table.clone()).or_default() += n;
                    }
                    Mark::Summary { table } => {
                        // no summary band alone at the top of a page
                        assert!(matches!(p.marks.get(i.wrapping_sub(1)), Some(Mark::GuestRows { table: t, n }) if t == table && *n > 0), "orphaned summary of table {table}");
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(rows_per_table.len(), tables);
        // every guest appears exactly once
        assert!(rows_per_table.values().all(|n| *n == per_table), "{rows_per_table:?}");
    }

    #[test]
    fn pagination_flows_continuously_without_orphans() {
        // Changed deliberately: a table is no longer kept on one page when it fits (that rule wasted
        // paper); sections flow on and may split, with the rules below instead.
        let guests = big(120, 15);
        for landscape in [false, true] {
            let l = layout_plan_courses(&guests, Some("Big Do"), "2026-01-01", landscape, true, &[]);
            check_layout_sane(&l);
            check_pagination(&l, 15, 8);
            assert!(!landscape || l.landscape);
            for (i, p) in l.pages.iter().enumerate() {
                let foot = format!("Page {} of {}", i + 1, l.pages.len());
                assert!(p.items.iter().any(|it| matches!(it, PlanItem::Text { text, .. } if *text == foot)));
            }
        }
        // some table section does split somewhere in a long list (adapted: with the compact, scaled
        // layout 3 tables of 40 guests no longer split, so the list is longer: 2 tables of 200)
        let l = layout_plan_courses(&big(400, 2), None, "d", false, false, &[]);
        check_layout_sane(&l);
        check_pagination(&l, 2, 200);
        assert!(l.pages.iter().flat_map(|p| &p.marks).any(|m| matches!(m, Mark::Section { cont: true, .. })));
    }

    #[test]
    fn auto_orientation_chooses_the_fewest_pages() {
        // Adapted from the old "fewest pages" test: the plan is now two sheets (Kitchen, Tables), each
        // choosing its own orientation and font size; the automatic choice never needs more pages than
        // forcing landscape, nor a smaller font.
        let order: Vec<String> = Vec::new();
        for (n, t) in [(6, 3), (40, 10), (120, 15), (30, 2), (90, 9)] {
            let guests = big(n, t);
            let auto = layout_plan_courses(&guests, None, "d", false, true, &order);
            let forced = layout_plan_courses(&guests, None, "d", true, true, &order);
            assert!(forced.pages.iter().all(|p| p.w > p.h), "forced landscape applies to every sheet");
            assert!(auto.pages.len() <= forced.pages.len(), "{n}/{t}");
            assert!(auto.kitchen_pt >= forced.kitchen_pt - 0.01 || auto.pages.len() < forced.pages.len(), "{n}/{t}");
            assert!(auto.tables_pt >= forced.tables_pt - 0.01 || auto.pages.len() < forced.pages.len(), "{n}/{t}");
        }
    }

    #[test]
    fn long_table_continues_with_repeated_header() {
        let guests = big(70, 1);
        let l = layout_plan(&guests, None, "2026-01-01", false);
        check_layout_sane(&l);
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
        // Changed deliberately: orientation is now chosen automatically (fewest pages, tie -> portrait)
        // instead of by a fixed rule, and the matrix is dropped only if its columns would be < 8 mm.
        let few = layout_plan(&big(6, 3), None, "d", false);
        assert!(!few.landscape && few.matrix_included);
        let forced = layout_plan(&big(6, 3), None, "d", true);
        assert!(forced.landscape);
        let many_tables = layout_plan(&big(40, 10), None, "d", false);
        assert!(many_tables.matrix_included);
        let fifteen = layout_plan(&big(60, 15), None, "d", false);
        assert!(fifteen.matrix_included, "15 tables still fit at >= 8 mm per column");
        // Adapted: the matrix columns now shrink with the font (down to about 5 mm at 6 pt), so 25 tables
        // still fit; it is omitted only when even a 6 pt matrix would not fit the page width.
        let twenty_five = layout_plan(&big(75, 25), None, "d", false);
        assert!(twenty_five.matrix_included, "25 tables fit at a smaller font");
        let sixty = layout_plan(&big(180, 60), None, "d", false);
        assert!(!sixty.matrix_included, "60 tables would need columns narrower than the 6 pt minimum");
        // many course columns still fit and stay inside the margins
        let cols = layout_plan(&[g("A", "1", "A: x; B: y; C: z; D: w")], None, "d", false);
        check_layout_sane(&cols);
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
        // Changed deliberately: entries are ordered allergens, then preferences, then Other
        // (so Vegan now precedes the Other entry "Nut allergy"), case-insensitive merge.
        assert_eq!(a.diet, vec!["Vegan", "Nut allergy"]);
        assert_eq!(a.guests[1].dietary, vec!["Nut allergy"]);
        assert!(a.guests[2].dietary.is_empty()); // only separators -> nothing
        assert_eq!(a.guests[3].dietary, vec!["Vegan"]); // duplicate within one guest counted once
        assert_eq!(a.diet_counts(None), vec![("Vegan".to_string(), 2), ("Nut allergy".to_string(), 2)]);
        // Table 1: A (Main x, dietary) and B (no menu, dietary) -> no standard plates.
        // (Changed deliberately: the course line now counts STANDARD portions only,
        // and the dietary band lists each modified plate per guest.)
        let (c, d) = a.summary_parts(&a.tables[0].guests);
        assert_eq!(c, "No standard portions");
        assert_eq!(
            d.as_deref(),
            Some("Dietary: A \u{2014} x \u{2014} Diet: Vegan \u{b7} Other: Nut allergy; B \u{2014} no menu choices recorded \u{2014} Other: Nut allergy")
        );
        // Table 2: C is standard (Main y); D has no menu but a requirement
        let (c, d) = a.summary_parts(&a.tables[1].guests);
        assert_eq!(c, "Main: y \u{d7}1");
        assert_eq!(d.as_deref(), Some("Dietary: D \u{2014} no menu choices recorded \u{2014} Diet: Vegan"));
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
        // "Gluten free" now maps to the Gluten allergen; "Shellfish allergy" is ambiguous so stays in Other
        assert_eq!(salmon[0].combo, "Allergens: Gluten \u{b7} Other: Shellfish allergy");
        // a guest's whole combination is one portion, not split
        assert!(!p.iter().any(|x| x.combo == "Allergens: Gluten"));
        // Zoe's Beef and Tart are separate dish rows
        assert_eq!(p.iter().filter(|x| x.combo == "Other: Nut allergy").count(), 2);
    }

    #[test]
    fn dietary_guest_without_menu_is_listed() {
        let a = analyze(&mixed());
        let p = a.diet_portions();
        let last = p.last().unwrap();
        assert_eq!((last.col, last.dish.as_str(), last.combo.as_str()), (None, NO_MENU, "Diet: Vegetarian"));
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
    fn plan_shows_allergens_diet_and_other_parts_and_ordered_totals() {
        let guests = vec![
            gd("Ann", "1", "Main: x", "Vegan; Milk; Peanuts; no coriander"),
            gd("Ben", "1", "Main: y", "Sesame"),
            gd("Cat", "2", "Main: x", "Halal"),
            gd("Dan", "2", "Main: y", "NO CORIANDER"),
        ];
        let a = analyze(&guests);
        // canonical order: allergens (fixed order), preferences, then Other
        assert_eq!(a.guests[0].dietary, vec!["Milk", "Peanuts", "Vegan", "no coriander"]);
        assert_eq!(a.guests[0].diet.describe(), "Allergens: Milk, Peanuts \u{b7} Diet: Vegan \u{b7} Other: no coriander");
        assert_eq!(a.guests[2].diet.describe(), "Diet: Halal"); // empty parts omitted
        // totals: per allergen first (fixed order), then preferences, then Other
        assert_eq!(
            a.diet_totals(),
            "Allergens: Milk \u{d7}1, Peanuts \u{d7}1, Sesame \u{d7}1 \u{b7} Diet: Vegan \u{d7}1, Halal \u{d7}1 \u{b7} Other: no coriander \u{d7}2"
        );
        // the same text reaches the document, in each place
        let joined = all_text(&layout_plan(&guests, None, "d", false)).join(" ");
        assert!(joined.contains("! Allergens: Milk, Peanuts"), "page-1 list / table column");
        assert!(joined.contains("Totals: Allergens: Milk"), "{joined}");
        assert!(joined.contains("Diet: Halal"));
        assert!(joined.contains("Other: no coriander"));
        // grouping uses the canonical sets, not the spelling
        let same = analyze(&[gd("A", "1", "Main: x", "Peanuts; Vegan"), gd("B", "1", "Main: x", "vegan; peanut allergy")]);
        assert_eq!(same.diet_portions().len(), 1);
        assert_eq!(same.diet_portions()[0].guests.len(), 2);
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
        assert!(t.iter().any(|x| x.contains("! Other: Nut allergy")));
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
    fn plan_columns_follow_course_order() {
        // first-seen order would be Dessert, Main, Starter
        let guests = vec![g("A", "1", "Dessert: Tart; Main: Beef"), g("B", "1", "Starter: Soup; Cheese: Brie")];
        let first_seen = analyze(&guests);
        assert_eq!(first_seen.columns, vec!["Dessert", "Main", "Starter", "Cheese"]);
        let order: Vec<String> = ["Starter", "Main", "Dessert"].iter().map(|s| s.to_string()).collect();
        let a = analyze_ordered(&guests, &order);
        assert_eq!(a.columns, vec!["Starter", "Main", "Dessert", "Cheese"]); // unknown labels after, first-seen
        let l = layout_plan_courses(&guests, None, "d", false, false, &order);
        let t = all_text(&l);
        let pos = |name: &str| t.iter().position(|x| x == name);
        assert!(pos("Starter") < pos("Main") && pos("Main") < pos("Dessert"), "{t:?}");
    }


    #[test]
    fn two_sheets_each_scaled_to_one_page() {
        let l = layout_plan_courses(&big(30, 4), Some("T"), "d", false, true, &[]);
        assert_eq!(l.pages.len(), 2, "kitchen sheet + tables sheet");
        assert_eq!((l.pages[0].sheet, l.pages[1].sheet), ("Kitchen", "Tables"));
        assert!(l.warnings.is_empty());
        assert!(l.pages[0].marks.contains(&Mark::Totals) && l.pages[1].marks.iter().any(|m| matches!(m, Mark::Section { .. })));
        // page header names the sheet
        let heads: Vec<String> = l.pages.iter().map(|p| p.items.iter().find_map(|i| match i {
            PlanItem::Text { text, size, .. } if *size == 12.0 => Some(text.clone()), _ => None }).unwrap()).collect();
        assert!(heads[0].ends_with("Kitchen") && heads[1].ends_with("Tables"), "{heads:?}");
        check_layout_sane(&l);
    }

    #[test]
    fn synthetic_120_guest_list_real_numbers() {
        // 120 guests over 15 tables with long dish and guest names: the Kitchen sheet fits one page at 9 pt and
        // the Tables sheet fits one page only at the 6 pt floor (4 columns, a table continuing in the next
        // column), the real numbers measured here
        let l = layout_plan_courses(&big(120, 15), None, "d", false, true, &[]);
        let kitchen = l.pages.iter().filter(|p| p.sheet == "Kitchen").count();
        assert_eq!(kitchen, 1);
        assert_eq!(l.pages.len() - kitchen, 1);
        assert_eq!((l.kitchen_pt, l.tables_pt), (9.0, 6.0));
        assert!(l.warnings.is_empty());
        check_layout_sane(&l);
        check_pagination(&l, 15, 8);
        // a smaller event fits both sheets on one page each, with no warning
        let l = layout_plan_courses(&big(60, 8), None, "d", false, true, &[]);
        assert_eq!(l.pages.len(), 2);
        assert!(l.warnings.is_empty() && l.tables_pt >= 6.0);
        check_layout_sane(&l);
    }

    #[test]
    fn no_per_table_dietary_band_but_dietary_text_stays_in_the_rows() {
        let guests = vec![gd("Ann", "1", "Main: x", "Peanuts; Vegan"), g("Ben", "1", "Main: y"), gd("Cy", "2", "", "no pork")];
        let l = layout_plan(&guests, None, "d", false);
        let tables_text: Vec<String> = l.pages.iter().filter(|p| p.sheet == "Tables").flat_map(|p| p.items.iter()).filter_map(|i| match i {
            PlanItem::Text { text, .. } => Some(text.clone()), _ => None }).collect();
        assert!(!tables_text.iter().any(|t| t.starts_with("! Dietary:")), "the per-table dietary band is gone");
        let joined = tables_text.join(" ");
        assert!(joined.contains("! Allergens: Peanuts"), "{joined}");
        assert!(joined.contains("Diet: Vegan") && joined.contains("! Other: no pork"));
        // the runner summary (standard portions only) stays, including the "No standard portions" case
        assert!(joined.contains("Main: y"));
        assert!(joined.contains("No standard portions") || joined.contains("No menu choices"));
        // and nothing in the dietary text is ever cut with an ellipsis
        assert!(!tables_text.iter().any(|t| t.contains('\u{2026}')));
    }

    #[test]
    fn overflow_at_the_floor_warns_and_continues_on_more_pages() {
        let l = layout_plan_courses(&big(400, 10), None, "d", false, true, &[]);
        assert!(l.pages.len() > 2);
        assert_eq!(l.tables_pt, 6.0, "never smaller than the floor");
        assert!(l.warnings.iter().any(|w| w.contains("Tables sheet needs") && w.contains("even at 6 pt")), "{:?}", l.warnings);
        let t = all_text(&l);
        assert!(t.iter().any(|x| x.contains("(cont.)")), "continued sections are labelled");
        check_layout_sane(&l);
        check_pagination(&l, 10, 40);
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
