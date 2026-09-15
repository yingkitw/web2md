//! Part of the `extract` module.

use serde::Serialize;

use crate::html_meta::extract_attr;
use crate::html_util::{decode_html_entities, find_ci, strip_html_tags};
use super::resolve_url;

/// A single link extracted from the page.
#[derive(Debug, Serialize)]
pub struct LinkEntry {
    pub url: String,
    pub text: String,
}

/// Extract all `<a href>` links from HTML, resolving relative URLs against `base_url`.
/// Returns deduplicated links in document order.
pub fn extract_links(html: &str, base_url: &str) -> Vec<LinkEntry> {
    let mut links = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut pos = 0;

    while pos < html.len() {
        let Some(start) = find_ci(&html[pos..], "<a") else {
            break;
        };
        let start = pos + start;
        let Some(end) = html[start..].find('>') else {
            break;
        };
        let tag = &html[start..=start + end];
        let tag_end = start + end + 1;

        if let Some(href) = extract_attr(tag, "href")
            && let Some(resolved) = resolve_url(&href, base_url)
                && seen.insert(resolved.clone()) {
                    // Extract text content between <a ...> and </a>
                    let text = extract_anchor_text(&html[tag_end..]);
                    links.push(LinkEntry {
                        url: resolved,
                        text: text.trim().to_string(),
                    });
                }
        pos = tag_end;
    }

    links
}

/// Extract text between the opening `<a>` tag and its closing `</a>`.
fn extract_anchor_text(rest: &str) -> String {
    let Some(close) = find_ci(rest, "</a>") else {
        return String::new();
    };
    let inner = &rest[..close];
    let text = strip_html_tags(inner);
    decode_html_entities(&text).trim().to_string()
}

