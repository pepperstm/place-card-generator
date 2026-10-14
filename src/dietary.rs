//! Dietary requirements: the 14 major allergens and common dietary preferences as
//! fixed checkbox options, plus free-text "Other" entries.
//!
//! A guest still stores one plain list of strings (so CSV and everything else keep
//! working); `Dietary` is the typed view. Entries are mapped to a checkbox only when
//! they clearly match; ambiguous ones ("Nut allergy", "Shellfish allergy") stay in Other.

/// (short canonical label, full legal name)
pub const ALLERGENS: [(&str, &str); 14] = [
    ("Celery", "Celery (and celeriac)"),
    ("Gluten", "Cereals containing gluten"),
    ("Crustaceans", "Crustaceans"),
    ("Eggs", "Eggs"),
    ("Fish", "Fish"),
    ("Lupin", "Lupin"),
    ("Milk", "Milk"),
    ("Molluscs", "Molluscs"),
    ("Mustard", "Mustard"),
    ("Tree nuts", "Nuts (tree nuts)"),
    ("Peanuts", "Peanuts"),
    ("Sesame", "Sesame seeds"),
    ("Soya", "Soya (soybeans)"),
    ("Sulphites", "Sulphur dioxide and sulphites"),
];

pub const PREFS: [&str; 5] = ["Vegetarian", "Vegan", "Pescatarian", "Halal", "Kosher"];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Allergen(usize),
    Pref(usize),
}

/// Lower-case aliases that clearly mean one checkbox. Anything not listed is "Other".
const ALIASES: &[(&str, Item)] = &[
    ("celery", Item::Allergen(0)),
    ("celery allergy", Item::Allergen(0)),
    ("celeriac", Item::Allergen(0)),
    ("celery (and celeriac)", Item::Allergen(0)),
    ("gluten", Item::Allergen(1)),
    ("cereals containing gluten", Item::Allergen(1)),
    ("gluten free", Item::Allergen(1)),
    ("gluten allergy", Item::Allergen(1)),
    ("coeliac", Item::Allergen(1)),
    ("celiac", Item::Allergen(1)),
    ("coeliac disease", Item::Allergen(1)),
    ("crustaceans", Item::Allergen(2)),
    ("crustacean", Item::Allergen(2)),
    ("crustacean allergy", Item::Allergen(2)),
    ("crustaceans allergy", Item::Allergen(2)),
    ("eggs", Item::Allergen(3)),
    ("egg", Item::Allergen(3)),
    ("egg allergy", Item::Allergen(3)),
    ("eggs allergy", Item::Allergen(3)),
    ("egg free", Item::Allergen(3)),
    ("fish", Item::Allergen(4)),
    ("fish allergy", Item::Allergen(4)),
    ("lupin", Item::Allergen(5)),
    ("lupine", Item::Allergen(5)),
    ("lupin allergy", Item::Allergen(5)),
    ("milk", Item::Allergen(6)),
    ("milk allergy", Item::Allergen(6)),
    ("dairy", Item::Allergen(6)),
    ("dairy free", Item::Allergen(6)),
    ("dairy allergy", Item::Allergen(6)),
    ("molluscs", Item::Allergen(7)),
    ("mollusc", Item::Allergen(7)),
    ("mollusks", Item::Allergen(7)),
    ("mollusk", Item::Allergen(7)),
    ("mollusc allergy", Item::Allergen(7)),
    ("molluscs allergy", Item::Allergen(7)),
    ("mustard", Item::Allergen(8)),
    ("mustard allergy", Item::Allergen(8)),
    ("tree nuts", Item::Allergen(9)),
    ("tree nut", Item::Allergen(9)),
    ("tree nut allergy", Item::Allergen(9)),
    ("tree nuts allergy", Item::Allergen(9)),
    ("nuts (tree nuts)", Item::Allergen(9)),
    ("peanuts", Item::Allergen(10)),
    ("peanut", Item::Allergen(10)),
    ("peanut allergy", Item::Allergen(10)),
    ("peanuts allergy", Item::Allergen(10)),
    ("groundnuts", Item::Allergen(10)),
    ("groundnut", Item::Allergen(10)),
    ("sesame", Item::Allergen(11)),
    ("sesame seeds", Item::Allergen(11)),
    ("sesame seed", Item::Allergen(11)),
    ("sesame allergy", Item::Allergen(11)),
    ("soya", Item::Allergen(12)),
    ("soy", Item::Allergen(12)),
    ("soya allergy", Item::Allergen(12)),
    ("soy allergy", Item::Allergen(12)),
    ("soya (soybeans)", Item::Allergen(12)),
    ("soybeans", Item::Allergen(12)),
    ("sulphites", Item::Allergen(13)),
    ("sulphite", Item::Allergen(13)),
    ("sulfites", Item::Allergen(13)),
    ("sulfite", Item::Allergen(13)),
    ("sulphur dioxide", Item::Allergen(13)),
    ("sulfur dioxide", Item::Allergen(13)),
    ("sulphur dioxide and sulphites", Item::Allergen(13)),
    ("sulphites allergy", Item::Allergen(13)),
    ("vegetarian", Item::Pref(0)),
    ("vegan", Item::Pref(1)),
    ("pescatarian", Item::Pref(2)),
    ("pescetarian", Item::Pref(2)),
    ("halal", Item::Pref(3)),
    ("kosher", Item::Pref(4)),
];

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase().replace('-', " ")
}

