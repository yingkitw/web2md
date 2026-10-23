//! JSON-LD `BreadcrumbList` extraction (deterministic).

use serde::Serialize;

use crate::html_meta::iter_json_ld_blocks;
use super::{json_ld_value_is_type, json_string, resolve_url};

/// One crumb in a breadcrumb trail.
#[derive(Debug, Serialize, Clone)]
pub struct BreadcrumbCrumb {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u64>,
}

/// A breadcrumb trail extracted from JSON-LD `BreadcrumbList`.
#[derive(Debug, Serialize)]
pub struct BreadcrumbList {
    pub items: Vec<BreadcrumbCrumb>,
}

/// Extract breadcrumb trails from JSON-LD `BreadcrumbList` blocks.
/// Relative item URLs are resolved against `base_url`.
pub fn extract_breadcrumbs(html: &str, base_url: &str) -> Option<BreadcrumbList> {
    for json in iter_json_ld_blocks(html) {
        if let Some(list) = parse_breadcrumb_list(&json, base_url) {
            return Some(list);
        }
        if let Some(graph) = json.get("@graph").and_then(|v| v.as_array()) {
            for node in graph {
                if let Some(list) = parse_breadcrumb_list(node, base_url) {
                    return Some(list);
                }
            }
        }
        if let Some(arr) = json.as_array() {
            for node in arr {
                if let Some(list) = parse_breadcrumb_list(node, base_url) {
                    return Some(list);
                }
            }
        }
    }
    None
}

fn parse_breadcrumb_list(json: &serde_json::Value, base_url: &str) -> Option<BreadcrumbList> {
    if !json_ld_value_is_type(json, "BreadcrumbList") {
        return None;
    }
    let elements = json.get("itemListElement")?;
    let nodes: Vec<&serde_json::Value> = if let Some(arr) = elements.as_array() {
        arr.iter().collect()
    } else {
        vec![elements]
    };
    let mut items = Vec::new();
    for node in nodes {
        let name = node
            .get("name")
            .and_then(json_string)
            .or_else(|| node.get("item").and_then(|i| i.get("name")).and_then(json_string))
            .or_else(|| {
                node.get("item")
                    .and_then(|i| i.as_str())
                    .map(|s| s.to_string())
            });
        let Some(name) = name.filter(|s| !s.trim().is_empty()) else {
            continue;
        };
        let url = node
            .get("item")
            .and_then(|i| {
                i.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| i.get("@id").and_then(json_string))
                    .or_else(|| i.get("url").and_then(json_string))
                    .or_else(|| i.get("id").and_then(json_string))
            })
            .or_else(|| node.get("@id").and_then(json_string))
            .and_then(|u| resolve_url(&u, base_url));
        let position = node
            .get("position")
            .and_then(|p| p.as_u64().or_else(|| p.as_str()?.parse().ok()));
        items.push(BreadcrumbCrumb {
            name: name.trim().to_string(),
            url,
            position,
        });
    }
    if items.is_empty() {
        None
    } else {
        items.sort_by_key(|c| c.position.unwrap_or(u64::MAX));
        Some(BreadcrumbList { items })
    }
}
