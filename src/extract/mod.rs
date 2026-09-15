//! Deterministic page-element extractors: links, images, products, videos, audio, attributes, menus.
//!
//! These mirror Firecrawl's `links`, `images`, `product`, `video`, `audio`, `attributes`, and `menu`
//! formats but are fully deterministic (no LLM, no SaaS) — they parse HTML and JSON-LD locally.

mod attributes;
mod images;
mod links;
mod media;
mod menu;
mod product;
#[cfg(test)]
mod tests;

pub use attributes::{extract_attributes, AttributeResult};
pub use images::{extract_images, ImageEntry};
pub use links::{extract_links, LinkEntry};
pub use media::{extract_audios, extract_videos, AudioEntry, VideoEntry};
pub use menu::{extract_menu, MenuEntry, MenuItem, MenuSection};
pub use product::{extract_product, ProductEntry, ProductVariant};

fn json_string(v: &serde_json::Value) -> Option<String> {
    v.as_str().map(|s| s.to_string())
}

fn json_ld_value_is_type(json: &serde_json::Value, type_name: &str) -> bool {
    match json.get("@type") {
        Some(t) => {
            if let Some(s) = t.as_str() {
                return s.eq_ignore_ascii_case(type_name);
            }
            if let Some(arr) = t.as_array() {
                return arr.iter().any(|v| {
                    v.as_str()
                        .map(|s| s.eq_ignore_ascii_case(type_name))
                        .unwrap_or(false)
                });
            }
            false
        }
        None => false,
    }
}

/// Resolve a possibly-relative URL against a base URL.
fn resolve_url(href: &str, base_url: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() || href.starts_with('#') || href.starts_with("data:") {
        return None;
    }

    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href.to_string());
    }
    if href.starts_with("//") {
        let base = url::Url::parse(base_url).ok()?;
        return Some(format!("{}:{}", base.scheme(), href));
    }

    let base = url::Url::parse(base_url).ok()?;
    base.join(href).ok().map(|u| u.to_string())
}