/// The checkbox an entry clearly refers to, if any. Ambiguous entries return None.
pub fn classify(entry: &str) -> Option<Item> {
    let n = norm(entry);
    ALIASES.iter().find(|(a, _)| *a == n).map(|(_, i)| *i)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dietary {
    pub allergens: [bool; 14],
    pub prefs: [bool; 5],
    pub other: Vec<String>,
}

impl Dietary {
    /// Build from plain entries (CSV, old data). Matches go to checkboxes, the
    /// rest stay in Other unchanged (duplicates merged case-insensitively).
    pub fn from_entries(entries: &[String]) -> Dietary {
        let mut d = Dietary::default();
        for e in entries.iter().map(|e| e.trim()).filter(|e| !e.is_empty()) {
            match classify(e) {
                Some(Item::Allergen(i)) => d.allergens[i] = true,
                Some(Item::Pref(i)) => d.prefs[i] = true,
                None => {
                    if !d.other.iter().any(|o| o.eq_ignore_ascii_case(e)) {
                        d.other.push(e.to_string());
                    }
                }
            }
        }
        d
    }

    /// Canonical entries: allergens in fixed order, then preferences, then Other.
    pub fn to_entries(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.extend(ALLERGENS.iter().enumerate().filter(|(i, _)| self.allergens[*i]).map(|(_, (l, _))| l.to_string()));
        v.extend(PREFS.iter().enumerate().filter(|(i, _)| self.prefs[*i]).map(|(_, l)| l.to_string()));
        v.extend(self.other.iter().map(|o| o.trim().to_string()).filter(|o| !o.is_empty()));
        v
    }

    pub fn is_empty(&self) -> bool {
        !self.allergens.iter().any(|b| *b) && !self.prefs.iter().any(|b| *b) && self.other.iter().all(|o| o.trim().is_empty())
    }

    pub fn allergen_labels(&self) -> Vec<&'static str> {
        ALLERGENS.iter().enumerate().filter(|(i, _)| self.allergens[*i]).map(|(_, (l, _))| *l).collect()
    }

    pub fn pref_labels(&self) -> Vec<&'static str> {
        PREFS.iter().enumerate().filter(|(i, _)| self.prefs[*i]).map(|(_, l)| *l).collect()
    }

    /// "Allergens: Milk, Peanuts · Diet: Vegan · Other: no coriander" (empty parts omitted).
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        let a = self.allergen_labels();
        if !a.is_empty() {
            parts.push(format!("Allergens: {}", a.join(", ")));
        }
        let p = self.pref_labels();
        if !p.is_empty() {
            parts.push(format!("Diet: {}", p.join(", ")));
        }
        let o: Vec<&str> = self.other.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        if !o.is_empty() {
            parts.push(format!("Other: {}", o.join(", ")));
        }
        parts.join(" \u{b7} ")
    }

    /// Compact form for table cells: "Milk, Peanuts · Vegan · Other: no coriander", or "(none)".
    pub fn summary(&self) -> String {
        if self.is_empty() {
            return "(none)".into();
        }
        let mut parts = Vec::new();
        let a = self.allergen_labels();
        if !a.is_empty() {
            parts.push(a.join(", "));
        }
        let p = self.pref_labels();
        if !p.is_empty() {
            parts.push(p.join(", "));
        }
        let o: Vec<&str> = self.other.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        if !o.is_empty() {
            parts.push(format!("Other: {}", o.join(", ")));
        }
        parts.join(" \u{b7} ")
    }

    /// Order-independent key for grouping identical requirement sets.
    pub fn key(&self) -> String {
        let mut o: Vec<String> = self.other.iter().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
        o.sort();
        format!("{:?}|{:?}|{}", self.allergens, self.prefs, o.join("\u{1}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn aliases_map_to_checkboxes() {
        let cases = [
            ("Milk allergy", Item::Allergen(6)),
            ("PEANUT ALLERGY", Item::Allergen(10)),
            ("Egg allergy", Item::Allergen(3)),
            ("Eggs allergy", Item::Allergen(3)),
            ("Gluten free", Item::Allergen(1)),
            ("gluten-free", Item::Allergen(1)),
            ("Coeliac", Item::Allergen(1)),
            ("Soy", Item::Allergen(12)),
            ("Soya", Item::Allergen(12)),
            ("Sulfites", Item::Allergen(13)),
            ("Sulphur dioxide", Item::Allergen(13)),
            ("Sesame seeds", Item::Allergen(11)),
            ("Dairy", Item::Allergen(6)),
            ("Dairy free", Item::Allergen(6)),
            ("Nuts (tree nuts)", Item::Allergen(9)),
            ("Cereals containing gluten", Item::Allergen(1)),
            ("  vegan ", Item::Pref(1)),
            ("Pescetarian", Item::Pref(2)),
            ("Kosher", Item::Pref(4)),
        ];
        for (s, want) in cases {
            assert_eq!(classify(s), Some(want), "{s}");
        }
        // every canonical label and full legal name maps back to itself
        for (i, (label, full)) in ALLERGENS.iter().enumerate() {
            assert_eq!(classify(label), Some(Item::Allergen(i)), "{label}");
            assert_eq!(classify(full), Some(Item::Allergen(i)), "{full}");
        }
        for (i, p) in PREFS.iter().enumerate() {
            assert_eq!(classify(p), Some(Item::Pref(i)));
        }
    }

    #[test]
    fn ambiguous_entries_stay_in_other_unchanged() {
        for s in ["Nut allergy", "nuts", "Shellfish allergy", "Shellfish", "no coriander", "Allergic to pears"] {
            assert_eq!(classify(s), None, "{s}");
        }
        let d = Dietary::from_entries(&e(&["Nut allergy", "Shellfish allergy", "Milk"]));
        assert!(d.allergens[6]);
        assert_eq!(d.other, vec!["Nut allergy", "Shellfish allergy"]);
    }

    #[test]
    fn canonical_order_and_describe() {
        let d = Dietary::from_entries(&e(&["Vegan", "no coriander", "Peanuts", "milk allergy", "Gluten free", "NO CORIANDER"]));
        assert_eq!(d.to_entries(), e(&["Gluten", "Milk", "Peanuts", "Vegan", "no coriander"]));
        assert_eq!(d.describe(), "Allergens: Gluten, Milk, Peanuts \u{b7} Diet: Vegan \u{b7} Other: no coriander");
        assert_eq!(d.summary(), "Gluten, Milk, Peanuts \u{b7} Vegan \u{b7} Other: no coriander");
        assert_eq!(Dietary::default().summary(), "(none)");
        // round trip is stable
        assert_eq!(Dietary::from_entries(&d.to_entries()), d);
        // the key ignores order and case
        let a = Dietary::from_entries(&e(&["Peanuts", "Vegan", "Zed", "alpha"]));
        let b = Dietary::from_entries(&e(&["vegan", "ALPHA", "peanut allergy", "zed"]));
        assert_eq!(a.key(), b.key());
    }
}
