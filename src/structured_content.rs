//! Structured content extraction from JSON-LD and Open Graph metadata.
//! Used as a fallback when DOM-based main-content heuristics score poorly.

use crate::html_meta::{extract_meta_content, iter_json_ld_blocks};
use crate::html_util::find_ci;

const MIN_CONTENT_LEN: usize = 50;

/// Extract article text from structured metadata when DOM heuristics fail.
/// Priority: JSON-LD `articleBody` → JSON-LD `description` → `og:description` → `meta description`.
pub fn extract_structured_content(html: &str) -> Option<String> {
    extract_json_ld_article_body(html)
        .or_else(|| extract_json_ld_description(html))
        .or_else(|| extract_meta_description(html))
}

fn extract_json_ld_article_body(html: &str) -> Option<String> {
    for json in iter_json_ld_blocks(html) {
        if let Some(body) = article_body_from_value(&json) {
            return Some(wrap_article_html(&body));
        }
    }
    None
}

fn extract_json_ld_description(html: &str) -> Option<String> {
    for json in iter_json_ld_blocks(html) {
        if let Some(desc) = json.get("description").and_then(|v| v.as_str()) {
            if let Some(text) = substantial_text(desc) {
                return Some(wrap_article_html(&text));
            }
        }
        if let Some(graph) = json.get("@graph").and_then(|v| v.as_array()) {
            for item in graph {
                if let Some(desc) = item.get("description").and_then(|v| v.as_str()) {
                    if let Some(text) = substantial_text(desc) {
                        return Some(wrap_article_html(&text));
                    }
                }
            }
        }
    }
    None
}

fn extract_meta_description(html: &str) -> Option<String> {
    extract_meta_content(html, "property", "og:description")
        .or_else(|| extract_meta_content(html, "name", "description"))
        .and_then(|text| substantial_text(&text))
        .map(|text| wrap_article_html(&text))
}

fn article_body_from_value(json: &serde_json::Value) -> Option<String> {
    if let Some(body) = json.get("articleBody").and_then(|v| v.as_str()) {
        return substantial_text(body);
    }
    if let Some(graph) = json.get("@graph").and_then(|v| v.as_array()) {
        for item in graph {
            if let Some(body) = article_body_from_value(item) {
                return Some(body);
            }
        }
    }
    None
}

fn substantial_text(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.len() >= MIN_CONTENT_LEN {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Wrap plain text or HTML fragments in `<article>` for downstream conversion.
fn wrap_article_html(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with('<') && find_ci(trimmed, "</").is_some() {
        return format!("<article>{trimmed}</article>");
    }

    let paras: Vec<&str> = trimmed
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if paras.is_empty() {
        return format!("<article><p>{}</p></article>", escape_html(trimmed));
    }

    let inner: String = paras
        .iter()
        .map(|p| format!("<p>{}</p>", escape_html(p)))
        .collect();
    format!("<article>{inner}</article>")
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_ld_article_body_html() {
        let html = r#"<script type="application/ld+json">{"articleBody":"<p>Structured article body with enough text to qualify as meaningful main content from metadata.</p>"}</script>"#;
        let out = extract_structured_content(html).unwrap();
        assert!(out.contains("Structured article body"));
        assert!(out.starts_with("<article>"));
    }

    #[test]
    fn extracts_json_ld_description_plain_text() {
        let html = r#"<script type="application/ld+json">{"description":"A plain-text article description long enough to serve as structured fallback content when DOM extraction fails."}</script>"#;
        let out = extract_structured_content(html).unwrap();
        assert!(out.contains("<p>A plain-text article description"));
    }

    #[test]
    fn extracts_og_description_fallback() {
        let html = r#"<meta property="og:description" content="Open Graph description long enough to be used when no JSON-LD body is available on the page.">"#;
        let out = extract_structured_content(html).unwrap();
        assert!(out.contains("Open Graph description"));
    }

    #[test]
    fn skips_short_structured_content() {
        let html = r#"<meta property="og:description" content="Too short.">"#;
        assert!(extract_structured_content(html).is_none());
    }

    #[test]
    fn article_body_takes_priority_over_description() {
        let html = r#"
        <script type="application/ld+json">{
          "description":"Description field long enough to be used but should lose to articleBody when both are present.",
          "articleBody":"<p>Primary articleBody content wins because it is the full article text from structured data.</p>"
        }</script>"#;
        let out = extract_structured_content(html).unwrap();
        assert!(out.contains("Primary articleBody content"));
        assert!(!out.contains("Description field"));
    }
}
