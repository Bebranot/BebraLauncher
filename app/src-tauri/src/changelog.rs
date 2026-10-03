//! The changelog (CHANGELOG.md) is part of the launcher: it is shown on the updates tab, and the
//! tests below keep it honest, so a release cannot go out without its entry. The rules are in
//! docs/CHANGELOG_RULES.md.

/// CHANGELOG.md as written.
pub const TEXT: &str = include_str!("../../../CHANGELOG.md");

/// The heading of changes that are not in a release yet.
#[cfg(test)]
const UNRELEASED: &str = "## [Не выпущено]";

/// `## [0.2.000] — 2026-10-04` -> the version text and the date.
#[cfg(test)]
fn heading(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("## [")?;
    let (version, rest) = rest.split_once(']')?;
    let date = rest.trim().strip_prefix('—').unwrap_or("").trim();
    Some((version, date))
}

#[cfg(test)]
mod tests {
    use bebra_core::self_update::Version;

    use super::*;

    fn sections(text: &str) -> Vec<(&str, &str, Vec<&str>)> {
        // (version, date, the lines of the section)
        let mut out: Vec<(&str, &str, Vec<&str>)> = Vec::new();
        for line in text.lines() {
            if let Some((v, d)) = heading(line) {
                out.push((v, d, Vec::new()));
            } else if let Some(last) = out.last_mut() {
                last.2.push(line);
            }
        }
        out
    }

    #[test]
    fn the_top_entry_is_the_version_of_this_build() {
        let top = sections(TEXT)
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
        for (v, date, body) in sections(TEXT) {
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
                "{v}: the heading needs a date `— YYYY-MM-DD`, got {date:?}"
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
