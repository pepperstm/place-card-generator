//! A4 page layout: 2 x 3 grid of fold-over tent cards.
//!
//! Geometry (all mm). The page is 210 x 297. Most printers cannot print the
//! outer ~4 mm, so the grid is inset by `MARGIN` = 5 mm on every side, giving
//! cells of 100 x 95.67 mm (instead of the nominal 105 x 99). Each cell folds
//! at its horizontal midline into a card face of 100 x 47.83 mm.
//!
//! `layout_cell` is a pure function producing positioned text; both the PDF
//! writer below and the GUI preview consume its output, so they always agree.

use crate::cff::{self, CffFont};
use crate::fonts::{self, FontSpec, Fonts, Measurer};
use crate::Guest;
use printpdf::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

pub const PAGE_W: f32 = 210.0;
pub const PAGE_H: f32 = 297.0;
pub const MARGIN: f32 = 5.0;
pub const COLS: usize = 2;
pub const ROWS: usize = 3;
pub const CELL_W: f32 = (PAGE_W - 2.0 * MARGIN) / COLS as f32;
pub const CELL_H: f32 = (PAGE_H - 2.0 * MARGIN) / ROWS as f32;
const PAD: f32 = 5.0; // text padding from cell edges
pub const PT_MM: f32 = 0.352_778;

/// One piece of text, positioned in page coordinates (mm, origin bottom-left,
/// y up). `(x_mm, y_mm)` is where the baseline starts. When `rotated` is true
/// the text is turned 180 degrees about that point, so it runs leftwards.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedText {
    pub text: String,
    pub size_pt: f32,
    pub font: FontSpec,
    pub grey: f32,
    pub x_mm: f32,
    pub y_mm: f32,
    pub rotated: bool,
}

struct TextLine {
    text: String,
    size: f32,
    font: FontSpec,
    grey: f32,
}

fn build_lines(g: &Guest, title: Option<&str>, f: &Fonts, m: &mut Measurer) -> Vec<TextLine> {
    let max_w = CELL_W - 2.0 * PAD;
    let mut fit = |t: &str, start: f32, min: f32, font: &FontSpec| {
        let w = m.width_mm(font, t, start);
        if w <= max_w { start } else { (start * max_w / w).max(min) }
    };
    let mut v = Vec::new();
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        v.push(TextLine { text: t.into(), size: fit(t, 10.0, 6.0, &f.title), font: f.title.clone(), grey: 0.4 });
    }
    v.push(TextLine { text: g.name.clone(), size: fit(&g.name, 30.0, 8.0, &f.name), font: f.name.clone(), grey: 0.0 });
    let tbl = format!("Table {}", g.table);
    v.push(TextLine { size: fit(&tbl, 16.0, 8.0, &f.table), text: tbl, font: f.table.clone(), grey: 0.15 });
    let msize = if g.menu.len() > 3 { 8.0 } else { 9.5 };
    for mn in &g.menu {
        v.push(TextLine { text: mn.clone(), size: fit(mn, msize, 5.5, &f.menu), font: f.menu.clone(), grey: 0.3 });
    }
    v
}

/// A small solid dot marking a guest with a dietary requirement / allergy
/// (drawn as a vector circle, never a font glyph; the requirement itself is not printed).
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub x_mm: f32,
    pub y_mm: f32,
    pub r_mm: f32,
    pub rotated: bool,
}

/// Diameter of the allergy marker is 2 * `MARKER_R`; its centre sits this far
/// from the cell's right and bottom edges (about 5.4 mm of clearance from the cut lines).
pub const MARKER_R: f32 = 1.6;
const MARKER_INSET: f32 = 7.0;

/// Options for card output.
#[derive(Clone, Debug)]
pub struct CardOptions {
    /// Draw the small allergy marker on cards of guests with dietary entries.
    pub allergy_marker: bool,
}

impl Default for CardOptions {
    fn default() -> Self {
        CardOptions { allergy_marker: true }
    }
}

/// Everything drawn for one cell.
pub struct CellLayout {
    pub texts: Vec<PlacedText>,
    pub markers: Vec<Marker>,
}

