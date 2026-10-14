//! Event courses and positional menu inference.
//!
//! A guest's menu is still a list of entries ("Starter: Soup"). When a CSV (or the
//! interactive CLI) gives unlabelled entries ("Soup; Beef; Tart") they are assigned to
//! the event's courses by position, so nobody has to type the course names.

/// Standard course order; names matching these keep this order when inferred from labels.
pub const STANDARD: [&str; 6] = ["Canap\u{e9}s", "Starter", "Soup", "Fish", "Main", "Dessert"];
pub const MAX_COURSES: usize = 6;

/// Default course names for an event with `n` courses (clamped to 1..=6).
pub fn default_names(n: usize) -> Vec<String> {
    let v: &[&str] = match n.clamp(1, MAX_COURSES) {
        1 => &["Main"],
        2 => &["Main", "Dessert"],
        3 => &["Starter", "Main", "Dessert"],
        4 => &["Starter", "Soup", "Main", "Dessert"],
        5 => &["Starter", "Soup", "Fish", "Main", "Dessert"],
        _ => &["Canap\u{e9}s", "Starter", "Soup", "Fish", "Main", "Dessert"],
    };
    v.iter().map(|s| s.to_string()).collect()
}

fn fold(s: &str) -> String {
    s.trim().to_lowercase().replace('\u{e9}', "e")
}

/// Position of `name` in the standard order (case-insensitive; "Canapes" works too).
pub fn std_index(name: &str) -> Option<usize> {
    let n = fold(name);
    STANDARD.iter().position(|s| fold(s) == n)
}

pub fn same_name(a: &str, b: &str) -> bool {
    fold(a) == fold(b)
}

/// Split a menu field on ';' keeping empty slots (they mark skipped courses);
/// trailing empty slots are dropped.
pub fn split_slots(s: &str) -> Vec<String> {
    let mut v: Vec<String> = s.split(';').map(|p| p.trim().to_string()).collect();
    while v.last().map(|l| l.is_empty()).unwrap_or(false) {
        v.pop();
    }
    v
}

/// "Main: Beef" -> ("Main", "Beef"). Returns the label even if the dish is empty.
pub fn split_label(entry: &str) -> Option<(String, String)> {
    let (l, d) = entry.split_once(':')?;
    let l = l.trim();
    (!l.is_empty()).then(|| (l.to_string(), d.trim().to_string()))
}

/// Result of assigning one row.
pub struct RowAssign {
    /// Labelled entries in the original slot order ("Starter: Soup", ...); overflow entries stay unlabelled.
    pub entries: Vec<String>,
    /// Unlabelled entries that had no course left (they stay in `entries`, unlabelled, i.e. "Other").
    pub overflow: Vec<String>,
}

/// Assign the slots of one row to `courses`: labelled entries go to their course (case-insensitive),
/// unlabelled ones fill the remaining courses in order (empty slots skip a course); extras overflow.
pub fn assign_row(slots: &[String], courses: &[String]) -> RowAssign {
    let course_of = |label: &str| courses.iter().position(|c| same_name(c, label));
    let mut claimed = vec![false; courses.len()];
    for s in slots {
        if let Some((l, _)) = split_label(s) {
            if let Some(i) = course_of(&l) {
                claimed[i] = true;
            }
        }
    }
    let remaining: Vec<usize> = (0..courses.len()).filter(|i| !claimed[*i]).collect();
    let mut pos = 0;
    let mut entries = Vec::new();
    let mut overflow = Vec::new();
    for s in slots {
        if let Some((l, d)) = split_label(s) {
            if d.is_empty() {
                continue; // "Main:" with no dish
            }
            match course_of(&l) {
                Some(i) => entries.push(format!("{}: {}", courses[i], d)),
                None => entries.push(format!("{l}: {d}")),
            }
        } else if s.is_empty() {
            pos += 1; // skipped course
        } else if pos < remaining.len() {
            entries.push(format!("{}: {}", courses[remaining[pos]], s));
            pos += 1;
        } else {
            entries.push(s.clone());
            overflow.push(s.clone());
        }
    }
    RowAssign { entries, overflow }
}

