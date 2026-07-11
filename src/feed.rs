//! RSS 2.0, Atom, and JSON Feed parsing and Markdown conversion.

use crate::html_meta::extract_attr;
use crate::html_util::{decode_html_entities, find_ci, strip_html_tags};

/// A single entry/item from an RSS, Atom, or JSON Feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedEntry {
    pub title: Option<String>,
    pub link: Option<String>,
    pub published: Option<String>,
    pub summary: Option<String>,
}

/// A parsed RSS, Atom, or JSON Feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    pub title: Option<String>,
    pub link: Option<String>,
    pub entries: Vec<FeedEntry>,
}

/// Parse RSS 2.0, Atom, or JSON Feed content into a [`Feed`].
/// Returns `None` when the document is not a recognized feed.
pub fn parse_feed(content: &str) -> Option<Feed> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('{') {
        return parse_json_feed(trimmed);
    }
    if find_ci(content, "<feed").is_some()
        && (find_ci(content, "<entry").is_some() || find_ci(content, "</feed>").is_some())
    {
        return Some(parse_atom(content));
    }
    if find_ci(content, "<rss").is_some() || find_ci(content, "<channel").is_some() {
        return Some(parse_rss(content));
    }
    None
}

/// Convert a feed to Markdown (feed title as H1, each entry as H2 with metadata).
pub fn feed_to_markdown(feed: &Feed) -> String {
    let mut out = String::new();

    if let Some(ref title) = feed.title {
        out.push_str("# ");
        out.push_str(title);
        out.push_str("\n\n");
    }
    if let Some(ref link) = feed.link {
        out.push_str(&format!("Feed: {}\n\n", link));
    }

    for entry in &feed.entries {
        if let Some(ref title) = entry.title {
            out.push_str("## ");
            if let Some(ref link) = entry.link {
                out.push_str(&format!("[{}]({})\n\n", title, link));
            } else {
                out.push_str(title);
                out.push_str("\n\n");
            }
        } else if let Some(ref link) = entry.link {
            out.push_str(&format!("## {}\n\n", link));
        }

        if let Some(ref published) = entry.published {
            out.push_str(&format!("*{}*\n\n", published));
        }

        if let Some(ref summary) = entry.summary {
            let text = strip_simple_html(summary);
            if !text.is_empty() {
                out.push_str(&text);
                out.push_str("\n\n");
            }
        }
    }

    out.trim_end().to_string()
}