/// Text-only convenience wrapper around `layout_cell_ex`.
pub fn layout_cell(g: &Guest, title: Option<&str>, fonts: &Fonts, m: &mut Measurer, cx: f32, cy: f32) -> Vec<PlacedText> {
    layout_cell_ex(g, title, fonts, m, cx, cy, false).texts
}

/// Pure layout of one cell centred at `(cx, cy)` (page mm). `fonts` should
/// already be resolved (see `Measurer::resolve`). Texts are the upright items
/// (bottom half) followed by the same items rotated 180 degrees (top half). If
/// `allergy_marker` is set and the guest has any dietary entry, a marker is
/// added at the bottom-right of the upright half and the mirrored position of
/// the rotated half.
pub fn layout_cell_ex(
    g: &Guest,
    title: Option<&str>,
    fonts: &Fonts,
    m: &mut Measurer,
    cx: f32,
    cy: f32,
    allergy_marker: bool,
) -> CellLayout {
    let lines = build_lines(g, title, fonts, m);
    let gap = |s: f32| s * PT_MM * 0.35; // extra spacing after a line
    let heights: Vec<f32> = lines.iter().map(|l| l.size * PT_MM * 1.1 + gap(l.size)).collect();
    let total: f32 = heights.iter().sum();
    // Bottom face spans cy - CELL_H/2 .. cy; centre block vertically in it.
    let face_mid = cy - CELL_H / 4.0;
    let mut out = Vec::new();
    let mut markers = Vec::new();
    if allergy_marker && (g.dietary.iter().any(|d| !d.trim().is_empty()) || !g.notes.trim().is_empty()) {
        let (mx, my) = (cx + CELL_W / 2.0 - MARKER_INSET, cy - CELL_H / 2.0 + MARKER_INSET);
        markers.push(Marker { x_mm: mx, y_mm: my, r_mm: MARKER_R, rotated: false });
        markers.push(Marker { x_mm: 2.0 * cx - mx, y_mm: 2.0 * cy - my, r_mm: MARKER_R, rotated: true });
    }
    for flip in [false, true] {
        let mut top = face_mid + total / 2.0;
        for (l, h) in lines.iter().zip(&heights) {
            let baseline = top - l.size * PT_MM * 0.85;
            top -= h;
            let w = m.width_mm(&l.font, &l.text, l.size);
            let (x, y) = if flip {
                (2.0 * cx - (cx - w / 2.0), 2.0 * cy - baseline)
            } else {
                (cx - w / 2.0, baseline)
            };
            out.push(PlacedText {
                text: l.text.clone(),
                size_pt: l.size,
                font: l.font.clone(),
                grey: l.grey,
                x_mm: x,
                y_mm: y,
                rotated: flip,
            });
        }
    }
    CellLayout { texts: out, markers }
}

/// Polygon points (page mm) approximating a filled circle.
pub fn circle_points(x: f32, y: f32, r: f32) -> Vec<(f32, f32)> {
    (0..32)
        .map(|i| {
            let a = i as f32 / 32.0 * std::f32::consts::TAU;
            (x + r * a.cos(), y + r * a.sin())
        })
        .collect()
}

/// Centre of the cell at slot `i` (0..6) on a page; slot 0 is top-left.
pub fn cell_center(i: usize) -> (f32, f32) {
    let (col, row) = (i % COLS, i / COLS);
    (
        MARGIN + (col as f32 + 0.5) * CELL_W,
        PAGE_H - MARGIN - (row as f32 + 0.5) * CELL_H,
    )
}

/// Number of A4 pages needed for `n` guests.
pub fn page_count(n: usize) -> usize {
    n.div_ceil(COLS * ROWS)
}

fn seg(layer: &PdfLayerReference, a: (f32, f32), b: (f32, f32)) {
    layer.add_line(Line {
        points: vec![
            (Point::new(Mm(a.0), Mm(a.1)), false),
            (Point::new(Mm(b.0), Mm(b.1)), false),
        ],
        is_closed: false,
    });
}

