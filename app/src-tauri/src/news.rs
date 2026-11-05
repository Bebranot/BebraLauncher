//! The news tab: titles and links from the Space Station 14 blog feed, like the official launcher,
//! and the launcher's own news, a JSON file in its repository (`news/news.json`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Where the official launcher reads its news from.
pub const FEED_URL: &str = "https://spacestation14.com/post/index.xml";

/// Where the launcher's own news lives in the repository.
pub const OWN_PATH: &str = "news/news.json";
/// Far more than a list of [`MAX_ITEMS`] short notes.
pub const OWN_MAX_BYTES: usize = 512 * 1024;

/// Most entries shown.
pub const MAX_ITEMS: usize = 30;

/// One of the launcher's own news: the texts are per language, the interface picks one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnNewsItem {
    pub id: String,
    /// `YYYY-MM-DD`; empty if the file has something else.
    #[serde(default)]
    pub date: String,
    pub title: BTreeMap<String, String>,
    #[serde(default)]
    pub body: BTreeMap<String, String>,
    #[serde(default)]
    pub link: Option<String>,
}

#[derive(Deserialize)]
struct OwnNewsFile {
    items: Vec<serde_json::Value>,
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

/// Reads the news file, newest first. A broken entry is left out and the rest are still shown;
/// only a file that is not this format at all is an error. Links that cannot be opened safely are
/// dropped: the file is plain text from the network, whoever wrote it.
pub fn parse_own(text: &str) -> Result<Vec<OwnNewsItem>, String> {
    let file: OwnNewsFile = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("news.json: {e}"))?;
    let mut items: Vec<OwnNewsItem> = file
        .items
        .into_iter()
        .filter_map(|value| serde_json::from_value::<OwnNewsItem>(value).ok())
        .filter_map(|mut item| {
            item.title.retain(|_, v| !v.trim().is_empty());
            item.body.retain(|_, v| !v.trim().is_empty());
            if item.id.trim().is_empty() || item.title.is_empty() {
                return None;
            }
            if !is_iso_date(&item.date) {
                item.date.clear();
            }
            item.link = item
                .link
                .filter(|l| l.starts_with("https://") && crate::system::is_safe_web_url(l));
            Some(item)
        })
        .collect();
    items.sort_by(|a, b| b.date.cmp(&a.date));
    items.truncate(MAX_ITEMS);
    Ok(items)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewsItem {
    pub title: String,
    pub link: String,
    /// As written in the feed (RFC 822); empty if the entry has none.
    pub date: String,
}

fn unescape(s: &str) -> String {
    let s = s.trim();
    // CDATA sections hold the text as it is.
    if let Some(inner) = s
        .strip_prefix("<![CDATA[")
        .and_then(|r| r.strip_suffix("]]>"))
    {
        return inner.trim().to_string();
    }
    // One pass over every entity: named ones (`&amp;`) and numeric ones (`&#8217;`, `&#x2019;`).
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        // An entity is short; anything longer is a stray `&`.
        let Some(semi) = tail
            .char_indices()
            .take(12)
            .find(|(_, c)| *c == ';')
            .map(|(j, _)| j)
        else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        match decode_entity(&tail[1..semi]) {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn decode_entity(entity: &str) -> Option<char> {
    Some(match entity {
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "amp" => '&',
        "nbsp" => '\u{a0}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "hellip" => '\u{2026}',
        _ => return decode_numeric_entity(entity),
    })
}

/// `#8217` or `#x2019`.
fn decode_numeric_entity(entity: &str) -> Option<char> {
    let number = entity.strip_prefix('#')?;
    let code = match number.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => number.parse::<u32>().ok()?,
    };
    char::from_u32(code).filter(|c| !c.is_control())
}

/// The text between `<tag>` and `</tag>` in `block`, if the tag is there.
fn tag_text<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    Some(&block[start..end])
}

/// Reads the entries of an RSS feed. Entries without a title or without an `https` link are
/// left out: the link is opened in the browser when clicked.
pub fn parse_feed(xml: &str) -> Vec<NewsItem> {
    let mut items = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<item>") {
        let after = &rest[start + "<item>".len()..];
        let Some(end) = after.find("</item>") else {
            break;
        };
        let block = &after[..end];
        rest = &after[end + "</item>".len()..];

        let title = tag_text(block, "title").map(unescape).unwrap_or_default();
        let link = tag_text(block, "link").map(unescape).unwrap_or_default();
        let date = tag_text(block, "pubDate").map(unescape).unwrap_or_default();
        if title.is_empty()
            || !link.starts_with("https://")
            || !crate::system::is_safe_web_url(&link)
        {
            continue;
        }
        items.push(NewsItem { title, link, date });
        if items.len() >= MAX_ITEMS {
            break;
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"<?xml version="1.0"?>
<rss version="2.0"><channel><title>Space Station 14</title><link>https://spacestation14.com/</link>
<item>
  <title>Patch &amp; fixes: 1.2</title>
  <link>https://spacestation14.com/post/patch-1-2/</link>
  <pubDate>Fri, 03 Oct 2026 10:00:00 +0000</pubDate>
</item>
<item>
  <title><![CDATA[Новости <b>октября</b>]]></title>
  <link>https://spacestation14.com/post/october/</link>
</item>
<item>
  <title>Unsafe link</title>
  <link>javascript:alert(1)</link>
</item>
<item>
  <title>Plain http</title>
  <link>http://spacestation14.com/post/http/</link>
</item>
<item>
  <title></title>
  <link>https://spacestation14.com/post/untitled/</link>
</item>
</channel></rss>"#;

    #[test]
    fn reads_titles_links_and_dates() {
        let items = parse_feed(FEED);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Patch & fixes: 1.2");
        assert_eq!(items[0].link, "https://spacestation14.com/post/patch-1-2/");
        assert_eq!(items[0].date, "Fri, 03 Oct 2026 10:00:00 +0000");
        assert_eq!(items[1].title, "Новости <b>октября</b>");
        assert_eq!(items[1].date, "");
    }

    #[test]
    fn leaves_out_entries_that_cannot_be_opened_safely() {
        let titles: Vec<String> = parse_feed(FEED).into_iter().map(|i| i.title).collect();
        assert!(!titles
            .iter()
            .any(|t| t == "Unsafe link" || t == "Plain http"));
    }

    #[test]
    fn garbage_and_empty_input_give_nothing() {
        assert!(parse_feed("").is_empty());
        assert!(parse_feed("<html>no feed</html>").is_empty());
        assert!(parse_feed("<item><title>cut off").is_empty());
    }

    #[test]
    fn numeric_and_named_entities_are_decoded() {
        assert_eq!(
            unescape("It&#8217;s fine&nbsp;now"),
            "It\u{2019}s fine\u{a0}now"
        );
        assert_eq!(unescape("a &#x2014; b &amp; c"), "a \u{2014} b & c");
        assert_eq!(unescape("5 &lt; 6 &gt; 4"), "5 < 6 > 4");
        // Not an entity: kept as written, and the rest is still decoded.
        assert_eq!(unescape("Tom & Jerry &amp; Co"), "Tom & Jerry & Co");
        assert_eq!(unescape("100&"), "100&");
        // Control characters are refused instead of being put into a title.
        assert_eq!(unescape("x&#1;y"), "x&#1;y");
    }

    #[test]
    fn own_news_are_sorted_and_broken_entries_left_out() {
        let items = parse_own(
            r#"{"items": [
                {"id": "old", "date": "2026-01-02", "title": {"ru": "Старая"}},
                {"id": "new", "date": "2026-11-05", "title": {"ru": "Новая", "en": "New"},
                 "body": {"ru": "Текст", "en": "  "}, "link": "https://example.com/a"},
                {"id": "no-title", "date": "2026-02-02", "title": {"ru": "  "}},
                {"date": "2026-02-03", "title": {"ru": "Нет id"}},
                "not an object"
            ]}"#,
        )
        .unwrap();
        let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["new", "old"]);
        assert_eq!(items[0].body.len(), 1, "the blank translation is dropped");
        assert_eq!(items[0].link.as_deref(), Some("https://example.com/a"));
    }

    #[test]
    fn own_news_drop_unsafe_links_and_odd_dates() {
        let items = parse_own(
            r#"{"items": [
                {"id": "a", "date": "вчера", "title": {"ru": "А"}, "link": "javascript:alert(1)"},
                {"id": "b", "title": {"ru": "Б"}, "link": "http://example.com/"}
            ]}"#,
        )
        .unwrap();
        assert!(items.iter().all(|i| i.link.is_none() && i.date.is_empty()));
    }

    #[test]
    fn own_news_file_with_a_bom_is_read_and_garbage_is_an_error() {
        assert!(parse_own("\u{feff}{\"items\": []}").unwrap().is_empty());
        assert!(parse_own("<html>not found</html>").is_err());
        assert!(parse_own("{}").is_err());
    }

    #[test]
    fn own_news_list_is_capped() {
        let one = r#"{"id": "x", "title": {"ru": "x"}}"#;
        let text = format!(r#"{{"items": [{}]}}"#, vec![one; 100].join(","));
        assert_eq!(parse_own(&text).unwrap().len(), MAX_ITEMS);
    }

    #[test]
    fn the_list_is_capped() {
        let one = "<item><title>t</title><link>https://a.test/x</link></item>";
        assert_eq!(parse_feed(&one.repeat(100)).len(), MAX_ITEMS);
    }
}