/// Parse a [JSON Feed](https://www.jsonfeed.org/) document (version 1 / 1.1).
fn parse_json_feed(json_str: &str) -> Option<Feed> {
    let json: serde_json::Value = serde_json::from_str(json_str).ok()?;
    // Require items array (or empty) and either version or title to avoid mistaking random JSON.
    let items = json.get("items")?.as_array()?;
    if json.get("version").is_none() && json.get("title").is_none() {
        return None;
    }

    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let link = json
        .get("home_page_url")
        .or_else(|| json.get("feed_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let entries = items
        .iter()
        .map(|item| FeedEntry {
            title: item
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            link: item
                .get("url")
                .or_else(|| item.get("external_url"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            published: item
                .get("date_published")
                .or_else(|| item.get("date_modified"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            summary: item
                .get("content_text")
                .or_else(|| item.get("summary"))
                .or_else(|| item.get("content_html"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        })
        .collect();

    Some(Feed {
        title,
        link,
        entries,
    })
}

fn parse_rss(xml: &str) -> Feed {
    let channel = extract_block(xml, "channel").unwrap_or(xml);
    let title = first_text_tag(channel, "title");
    let link = first_text_tag(channel, "link");

    let mut entries = Vec::new();
    for item in iter_blocks(channel, "item") {
        let entry_title = first_text_tag(item, "title");
        let entry_link = first_text_tag(item, "link")
            .or_else(|| first_text_tag(item, "guid"));
        let published = first_text_tag(item, "pubDate");
        let summary = first_text_tag(item, "content:encoded")
            .or_else(|| first_text_tag(item, "description"));
        entries.push(FeedEntry {
            title: entry_title,
            link: entry_link,
            published,
            summary,
        });
    }

    Feed {
        title,
        link,
        entries,
    }
}

fn parse_atom(xml: &str) -> Feed {
    let feed_body = extract_block(xml, "feed").unwrap_or(xml);
    let title = first_text_tag(feed_body, "title");
    let link = first_atom_link(feed_body);

    let mut entries = Vec::new();
    for entry in iter_blocks(feed_body, "entry") {
        let entry_title = first_text_tag(entry, "title");
        let entry_link = first_atom_link(entry);
        let published = first_text_tag(entry, "published")
            .or_else(|| first_text_tag(entry, "updated"));
        let summary = first_text_tag(entry, "content")
            .or_else(|| first_text_tag(entry, "summary"));
        entries.push(FeedEntry {
            title: entry_title,
            link: entry_link,
            published,
            summary,
        });
    }

    Feed {
        title,
        link,
        entries,
    }
}

/// Extract the inner XML of the first `<tag>...</tag>` block (case-insensitive).
fn extract_block<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let start = find_ci(xml, &open)?;
    let after_open = xml[start..].find('>')? + start + 1;
    let end = find_ci(&xml[after_open..], &close)? + after_open;
    Some(&xml[after_open..end])
}

/// Iterate over all `<tag>...</tag>` blocks, yielding inner content.
fn iter_blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut blocks = Vec::new();
    let mut pos = 0;
    while pos < xml.len() {
        let Some(rel) = find_ci(&xml[pos..], &open) else {
            break;
        };
        let start = pos + rel;
        // Ensure we matched the tag name boundary (e.g. <item not <items)
        let after_name = start + open.len();
        if after_name < xml.len() {
            let next = xml.as_bytes()[after_name];
            if next.is_ascii_alphanumeric() || next == b':' || next == b'-' || next == b'_' {
                pos = after_name;
                continue;
            }
        }
        let Some(gt) = xml[start..].find('>') else {
            break;
        };
        let after_open = start + gt + 1;
        let Some(rel_end) = find_ci(&xml[after_open..], &close) else {
            break;
        };
        let end = after_open + rel_end;
        blocks.push(&xml[after_open..end]);
        pos = end + close.len();
    }
    blocks
}

/// First text content of `<tag>...</tag>`, unwrapping CDATA and decoding entities.
fn first_text_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut pos = 0;
    while pos < xml.len() {
        let Some(rel) = find_ci(&xml[pos..], &open) else {
            return None;
        };
        let start = pos + rel;
        let after_name = start + open.len();
        if after_name < xml.len() {
            let next = xml.as_bytes()[after_name];
            if next.is_ascii_alphanumeric() || next == b':' || next == b'-' || next == b'_' {
                pos = after_name;
                continue;
            }
        }
        // Self-closing or empty
        let Some(gt) = xml[start..].find('>') else {
            return None;
        };
        let open_tag = &xml[start..start + gt + 1];
        if open_tag.ends_with("/>") {
            return None;
        }
        let after_open = start + gt + 1;
        let Some(rel_end) = find_ci(&xml[after_open..], &close) else {
            return None;
        };
        let raw = xml[after_open..after_open + rel_end].trim();
        return Some(normalize_text(raw));
    }
    None
}

/// Atom `<link href="..."/>` — prefer `rel="alternate"` or first href.
/// Searches only until the first `<entry` so feed-level links ignore entry links.
fn first_atom_link(xml: &str) -> Option<String> {
    let search = match find_ci(xml, "<entry") {
        Some(e) => &xml[..e],
        None => xml,
    };
    let mut first: Option<String> = None;
    let mut pos = 0;
    while pos < search.len() {
        let Some(rel) = find_ci(&search[pos..], "<link") else {
            break;
        };
        let start = pos + rel;
        let Some(gt) = search[start..].find('>') else {
            break;
        };
        let tag = &search[start..=start + gt];
        if let Some(href) = extract_attr(tag, "href") {
            if !href.is_empty() {
                let is_alternate = find_ci(tag, "rel=\"alternate\"").is_some()
                    || find_ci(tag, "rel='alternate'").is_some()
                    || find_ci(tag, "rel=").is_none();
                if is_alternate {
                    return Some(href);
                }
                if first.is_none() {
                    first = Some(href);
                }
            }
        }
        pos = start + gt + 1;
    }
    first
}

fn normalize_text(raw: &str) -> String {
    let text = if let Some(inner) = unwrap_cdata(raw) {
        inner.to_string()
    } else {
        raw.to_string()
    };
    decode_html_entities(text.trim())
}

fn unwrap_cdata(s: &str) -> Option<&str> {
    let s = s.trim();
    if s.starts_with("<![CDATA[") && s.ends_with("]]>") {
        Some(&s[9..s.len() - 3])
    } else {
        None
    }
}

/// Strip common HTML tags from feed summaries for plain Markdown body text.
fn strip_simple_html(html: &str) -> String {
    decode_html_entities(strip_html_tags(html).trim())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rss_basic() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <title>Tech Blog</title>
  <link>https://example.com</link>
  <item>
    <title>First Post</title>
    <link>https://example.com/1</link>
    <pubDate>Sat, 11 Jul 2026 12:00:00 GMT</pubDate>
    <description>Hello world summary</description>
  </item>
  <item>
    <title>Second Post</title>
    <link>https://example.com/2</link>
    <description><![CDATA[<p>HTML <b>body</b></p>]]></description>
  </item>
</channel></rss>"#;
        let feed = parse_feed(xml).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Tech Blog"));
        assert_eq!(feed.link.as_deref(), Some("https://example.com"));
        assert_eq!(feed.entries.len(), 2);
        assert_eq!(feed.entries[0].title.as_deref(), Some("First Post"));
        assert_eq!(feed.entries[0].link.as_deref(), Some("https://example.com/1"));
        assert_eq!(
            feed.entries[0].published.as_deref(),
            Some("Sat, 11 Jul 2026 12:00:00 GMT")
        );
        assert_eq!(feed.entries[0].summary.as_deref(), Some("Hello world summary"));
        assert_eq!(feed.entries[1].summary.as_deref(), Some("<p>HTML <b>body</b></p>"));
    }

    #[test]
    fn parse_atom_basic() {
        let xml = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Feed</title>
  <link href="https://example.com/atom" rel="self"/>
  <link href="https://example.com/" rel="alternate"/>
  <entry>
    <title>Entry One</title>
    <link href="https://example.com/e1" rel="alternate"/>
    <published>2026-07-11T10:00:00Z</published>
    <summary>Short summary</summary>
  </entry>
</feed>"#;
        let feed = parse_feed(xml).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Atom Feed"));
        assert_eq!(feed.link.as_deref(), Some("https://example.com/"));
        assert_eq!(feed.entries.len(), 1);
        assert_eq!(feed.entries[0].title.as_deref(), Some("Entry One"));
        assert_eq!(feed.entries[0].link.as_deref(), Some("https://example.com/e1"));
        assert_eq!(
            feed.entries[0].published.as_deref(),
            Some("2026-07-11T10:00:00Z")
        );
        assert_eq!(feed.entries[0].summary.as_deref(), Some("Short summary"));
    }

    #[test]
    fn parse_non_feed_returns_none() {
        assert!(parse_feed("<html><body>Not a feed</body></html>").is_none());
        assert!(parse_feed("").is_none());
        assert!(parse_feed(r#"{"hello":"world"}"#).is_none());
    }

    #[test]
    fn parse_json_feed_basic() {
        let json = r#"{
            "version": "https://jsonfeed.org/version/1.1",
            "title": "JSON Feed Blog",
            "home_page_url": "https://example.com/",
            "items": [
                {
                    "id": "1",
                    "url": "https://example.com/1",
                    "title": "First Item",
                    "content_text": "Plain text body",
                    "date_published": "2026-07-11T12:00:00Z"
                },
                {
                    "id": "2",
                    "title": "Second Item",
                    "summary": "Short summary",
                    "external_url": "https://example.com/2"
                }
            ]
        }"#;
        let feed = parse_feed(json).unwrap();
        assert_eq!(feed.title.as_deref(), Some("JSON Feed Blog"));
        assert_eq!(feed.link.as_deref(), Some("https://example.com/"));
        assert_eq!(feed.entries.len(), 2);
        assert_eq!(feed.entries[0].title.as_deref(), Some("First Item"));
        assert_eq!(feed.entries[0].link.as_deref(), Some("https://example.com/1"));
        assert_eq!(
            feed.entries[0].published.as_deref(),
            Some("2026-07-11T12:00:00Z")
        );
        assert_eq!(feed.entries[0].summary.as_deref(), Some("Plain text body"));
        assert_eq!(feed.entries[1].link.as_deref(), Some("https://example.com/2"));
        assert_eq!(feed.entries[1].summary.as_deref(), Some("Short summary"));
    }

    #[test]
    fn feed_to_markdown_renders_entries() {
        let feed = Feed {
            title: Some("Blog".to_string()),
            link: Some("https://example.com".to_string()),
            entries: vec![FeedEntry {
                title: Some("Hello".to_string()),
                link: Some("https://example.com/hello".to_string()),
                published: Some("2026-07-11".to_string()),
                summary: Some("<p>Body &amp; more</p>".to_string()),
            }],
        };
        let md = feed_to_markdown(&feed);
        assert!(md.contains("# Blog"));
        assert!(md.contains("Feed: https://example.com"));
        assert!(md.contains("## [Hello](https://example.com/hello)"));
        assert!(md.contains("*2026-07-11*"));
        assert!(md.contains("Body & more"));
    }

    #[test]
    fn rss_content_encoded_preferred_over_description() {
        let xml = r#"<rss><channel>
  <item>
    <title>T</title>
    <description>short</description>
    <content:encoded><![CDATA[full body]]></content:encoded>
  </item>
</channel></rss>"#;
        let feed = parse_feed(xml).unwrap();
        assert_eq!(feed.entries[0].summary.as_deref(), Some("full body"));
    }

    #[test]
    fn atom_empty_feed_still_parses() {
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Empty</title></feed>"#;
        let feed = parse_feed(xml).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Empty"));
        assert!(feed.entries.is_empty());
    }
}