fn draw_guides(layer: &PdfLayerReference) {
    layer.set_outline_thickness(0.3);
    // Cut lines: light grey dashed, the full grid lines.
    layer.set_outline_color(Color::Greyscale(Greyscale::new(0.7, None)));
    layer.set_line_dash_pattern(LineDashPattern { dash_1: Some(3), gap_1: Some(2), ..Default::default() });
    for c in 0..=COLS {
        let x = MARGIN + c as f32 * CELL_W;
        seg(layer, (x, MARGIN), (x, PAGE_H - MARGIN));
    }
    for r in 0..=ROWS {
        let y = MARGIN + r as f32 * CELL_H;
        seg(layer, (MARGIN, y), (PAGE_W - MARGIN, y));
    }
    // Fold lines: lighter, finer dots across the middle of each cell.
    layer.set_outline_color(Color::Greyscale(Greyscale::new(0.85, None)));
    layer.set_line_dash_pattern(LineDashPattern { dash_1: Some(1), gap_1: Some(2), ..Default::default() });
    for r in 0..ROWS {
        let y = MARGIN + (r as f32 + 0.5) * CELL_H;
        seg(layer, (MARGIN, y), (PAGE_W - MARGIN, y));
    }
    layer.set_line_dash_pattern(LineDashPattern::default());
}

fn embed_bytes(doc: &PdfDocumentReference, data: &[u8]) -> Result<IndirectFontRef, String> {
    if !fonts::is_truetype(data) {
        return Err("not an embeddable TrueType font".into());
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| doc.add_external_font(data)))
        .map_err(|_| "font embedding panicked".to_string())?
        .map_err(|e| e.to_string())
}

fn load_external(doc: &PdfDocumentReference, path: &Path) -> Result<IndirectFontRef, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    embed_bytes(doc, &data)
}

/// Render with the default fonts. Returns the page count.
pub fn render(guests: &[Guest], title: Option<&str>, out: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    Ok(render_with(guests, title, &Fonts::default(), out)?.0)
}

/// Render with chosen fonts. Returns the page count and any warnings (for
/// example a font that could not be embedded and was replaced by the default).
pub fn render_with(
    guests: &[Guest],
    title: Option<&str>,
    want: &Fonts,
    out: &Path,
) -> Result<(usize, Vec<String>), Box<dyn std::error::Error>> {
    render_opts(guests, title, want, &CardOptions::default(), out)
}

