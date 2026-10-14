//! Font selection: built-in PDF fonts and installed TrueType fonts.

use std::path::{Path, PathBuf};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    Helvetica,
    HelveticaBold,
    HelveticaOblique,
    HelveticaBoldOblique,
    TimesRoman,
    TimesBold,
    TimesItalic,
    TimesBoldItalic,
    Courier,
    CourierBold,
    CourierOblique,
    CourierBoldOblique,
}

impl Builtin {
    pub const ALL: [Builtin; 12] = [
        Builtin::Helvetica,
        Builtin::HelveticaBold,
        Builtin::HelveticaOblique,
        Builtin::HelveticaBoldOblique,
        Builtin::TimesRoman,
        Builtin::TimesBold,
        Builtin::TimesItalic,
        Builtin::TimesBoldItalic,
        Builtin::Courier,
        Builtin::CourierBold,
        Builtin::CourierOblique,
        Builtin::CourierBoldOblique,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Builtin::Helvetica => "Helvetica",
            Builtin::HelveticaBold => "Helvetica-Bold",
            Builtin::HelveticaOblique => "Helvetica-Oblique",
            Builtin::HelveticaBoldOblique => "Helvetica-BoldOblique",
            Builtin::TimesRoman => "Times-Roman",
            Builtin::TimesBold => "Times-Bold",
            Builtin::TimesItalic => "Times-Italic",
            Builtin::TimesBoldItalic => "Times-BoldItalic",
            Builtin::Courier => "Courier",
            Builtin::CourierBold => "Courier-Bold",
            Builtin::CourierOblique => "Courier-Oblique",
            Builtin::CourierBoldOblique => "Courier-BoldOblique",
        }
    }

    pub fn from_name(s: &str) -> Option<Builtin> {
        Builtin::ALL.into_iter().find(|b| b.name().eq_ignore_ascii_case(s.trim()))
    }

    pub(crate) fn to_printpdf(self) -> printpdf::BuiltinFont {
        use printpdf::BuiltinFont as B;
        match self {
            Builtin::Helvetica => B::Helvetica,
            Builtin::HelveticaBold => B::HelveticaBold,
            Builtin::HelveticaOblique => B::HelveticaOblique,
            Builtin::HelveticaBoldOblique => B::HelveticaBoldOblique,
            Builtin::TimesRoman => B::TimesRoman,
            Builtin::TimesBold => B::TimesBold,
            Builtin::TimesItalic => B::TimesItalic,
            Builtin::TimesBoldItalic => B::TimesBoldItalic,
            Builtin::Courier => B::Courier,
            Builtin::CourierBold => B::CourierBold,
            Builtin::CourierOblique => B::CourierOblique,
            Builtin::CourierBoldOblique => B::CourierBoldOblique,
        }
    }
}

/// EB Garamond (SIL OFL 1.1), compiled into the program so it works on any Mac.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Bundled {
    Regular,
    Italic,
    Bold,
    BoldItalic,
}

impl Bundled {
    pub const ALL: [Bundled; 4] = [Bundled::Regular, Bundled::Italic, Bundled::Bold, Bundled::BoldItalic];

    pub fn name(self) -> &'static str {
        match self {
            Bundled::Regular => "EB Garamond",
            Bundled::Italic => "EB Garamond Italic",
            Bundled::Bold => "EB Garamond Bold",
            Bundled::BoldItalic => "EB Garamond Bold Italic",
        }
    }

    pub fn bytes(self) -> &'static [u8] {
        match self {
            Bundled::Regular => include_bytes!("../assets/fonts/EBGaramond-Regular.ttf"),
            Bundled::Italic => include_bytes!("../assets/fonts/EBGaramond-Italic.ttf"),
            Bundled::Bold => include_bytes!("../assets/fonts/EBGaramond-Bold.ttf"),
            Bundled::BoldItalic => include_bytes!("../assets/fonts/EBGaramond-BoldItalic.ttf"),
        }
    }

    pub fn from_name(s: &str) -> Option<Bundled> {
        Bundled::ALL.into_iter().find(|b| b.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// A font choice: a built-in PDF font, a bundled font, or a font file on disk.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontSpec {
    Builtin(Builtin),
    Bundled(Bundled),
    File(PathBuf),
}

impl FontSpec {
    /// Parse a built-in name (case-insensitive) or a path to a .ttf/.otf file.
    pub fn parse(s: &str) -> Result<FontSpec, String> {
        if let Some(b) = Builtin::from_name(s) {
            return Ok(FontSpec::Builtin(b));
        }
        if let Some(b) = Bundled::from_name(s) {
            return Ok(FontSpec::Bundled(b));
        }
        let p = Path::new(s.trim());
        if p.is_file() {
            Ok(FontSpec::File(p.to_path_buf()))
        } else {
            Err(format!("unknown font '{s}': not a built-in font name or an existing font file"))
        }
    }

    /// Text form that `parse` accepts (used for persistence).
    pub fn to_text(&self) -> String {
        match self {
            FontSpec::Builtin(b) => b.name().to_string(),
            FontSpec::Bundled(b) => b.name().to_string(),
            FontSpec::File(p) => p.to_string_lossy().into_owned(),
        }
    }
}

/// Font choices for each text element of a card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fonts {
    pub title: FontSpec,
    pub name: FontSpec,
    pub table: FontSpec,
    pub menu: FontSpec,
}

impl Default for Fonts {
    fn default() -> Self {
        Fonts {
            title: FontSpec::Builtin(Builtin::Helvetica),
            name: FontSpec::Builtin(Builtin::HelveticaBold),
            table: FontSpec::Builtin(Builtin::Helvetica),
            menu: FontSpec::Builtin(Builtin::Helvetica),
        }
    }
}

