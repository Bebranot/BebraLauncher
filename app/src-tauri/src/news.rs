//! The news tab: titles and links from the Space Station 14 blog feed, like the official launcher.

use serde::Serialize;

/// Where the official launcher reads its news from.
pub const FEED_URL: &str = "https://spacestation14.com/post/index.xml";

/// Most entries shown.
pub const MAX_ITEMS: usize = 30;

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
    fn the_list_is_capped() {
        let one = "<item><title>t</title><link>https://a.test/x</link></item>";
        assert_eq!(parse_feed(&one.repeat(100)).len(), MAX_ITEMS);
    }
}
