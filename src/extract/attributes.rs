//! Part of the `extract` module.

use serde::Serialize;


/// One attribute extraction result (≈ Firecrawl `attributes` format entry).
#[derive(Debug, Serialize)]
pub struct AttributeResult {
    pub selector: String,
    pub attribute: String,
    pub values: Vec<String>,
}

/// Extract named HTML attributes for each `selector:attribute` pair.
/// `specs` entries are `"css-selector:attribute-name"` (e.g. `"a:href"`, `"img:src"`).
/// The last `:` separates selector from attribute so attribute selectors like
/// `[data-id]` still work when written as `[data-id]:data-id`.
pub fn extract_attributes(html: &str, specs: &[String]) -> Vec<AttributeResult> {
    use scraper::{Html, Selector};
    let document = Html::parse_document(html);
    let mut results = Vec::new();
    for spec in specs {
        let Some((selector, attribute)) = split_attr_spec(spec) else {
            continue;
        };
        let Ok(sel) = Selector::parse(selector) else {
            continue;
        };
        let mut values = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for element in document.select(&sel) {
            if let Some(val) = element.value().attr(attribute) {
                let trimmed = val.trim();
                if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                    values.push(trimmed.to_string());
                }
            }
        }
        results.push(AttributeResult {
            selector: selector.to_string(),
            attribute: attribute.to_string(),
            values,
        });
    }
    results
}

/// Split `"selector:attribute"` on the last colon.
fn split_attr_spec(spec: &str) -> Option<(&str, &str)> {
    let trimmed = spec.trim();
    let idx = trimmed.rfind(':')?;
    let selector = trimmed[..idx].trim();
    let attribute = trimmed[idx + 1..].trim();
    if selector.is_empty() || attribute.is_empty() {
        return None;
    }
    Some((selector, attribute))
}

