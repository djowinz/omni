//! Fork an installed bundle into a new local overlay.
//!
//! Sub-spec #013. Reads from `bundles/<slug>/` (produced by #010's install
//! pipeline), writes to `overlays/<name>/`, atomically via
//! `workspace::atomic_dir`. Records heritage in `.omni-origin.json`.

#![allow(dead_code)] // types wired in Wave 2+

/// Windows reserved filename stems, uppercase. Match is case-insensitive and
/// applies whether or not the name carries an extension (per Win32 rules).
const WINDOWS_RESERVED_STEMS: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Validate a user-chosen overlay name. Rejects per sub-spec #013 §4.
pub(crate) fn sanitize_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("name must not be empty");
    }
    if name.len() > 48 {
        return Err("name exceeds 48 characters");
    }
    if name != name.trim() {
        return Err("name must not have leading or trailing whitespace");
    }
    if name == "." || name == ".." {
        return Err("name must not be '.' or '..'");
    }
    for ch in name.chars() {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => {
                return Err("name contains a forbidden character");
            }
            c if c.is_control() => {
                return Err("name contains a non-printable character");
            }
            _ => {}
        }
    }
    // Windows reserved stems: compare the part before the first '.' (if any),
    // case-insensitive.
    let stem = name.split('.').next().unwrap_or(name);
    let stem_upper = stem.to_ascii_uppercase();
    if WINDOWS_RESERVED_STEMS.iter().any(|r| *r == stem_upper) {
        return Err("name is a Windows reserved stem");
    }
    Ok(())
}

#[cfg(test)]
mod sanitize_tests {
    use super::sanitize_name;

    #[test]
    fn accepts_reasonable_names() {
        for good in ["my-hud", "Cyberpunk HUD", "a", "with_underscore",
                     "unicode-Ω-ok", "digits-123", "dot.in.middle"] {
            assert!(sanitize_name(good).is_ok(), "expected ok: {good:?}");
        }
    }

    #[test]
    fn rejects_empty_and_length_bounds() {
        assert!(sanitize_name("").is_err());
        let long = "x".repeat(49);
        assert!(sanitize_name(&long).is_err());
        let ok48 = "x".repeat(48);
        assert!(sanitize_name(&ok48).is_ok());
    }

    #[test]
    fn rejects_whitespace_edges() {
        for bad in [" leading", "trailing ", " both ", "\ttab\t"] {
            assert!(sanitize_name(bad).is_err(), "expected err: {bad:?}");
        }
    }

    #[test]
    fn rejects_dot_dotdot() {
        assert!(sanitize_name(".").is_err());
        assert!(sanitize_name("..").is_err());
    }

    #[test]
    fn rejects_path_traversal_and_separators() {
        for bad in ["../evil", "foo/bar", "foo\\bar", "/abs", "\\abs",
                    "c:name", "ads:stream"] {
            assert!(sanitize_name(bad).is_err(), "expected err: {bad:?}");
        }
    }

    #[test]
    fn rejects_forbidden_chars() {
        for bad in ["star*name", "q?mark", "quo\"te", "less<than",
                    "greater>than", "pipe|name"] {
            assert!(sanitize_name(bad).is_err(), "expected err: {bad:?}");
        }
    }

    #[test]
    fn rejects_null_and_control_bytes() {
        assert!(sanitize_name("nul\0byte").is_err());
        assert!(sanitize_name("bell\x07").is_err());
        assert!(sanitize_name("newline\nhere").is_err());
    }

    #[test]
    fn rejects_all_windows_reserved_stems_all_case_variants_and_with_ext() {
        let bases = [
            "CON", "PRN", "AUX", "NUL",
            "COM0", "COM1", "COM2", "COM3", "COM4",
            "COM5", "COM6", "COM7", "COM8", "COM9",
            "LPT0", "LPT1", "LPT2", "LPT3", "LPT4",
            "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ];
        let case_variants = |s: &str| -> Vec<String> {
            vec![
                s.to_ascii_uppercase(),
                s.to_ascii_lowercase(),
                {
                    let mut c = s.chars();
                    match c.next() {
                        Some(first) => format!("{}{}",
                            first.to_ascii_uppercase(),
                            c.as_str().to_ascii_lowercase()),
                        None => String::new(),
                    }
                },
                s.chars().enumerate().map(|(i, c)| {
                    if i % 2 == 0 { c.to_ascii_lowercase() }
                    else { c.to_ascii_uppercase() }
                }).collect(),
            ]
        };
        for base in bases {
            for v in case_variants(base) {
                assert!(sanitize_name(&v).is_err(),
                    "expected err for reserved stem {v:?}");
                for ext in [".txt", ".omni", ".json"] {
                    let with_ext = format!("{v}{ext}");
                    assert!(sanitize_name(&with_ext).is_err(),
                        "expected err for reserved+ext {with_ext:?}");
                }
            }
        }
    }

    #[test]
    fn allows_reserved_stem_as_substring_but_not_as_stem() {
        assert!(sanitize_name("console").is_ok());
        assert!(sanitize_name("comic").is_ok());
        assert!(sanitize_name("lptop").is_ok());
        assert!(sanitize_name("con.anything").is_err());
    }
}
