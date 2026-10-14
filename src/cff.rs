//! Support for CFF-flavoured OpenType fonts (`.otf` files starting with "OTTO").
//!
//! printpdf 0.7 can only embed TrueType-outline fonts, so CFF fonts are written
//! by hand: text is emitted as raw 2-byte glyph IDs, and after printpdf has
//! produced the document we add, via lopdf, a Type0 / Identity-H font with a
//! CIDFontType0 descendant, a FontDescriptor, the whole .otf as FontFile3
//! (/Subtype /OpenType), a W widths array and a ToUnicode CMap.

use printpdf::lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct CffFont {
    pub res_name: String,
    pub data: Arc<Vec<u8>>,
    /// glyph id -> a character that maps to it (for widths and ToUnicode)
    pub used: BTreeMap<u16, char>,
}

impl CffFont {
    pub fn new(res_name: String, data: Arc<Vec<u8>>) -> Self {
        Self { res_name, data, used: BTreeMap::new() }
    }

    /// Encode text as big-endian 2-byte glyph IDs (missing glyphs become .notdef).
    pub fn encode(&mut self, text: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let Ok(face) = ttf_parser::Face::parse(&self.data, 0) else { return out };
        for c in text.chars() {
            let gid = face.glyph_index(c).map(|g| g.0).unwrap_or(0);
            self.used.entry(gid).or_insert(c);
            out.extend_from_slice(&gid.to_be_bytes());
        }
        out
    }
}

fn name_of(face: &ttf_parser::Face, id: u16) -> Option<String> {
    face.names().into_iter().filter(|n| n.name_id == id).find_map(|n| n.to_string())
}

fn hex_utf16(c: char) -> String {
    let mut b = [0u16; 2];
    c.encode_utf16(&mut b).iter().map(|u| format!("{u:04X}")).collect()
}

fn to_unicode_cmap(used: &BTreeMap<u16, char>) -> String {
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<_> = used.iter().filter(|(g, _)| **g != 0).collect();
    for chunk in entries.chunks(100) {
        s.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (g, c) in chunk {
            s.push_str(&format!("<{g:04X}> <{}>\n", hex_utf16(**c)));
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s
}

fn add_font(doc: &mut Document, f: &CffFont) -> Result<ObjectId, String> {
    let face = ttf_parser::Face::parse(&f.data, 0).map_err(|e| e.to_string())?;
    let upm = face.units_per_em() as f32;
    let k = 1000.0 / upm;
    let ps = name_of(&face, ttf_parser::name_id::POST_SCRIPT_NAME).unwrap_or_else(|| "CFFFont".into());
    let ps: String = ps.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();

    // Font file (the whole OpenType file), flate-compressed.
    let mut fd = Dictionary::new();
    fd.set("Subtype", Object::Name(b"OpenType".to_vec()));
    let mut stream = Stream::new(fd, f.data.to_vec());
    stream.compress().map_err(|e| e.to_string())?;
    let file_id = doc.add_object(Object::Stream(stream));

    let bb = face.global_bounding_box();
    let mut desc = Dictionary::new();
    desc.set("Type", Object::Name(b"FontDescriptor".to_vec()));
    desc.set("FontName", Object::Name(ps.clone().into_bytes()));
    desc.set("Flags", Object::Integer(32 | if face.is_italic() { 64 } else { 0 }));
    desc.set(
        "FontBBox",
        Object::Array(
            [bb.x_min, bb.y_min, bb.x_max, bb.y_max].iter().map(|v| Object::Integer((*v as f32 * k).round() as i64)).collect(),
        ),
    );
    desc.set("ItalicAngle", Object::Real(face.italic_angle()));
    desc.set("Ascent", Object::Integer((face.ascender() as f32 * k).round() as i64));
    desc.set("Descent", Object::Integer((face.descender() as f32 * k).round() as i64));
    let cap = face.capital_height().unwrap_or(face.ascender());
    desc.set("CapHeight", Object::Integer((cap as f32 * k).round() as i64));
    desc.set("StemV", Object::Integer(80));
    desc.set("FontFile3", Object::Reference(file_id));
    let desc_id = doc.add_object(Object::Dictionary(desc));

    // Widths for the glyphs used: "gid [w]" pairs, in 1/1000 em.
    let mut w = Vec::new();
    for gid in f.used.keys() {
        let adv = face.glyph_hor_advance(ttf_parser::GlyphId(*gid)).unwrap_or(0) as f32 * k;
        w.push(Object::Integer(*gid as i64));
        w.push(Object::Array(vec![Object::Integer(adv.round() as i64)]));
    }
    let mut sys = Dictionary::new();
    sys.set("Registry", Object::String(b"Adobe".to_vec(), StringFormat::Literal));
    sys.set("Ordering", Object::String(b"Identity".to_vec(), StringFormat::Literal));
    sys.set("Supplement", Object::Integer(0));
    let mut cid = Dictionary::new();
    cid.set("Type", Object::Name(b"Font".to_vec()));
    cid.set("Subtype", Object::Name(b"CIDFontType0".to_vec()));
    cid.set("BaseFont", Object::Name(ps.clone().into_bytes()));
    cid.set("CIDSystemInfo", Object::Dictionary(sys));
    cid.set("FontDescriptor", Object::Reference(desc_id));
    cid.set("DW", Object::Integer(1000));
    cid.set("W", Object::Array(w));
    let cid_id = doc.add_object(Object::Dictionary(cid));

    let mut tu = Stream::new(Dictionary::new(), to_unicode_cmap(&f.used).into_bytes());
    tu.compress().map_err(|e| e.to_string())?;
    let tu_id = doc.add_object(Object::Stream(tu));

    let mut t0 = Dictionary::new();
    t0.set("Type", Object::Name(b"Font".to_vec()));
    t0.set("Subtype", Object::Name(b"Type0".to_vec()));
    t0.set("BaseFont", Object::Name(ps.into_bytes()));
    t0.set("Encoding", Object::Name(b"Identity-H".to_vec()));
    t0.set("DescendantFonts", Object::Array(vec![Object::Reference(cid_id)]));
    t0.set("ToUnicode", Object::Reference(tu_id));
    Ok(doc.add_object(Object::Dictionary(t0)))
}

/// Resolve `key` of `dict_id`'s dictionary to a mutable dictionary, creating it if absent.
fn child_dict<'a>(doc: &'a mut Document, owner: ObjectId, key: &[u8]) -> Result<&'a mut Dictionary, String> {
    enum Where {
        Inline,
        Ref(ObjectId),
        Missing,
    }
    let w = {
        let d = doc.get_dictionary(owner).map_err(|e| e.to_string())?;
        match d.get(key) {
            Ok(Object::Reference(id)) => Where::Ref(*id),
            Ok(Object::Dictionary(_)) => Where::Inline,
            _ => Where::Missing,
        }
    };
    match w {
        Where::Ref(id) => doc.get_object_mut(id).and_then(|o| o.as_dict_mut()).map_err(|e| e.to_string()),
        Where::Missing | Where::Inline => {
            let d = doc.get_object_mut(owner).and_then(|o| o.as_dict_mut()).map_err(|e| e.to_string())?;
            if matches!(w, Where::Missing) {
                d.set(key.to_vec(), Object::Dictionary(Dictionary::new()));
            }
            d.get_mut(key).and_then(|o| o.as_dict_mut()).map_err(|e| e.to_string())
        }
    }
}

