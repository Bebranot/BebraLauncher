//! The changelog (CHANGELOG.md) is part of the launcher: it is shown on the updates tab, and the
//! tests below keep it honest, so a release cannot go out without its entry. The rules are in
//! docs/CHANGELOG_RULES.md.

/// CHANGELOG.md as written.
pub const TEXT: &str = include_str!("../../../CHANGELOG.md");

/// The heading of changes that are not in a release yet.
#[cfg(test)]
const UNRELEASED: &str = "## [Не выпущено]";

/// `## [0.2.000] — 2026-10-04` -> the version text and the date. A plain hyphen is accepted too,
/// so `## [0.2.000] - 2026-10-04` is not silently swallowed as part of the previous section.
#[cfg(test)]
fn heading(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("## [")?;
    let (version, rest) = rest.split_once(']')?;
    let rest = rest.trim();
    let date = rest
        .strip_prefix('—')
        .or_else(|| rest.strip_prefix('-'))
        .unwrap_or("")
        .trim();
    Some((version, date))
}

#[cfg(test)]
mod tests {
    use bebra_core::self_update::Version;

    use super::*;

    type Section<'a> = (&'a str, &'a str, Vec<&'a str>);

    /// Splits the changelog into sections. Lines inside fenced code blocks are not headings, and
    /// every `##` line that is not a version heading is reported so a test can refuse it.
    fn parse(text: &str) -> (Vec<Section<'_>>, Vec<&str>) {
        // (version, date, the lines of the section)
        let mut out: Vec<Section<'_>> = Vec::new();
        let mut unrecognized = Vec::new();
        let mut in_fence = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                if let Some(last) = out.last_mut() {
                    last.2.push(line);
                }
                continue;
            }
            if in_fence {
                if let Some(last) = out.last_mut() {
                    last.2.push(line);
                }
                continue;
            }
            if let Some((v, d)) = heading(line) {
                out.push((v, d, Vec::new()));
            } else {
                if line.starts_with("## ") {
                    unrecognized.push(line);
                }
                if let Some(last) = out.last_mut() {
                    last.2.push(line);
                }
            }
        }
        (out, unrecognized)
    }

    #[test]
    fn sections_ignore_code_fences_and_report_unknown_headings() {
        let text = "\
## [0.2.000] — 2026-10-04
- one
```md
## [9.9.9] — not a release
```
## [0.1.000] - 2026-01-01
- two
## Something else
text
";
        let (sections, unrecognized) = parse(text);
        assert_eq!(sections.len(), 2, "the fenced heading is not a section");
        assert_eq!(sections[0].0, "0.2.000");
        assert_eq!(
            sections[1].0, "0.1.000",
            "a plain hyphen is a date separator"
        );
        assert_eq!(sections[1].1, "2026-01-01");
        assert!(sections[0].2.iter().any(|l| l.contains("## [9.9.9]")));
        assert_eq!(unrecognized, ["## Something else"]);
    }

    #[test]
    fn the_changelog_has_no_unrecognized_headings() {
        let (_, unrecognized) = parse(TEXT);
        assert!(
            unrecognized.is_empty(),
            "these `##` lines are not version headings and would silently extend the previous \
             section: {unrecognized:?}"
        );
    }

    #[test]
    fn the_top_entry_is_the_version_of_this_build() {
        let top = parse(TEXT)
            .0
            .into_iter()
            .find(|(v, _, _)| Version::parse(v).is_some())
            .expect("CHANGELOG.md has no version entry");
        assert_eq!(
            top.0,
            Version::current().to_string(),
            "the newest entry of CHANGELOG.md must be the version of Cargo.toml: \
             write the entry (docs/CHANGELOG_RULES.md) or fix the version"
        );
    }

    #[test]
    fn entries_are_valid_newest_first_and_not_empty() {
        let mut previous: Option<Version> = None;
        let mut seen_release = false;
        for (v, date, body) in parse(TEXT).0 {
            if format!("## [{v}]") == UNRELEASED {
                assert!(
                    !seen_release,
                    "\"Не выпущено\" must stand above every release"
                );
                continue;
            }
            seen_release = true;
            let version =
                Version::parse(v).unwrap_or_else(|| panic!("{v:?} is not a version a.b.cdd"));
            assert_eq!(v, version.to_string(), "write {v} as {version}");
            if let Some(p) = previous {
                assert!(version < p, "{v} must be older than the entry above it");
            }
            previous = Some(version);

            let ok_date = date.len() == 10
                && date.bytes().enumerate().all(|(i, b)| {
                    if i == 4 || i == 7 {
                        b == b'-'
                    } else {
                        b.is_ascii_digit()
                    }
                });
            assert!(
                ok_date,
                "{v}: the heading needs a date `— YYYY-MM-DD` (a `-` is accepted too), got {date:?}"
            );
            assert!(
                body.iter().any(|l| l.trim_start().starts_with("- ")),
                "{v}: an entry without a single `- ` line"
            );
        }
        assert!(seen_release);
    }

    #[test]
    fn every_place_that_holds_the_version_agrees() {
        let own = env!("CARGO_PKG_VERSION");
        let field = |json: &str| -> String {
            let value: serde_json::Value = serde_json::from_str(json).unwrap();
            value["version"].as_str().unwrap().to_string()
        };
        assert_eq!(
            field(include_str!("../tauri.conf.json")),
            own,
            "tauri.conf.json"
        );
        assert_eq!(
            field(include_str!("../../package.json")),
            own,
            "app/package.json"
        );
    }
}