/// Courses for an import. With `base` (the user's existing courses) they are kept and unknown labels
/// appended; without it they are inferred from the labels (standard order for known names) or, if
/// there are none, from the largest number of entries in any row.
pub fn infer_courses(rows: &[Vec<String>], base: Option<&[String]>) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for r in rows {
        for s in r {
            if let Some((l, _)) = split_label(s) {
                if !labels.iter().any(|x| same_name(x, &l)) {
                    labels.push(l);
                }
            }
        }
    }
    if let Some(base) = base {
        let mut c = base.to_vec();
        for l in labels {
            if !c.iter().any(|x| same_name(x, &l)) {
                c.push(l);
            }
        }
        return c;
    }
    // Rows with any unlabelled slot are positional, so their length counts towards the course total
    // even when other rows (or other entries in the same row) carry labels.
    let positional = rows
        .iter()
        .filter(|r| r.iter().any(|s| split_label(s).is_none()))
        .map(|r| r.len())
        .max()
        .unwrap_or(0);
    if labels.is_empty() {
        return default_names(if positional == 0 { 3 } else { positional });
    }
    let mut names = if positional > 0 { default_names(positional) } else { Vec::new() };
    for l in labels {
        if !names.iter().any(|x| same_name(x, &l)) {
            names.push(l);
        }
    }
    let mut std: Vec<(usize, String)> = Vec::new();
    let mut custom = Vec::new();
    for l in names {
        match std_index(&l) {
            Some(i) => std.push((i, STANDARD[i].to_string())),
            None => custom.push(l),
        }
    }
    std.sort_by_key(|(i, _)| *i);
    // standard spelling is canonical for known names
    let mut c: Vec<String> = std.into_iter().map(|(_, n)| n).collect();
    c.extend(custom);
    c
}

