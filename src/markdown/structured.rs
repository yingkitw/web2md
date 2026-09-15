//! JSON-LD / Open Graph structured-content fallback for low-scoring pages.

use super::find_ci;

const MIN_STRUCTURED_LEN: usize = 50;

/// JSON-LD / Open Graph fallback when DOM heuristics score poorly.
pub(super) fn extract_structured_content(html: &str) -> Option<String> {
    for json in crate::html_meta::iter_json_ld_blocks(html) {
        if let Some(text) = structured_text_from_json(&json) {
            return Some(wrap_structured_html(&text));
        }
    }
    crate::html_meta::extract_meta_content(html, "property", "og:description")
        .or_else(|| crate::html_meta::extract_meta_content(html, "name", "description"))
        .and_then(|text| substantial_structured_text(&text))
        .map(|text| wrap_structured_html(&text))
}

fn structured_text_from_json(json: &serde_json::Value) -> Option<String> {
    if let Some(body) = json.get("articleBody").and_then(|v| v.as_str())
        && let Some(text) = substantial_structured_text(body) {
            return Some(text);
        }
    if let Some(desc) = json.get("description").and_then(|v| v.as_str())
        && let Some(text) = substantial_structured_text(desc) {
            return Some(text);
        }
    json.get("@graph")
        .and_then(|v| v.as_array())
        .and_then(|items| items.iter().find_map(structured_text_from_json))
}

fn substantial_structured_text(s: &str) -> Option<String> {
    let trimmed = s.trim();
    (trimmed.len() >= MIN_STRUCTURED_LEN).then(|| trimmed.to_string())
}

fn wrap_structured_html(text: &str) -> String {
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
        return format!("<article><p>{}</p></article>", escape_structured_html(trimmed));
    }

    let inner: String = paras
        .iter()
        .map(|p| format!("<p>{}</p>", escape_structured_html(p)))
        .collect();
    format!("<article>{inner}</article>")
}

fn escape_structured_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