/// Advance width of `text` in em units (1.0 = font size) from a TrueType/OpenType font.
pub fn ttf_width_em(data: &[u8], text: &str) -> Option<f32> {
    let face = ttf_parser::Face::parse(data, 0).ok()?;
    let upm = face.units_per_em() as f32;
    if upm <= 0.0 {
        return None;
    }
    let mut total = 0.0;
    for c in text.chars() {
        let adv = face
            .glyph_index(c)
            .or(Some(ttf_parser::GlyphId(0)))
            .and_then(|g| face.glyph_hor_advance(g))
            .unwrap_or((upm * 0.5) as u16);
        total += adv as f32;
    }
    Some(total / upm)
}

/// True if `data` is a single-face font with TrueType (glyf) outlines.
pub fn is_truetype(data: &[u8]) -> bool {
    match ttf_parser::Face::parse(data, 0) {
        Ok(f) => f.tables().glyf.is_some() && f.units_per_em() > 0,
        Err(_) => false,
    }
}

/// True if `data` is a CFF-flavoured OpenType font ("OTTO").
pub fn is_cff(data: &[u8]) -> bool {
    match ttf_parser::Face::parse(data, 0) {
        Ok(f) => f.tables().cff.is_some() && f.units_per_em() > 0,
        Err(_) => false,
    }
}

/// True if `data` is a font the PDF writer can embed (TrueType or CFF OpenType).
pub fn embeddable(data: &[u8]) -> bool {
    is_truetype(data) || is_cff(data)
}

#[derive(Clone, Debug)]
pub struct SystemFont {
    pub name: String,
    pub path: PathBuf,
}

fn face_name(data: &[u8]) -> Option<String> {
    let face = ttf_parser::Face::parse(data, 0).ok()?;
    let names = face.names();
    for id in [
        ttf_parser::name_id::FULL_NAME,
        ttf_parser::name_id::FAMILY,
    ] {
        for n in names.into_iter().filter(|n| n.name_id == id) {
            if let Some(s) = n.to_string() {
                let s = s.trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn walk(dir: &Path, out: &mut Vec<SystemFont>, depth: usize) {
    if depth > 6 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out, depth + 1);
            continue;
        }
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext != "ttf" && ext != "otf" {
            continue; // also skips .ttc/.otc collections
        }
        let Ok(data) = std::fs::read(&p) else { continue };
        // A panic in a parser must not take the app down.
        let res = std::panic::catch_unwind(|| {
            if embeddable(&data) { face_name(&data) } else { None }
        });
        if let Ok(Some(name)) = res {
            out.push(SystemFont { name, path: p });
        }
    }
}

/// Scan the macOS font folders for embeddable TrueType fonts, sorted by name
/// and de-duplicated by name.
pub fn list_system_fonts() -> Vec<SystemFont> {
    let mut dirs = vec![PathBuf::from("/System/Library/Fonts"), PathBuf::from("/Library/Fonts")];
    if let Some(h) = std::env::var_os("HOME") {
        dirs.push(Path::new(&h).join("Library/Fonts"));
    }
    let mut out = Vec::new();
    for d in dirs {
        walk(&d, &mut out, 0);
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.path.cmp(&b.path)));
    out.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    out
}

/// Measures text for layout. Font files are read once and cached.
#[derive(Default)]
pub struct Measurer {
    cache: std::collections::HashMap<FontSpec, Option<std::sync::Arc<Vec<u8>>>>,
}

impl Measurer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Font file bytes for a bundled or file font (cached).
    fn data(&mut self, spec: &FontSpec) -> Option<std::sync::Arc<Vec<u8>>> {
        self.cache
            .entry(spec.clone())
            .or_insert_with(|| match spec {
                FontSpec::Builtin(_) => None,
                FontSpec::Bundled(b) => Some(std::sync::Arc::new(b.bytes().to_vec())),
                FontSpec::File(p) => std::fs::read(p).ok().filter(|d| embeddable(d)).map(std::sync::Arc::new),
            })
            .clone()
    }

    /// True if this font can be used (built-ins always can).
    pub fn usable(&mut self, spec: &FontSpec) -> bool {
        match spec {
            FontSpec::Builtin(_) => true,
            _ => self.data(spec).is_some(),
        }
    }

    /// Replace unusable font files by the default font for that element.
    /// Returns the effective fonts and a warning per replacement.
    pub fn resolve(&mut self, want: &Fonts) -> (Fonts, Vec<String>) {
        let d = Fonts::default();
        let mut warns = Vec::new();
        let mut one = |m: &mut Measurer, s: &FontSpec, fb: &FontSpec, label: &str| {
            if m.usable(s) {
                s.clone()
            } else {
                warns.push(format!("{label} font {}: not an embeddable TrueType font; using {}", s.to_text(), fb.to_text()));
                fb.clone()
            }
        };
        let f = Fonts {
            title: one(self, &want.title, &d.title, "title"),
            name: one(self, &want.name, &d.name, "name"),
            table: one(self, &want.table, &d.table, "table"),
            menu: one(self, &want.menu, &d.menu, "menu"),
        };
        (f, warns)
    }

    /// Width of `text` in millimetres at `size_pt`.
    pub fn width_mm(&mut self, spec: &FontSpec, text: &str, size_pt: f32) -> f32 {
        const PT_MM: f32 = 0.352_778;
        match spec {
            FontSpec::Builtin(b) => crate::metrics::width_mm(text, size_pt, *b),
            _ => match self.data(spec) {
                Some(d) => ttf_width_em(&d, text).unwrap_or(text.chars().count() as f32 * 0.55) * size_pt * PT_MM,
                None => text.chars().count() as f32 * 0.55 * size_pt * PT_MM,
            },
        }
    }
}