/// Add the CFF fonts to every page's font resources of a printpdf-produced PDF.
pub fn add_to_pdf(pdf: &[u8], fonts: &[CffFont]) -> Result<Vec<u8>, String> {
    let mut doc = Document::load_mem(pdf).map_err(|e| e.to_string())?;
    let mut ids = Vec::new();
    for f in fonts {
        ids.push((f.res_name.clone(), add_font(&mut doc, f)?));
    }
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for page in pages {
        // Resources may be inline or an indirect object; Font likewise.
        let res_owner = match doc.get_dictionary(page).map_err(|e| e.to_string())?.get(b"Resources") {
            Ok(Object::Reference(id)) => *id,
            _ => page,
        };
        let res: &mut Dictionary = if res_owner == page {
            child_dict(&mut doc, page, b"Resources")?
        } else {
            doc.get_object_mut(res_owner).and_then(|o| o.as_dict_mut()).map_err(|e| e.to_string())?
        };
        // Font dict inside the resources dictionary.
        let font_ref = match res.get(b"Font") {
            Ok(Object::Reference(id)) => Some(*id),
            Ok(Object::Dictionary(_)) => None,
            _ => {
                res.set("Font", Object::Dictionary(Dictionary::new()));
                None
            }
        };
        let fd: &mut Dictionary = match font_ref {
            Some(id) => doc.get_object_mut(id).and_then(|o| o.as_dict_mut()).map_err(|e| e.to_string())?,
            None => {
                // re-borrow the resources dictionary
                let res: &mut Dictionary = if res_owner == page {
                    child_dict(&mut doc, page, b"Resources")?
                } else {
                    doc.get_object_mut(res_owner).and_then(|o| o.as_dict_mut()).map_err(|e| e.to_string())?
                };
                res.get_mut(b"Font").and_then(|o| o.as_dict_mut()).map_err(|e| e.to_string())?
            }
        };
        for (name, id) in &ids {
            fd.set(name.as_bytes().to_vec(), Object::Reference(*id));
        }
    }
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}