/// Like `render_with`, with explicit card options (allergy marker on/off).
pub fn render_opts(
    guests: &[Guest],
    title: Option<&str>,
    want: &Fonts,
    opts: &CardOptions,
    out: &Path,
) -> Result<(usize, Vec<String>), Box<dyn std::error::Error>> {
    let (doc, p0, l0) = PdfDocument::new("Place cards", Mm(PAGE_W), Mm(PAGE_H), "cards");
    let mut measurer = Measurer::new();
    let (mut eff, mut warnings) = measurer.resolve(want);

    // Embed each distinct font once; on failure fall back to that element's default.
    let defaults = Fonts::default();
    let mut refs: HashMap<FontSpec, IndirectFontRef> = HashMap::new();
    let mut cffs: HashMap<FontSpec, CffFont> = HashMap::new();
    let mut ensure = |spec: &mut FontSpec, fallback: &FontSpec, label: &str, warnings: &mut Vec<String>|
     -> Result<(), Box<dyn std::error::Error>> {
        // CFF-flavoured OpenType fonts are embedded by hand (see cff.rs).
        if cffs.contains_key(spec) {
            return Ok(());
        }
        if let FontSpec::File(p) = &*spec {
            if let Ok(d) = std::fs::read(p) {
                if fonts::is_cff(&d) {
                    let name = format!("FC{}", cffs.len());
                    cffs.insert(spec.clone(), CffFont::new(name, std::sync::Arc::new(d)));
                    return Ok(());
                }
            }
        }
        if !refs.contains_key(spec) {
            let r = match &*spec {
                FontSpec::Builtin(b) => Some(doc.add_builtin_font(b.to_printpdf())?),
                FontSpec::Bundled(b) => match embed_bytes(&doc, b.bytes()) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        warnings.push(format!("{label} font {}: {e}; using {}", b.name(), fallback.to_text()));
                        None
                    }
                },
                FontSpec::File(p) => match load_external(&doc, p) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        warnings.push(format!("{label} font {}: {e}; using {}", p.display(), fallback.to_text()));
                        None
                    }
                },
            };
            match r {
                Some(r) => {
                    refs.insert(spec.clone(), r);
                }
                None => {
                    *spec = fallback.clone();
                    if !refs.contains_key(spec) {
                        let FontSpec::Builtin(b) = &*spec else { unreachable!() };
                        refs.insert(spec.clone(), doc.add_builtin_font(b.to_printpdf())?);
                    }
                }
            }
        }
        Ok(())
    };
    ensure(&mut eff.title, &defaults.title, "title", &mut warnings)?;
    ensure(&mut eff.name, &defaults.name, "name", &mut warnings)?;
    ensure(&mut eff.table, &defaults.table, "table", &mut warnings)?;
    ensure(&mut eff.menu, &defaults.menu, "menu", &mut warnings)?;

    let per_page = COLS * ROWS;
    let mut pages = 0;
    for (pi, chunk) in guests.chunks(per_page).enumerate() {
        let layer = if pi == 0 {
            doc.get_page(p0).get_layer(l0)
        } else {
            let (p, l) = doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "cards");
            doc.get_page(p).get_layer(l)
        };
        pages += 1;
        draw_guides(&layer);
        for (i, g) in chunk.iter().enumerate() {
            let (cx, cy) = cell_center(i);
            let cell = layout_cell_ex(g, title, &eff, &mut measurer, cx, cy, opts.allergy_marker);
            for mk in &cell.markers {
                layer.set_fill_color(Color::Greyscale(Greyscale::new(0.0, None)));
                layer.add_polygon(Polygon {
                    rings: vec![circle_points(mk.x_mm, mk.y_mm, mk.r_mm)
                        .into_iter()
                        .map(|(x, y)| (Point::new(Mm(x), Mm(y)), false))
                        .collect()],
                    mode: path::PaintMode::Fill,
                    winding_order: path::WindingOrder::NonZero,
                });
            }
            for t in cell.texts {
                let m = if t.rotated { [-1.0, 0.0, 0.0, -1.0] } else { [1.0, 0.0, 0.0, 1.0] };
                let tm = TextMatrix::Raw([m[0], m[1], m[2], m[3], Mm(t.x_mm).into_pt().0, Mm(t.y_mm).into_pt().0]);
                layer.set_fill_color(Color::Greyscale(Greyscale::new(t.grey, None)));
                layer.begin_text_section();
                if let Some(c) = cffs.get_mut(&t.font) {
                    use printpdf::lopdf::{content::Operation, Object, StringFormat};
                    let codes = c.encode(&t.text);
                    layer.add_operation(Operation::new(
                        "Tf",
                        vec![Object::Name(c.res_name.clone().into_bytes()), Object::Real(t.size_pt)],
                    ));
                    layer.set_text_matrix(tm);
                    layer.add_operation(Operation::new("Tj", vec![Object::String(codes, StringFormat::Hexadecimal)]));
                } else {
                    let fref = &refs[&t.font];
                    layer.set_font(fref, t.size_pt);
                    layer.set_text_matrix(tm);
                    layer.write_text(t.text.clone(), fref);
                }
                layer.end_text_section();
            }
        }
    }
    if cffs.is_empty() {
        doc.save(&mut BufWriter::new(File::create(out)?))?;
        return Ok((pages, warnings));
    }
    let bytes = doc.save_to_bytes()?;
    let list: Vec<CffFont> = cffs.into_values().collect();
    match cff::add_to_pdf(&bytes, &list) {
        Ok(b) => {
            std::fs::write(out, b)?;
            Ok((pages, warnings))
        }
        Err(e) => {
            // Could not write the CFF fonts: redo the PDF without them.
            let d = Fonts::default();
            let swap = |s: &FontSpec, fb: &FontSpec| if matches!(s, FontSpec::File(p) if std::fs::read(p).map(|x| fonts::is_cff(&x)).unwrap_or(false)) { fb.clone() } else { s.clone() };
            let safe = Fonts { title: swap(&want.title, &d.title), name: swap(&want.name, &d.name), table: swap(&want.table, &d.table), menu: swap(&want.menu, &d.menu) };
            let (n, mut w) = render_opts(guests, title, &safe, opts, out)?;
            w.push(format!("CFF font could not be embedded ({e}); used default fonts instead"));
            Ok((n, w))
        }
    }
}