/// Parse `--courses`: a count ("3") or a comma list ("Starter,Main,Dessert").
pub fn parse_courses_arg(s: &str) -> Result<Vec<String>, String> {
    let s = s.trim();
    if let Ok(n) = s.parse::<usize>() {
        if (1..=MAX_COURSES).contains(&n) {
            return Ok(default_names(n));
        }
        return Err(format!("--courses count must be 1 to {MAX_COURSES}"));
    }
    let v: Vec<String> = s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    if v.is_empty() {
        Err("--courses needs a count or a comma-separated list of names".into())
    } else {
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(n: usize) -> Vec<String> {
        default_names(n)
    }
    fn run(row: &str, courses: &[String]) -> Vec<String> {
        assign_row(&split_slots(row), courses).entries
    }

    #[test]
    fn default_course_names() {
        assert_eq!(c(1), ["Main"]);
        assert_eq!(c(2), ["Main", "Dessert"]);
        assert_eq!(c(3), ["Starter", "Main", "Dessert"]);
        assert_eq!(c(4), ["Starter", "Soup", "Main", "Dessert"]);
        assert_eq!(c(5), ["Starter", "Soup", "Fish", "Main", "Dessert"]);
        assert_eq!(c(6), ["Canap\u{e9}s", "Starter", "Soup", "Fish", "Main", "Dessert"]);
    }

    #[test]
    fn positional_inference_for_each_count() {
        for n in 1..=6 {
            let names = c(n);
            let row = (0..n).map(|i| format!("D{i}")).collect::<Vec<_>>().join("; ");
            let got = run(&row, &names);
            let want: Vec<String> = names.iter().enumerate().map(|(i, nm)| format!("{nm}: D{i}")).collect();
            assert_eq!(got, want, "{n} courses");
        }
    }

    #[test]
    fn skipped_courses_and_trailing_omission() {
        let three = c(3);
        assert_eq!(run("Soup; Beef; Tart", &three), ["Starter: Soup", "Main: Beef", "Dessert: Tart"]);
        assert_eq!(run("; Beef; Tart", &three), ["Main: Beef", "Dessert: Tart"]);
        assert_eq!(run("Soup;;Tart", &three), ["Starter: Soup", "Dessert: Tart"]);
        assert_eq!(run("Soup; Chicken", &three), ["Starter: Soup", "Main: Chicken"]);
        assert_eq!(run("", &three), Vec::<String>::new());
        assert_eq!(run("Soup; ; ", &three), ["Starter: Soup"]); // trailing empties dropped
    }

    #[test]
    fn labelled_mixed_unknown_and_overflow() {
        let three = c(3);
        // labelled entries keep their course (case-insensitive) and original order
        assert_eq!(run("main: Beef; Starter: Soup", &three), ["Main: Beef", "Starter: Soup"]);
        // mixed: labelled claim their course, unlabelled fill the remaining courses in order
        assert_eq!(run("Main: Beef; Soup; Tart", &three), ["Main: Beef", "Starter: Soup", "Dessert: Tart"]);
        // unknown label stays as given (the import adds it as a new course)
        assert_eq!(run("Cheese: Brie; Soup", &three), ["Cheese: Brie", "Starter: Soup"]);
        // overflow: more unlabelled entries than courses
        let r = assign_row(&split_slots("A; B; C; D; E"), &three);
        assert_eq!(r.overflow, ["D", "E"]);
        assert_eq!(r.entries, ["Starter: A", "Main: B", "Dessert: C", "D", "E"]);
        // an explicit "Main:" with no dish claims the course but adds nothing
        assert_eq!(run("Main:; Soup", &three), ["Starter: Soup"]);
    }

    #[test]
    fn course_count_inference() {
        let rows = |v: &[&str]| v.iter().map(|r| split_slots(r)).collect::<Vec<_>>();
        // from labels: standard order for known names, others after, first-seen
        assert_eq!(
            infer_courses(&rows(&["Dessert: Tart; Main: Beef", "Starter: Soup; Cheese: Brie"]), None),
            ["Starter", "Main", "Dessert", "Cheese"]
        );
        assert_eq!(infer_courses(&rows(&["canapes: x; fish: y; SOUP: z"]), None), ["Canap\u{e9}s", "Soup", "Fish"]);
        // from the largest row when unlabelled
        assert_eq!(infer_courses(&rows(&["Soup; Beef", "A; B; C; D"]), None), c(4));
        assert_eq!(infer_courses(&rows(&["Soup"]), None), c(1));
        assert_eq!(infer_courses(&rows(&["", ""]), None), c(3)); // nothing known: 3 courses
        assert_eq!(infer_courses(&rows(&["1;2;3;4;5;6;7;8"]), None), c(6)); // clamped
        // mixed file: one labelled entry must not shrink the course count of positional rows
        assert_eq!(infer_courses(&rows(&["Soup; Beef; Tart", "Main: Fish; Soup"]), None), c(3));
        assert_eq!(infer_courses(&rows(&["Soup; Beef", "Cheese: Brie"]), None), ["Main", "Dessert", "Cheese"]);
        // with the user's courses: kept; only new labels appended
        let base = vec!["Apps".to_string(), "Mains".to_string()];
        assert_eq!(infer_courses(&rows(&["Apps: x; Pud: y"]), Some(&base)), ["Apps", "Mains", "Pud"]);
        assert_eq!(infer_courses(&rows(&["a; b; c"]), Some(&base)), ["Apps", "Mains"]);
    }

    #[test]
    fn courses_arg() {
        assert_eq!(parse_courses_arg("3").unwrap(), c(3));
        assert_eq!(parse_courses_arg("Starter, Main ,Dessert").unwrap(), c(3));
        assert!(parse_courses_arg("9").is_err() && parse_courses_arg(" , ").is_err());
    }
}
