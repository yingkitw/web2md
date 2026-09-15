//! Part of the `extract` module.

use serde::Serialize;

use crate::html_meta::iter_json_ld_blocks;
use super::{json_ld_value_is_type, json_string};

/// A restaurant/café menu extracted from JSON-LD `Menu` schema (≈ Firecrawl `menu`).
#[derive(Debug, Serialize)]
pub struct MenuEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    pub sections: Vec<MenuSection>,
}

#[derive(Debug, Serialize)]
pub struct MenuSection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub items: Vec<MenuItem>,
}

#[derive(Debug, Serialize)]
pub struct MenuItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

/// Extract a structured menu from JSON-LD `Menu` / `MenuSection` / `MenuItem` blocks.
pub fn extract_menu(html: &str) -> Option<MenuEntry> {
    for json in iter_json_ld_blocks(html) {
        if let Some(menu) = parse_menu_value(&json) {
            return Some(menu);
        }
        // Also walk @graph arrays.
        if let Some(graph) = json.get("@graph").and_then(|v| v.as_array()) {
            for node in graph {
                if let Some(menu) = parse_menu_value(node) {
                    return Some(menu);
                }
            }
        }
    }
    None
}

fn parse_menu_value(json: &serde_json::Value) -> Option<MenuEntry> {
    if !json_ld_value_is_type(json, "Menu") {
        return None;
    }
    let name = json.get("name").and_then(json_string);
    let description = json.get("description").and_then(json_string);
    let sections = json
        .get("hasMenuSection")
        .map(parse_menu_sections)
        .unwrap_or_default();
    if sections.is_empty() && name.is_none() {
        return None;
    }
    let currency = sections
        .iter()
        .flat_map(|s| s.items.iter())
        .find_map(|i| i.currency.clone());
    Some(MenuEntry {
        name,
        description,
        currency,
        sections,
    })
}

fn parse_menu_sections(value: &serde_json::Value) -> Vec<MenuSection> {
    let nodes: Vec<&serde_json::Value> = if let Some(arr) = value.as_array() {
        arr.iter().collect()
    } else {
        vec![value]
    };
    nodes
        .into_iter()
        .filter(|n| json_ld_value_is_type(n, "MenuSection") || n.get("hasMenuItem").is_some() || n.get("name").is_some())
        .map(|n| MenuSection {
            name: n.get("name").and_then(json_string),
            items: n
                .get("hasMenuItem")
                .map(parse_menu_items)
                .unwrap_or_default(),
        })
        .collect()
}

fn parse_menu_items(value: &serde_json::Value) -> Vec<MenuItem> {
    let nodes: Vec<&serde_json::Value> = if let Some(arr) = value.as_array() {
        arr.iter().collect()
    } else {
        vec![value]
    };
    nodes
        .into_iter()
        .map(|n| {
            let (price, currency) = n
                .get("offers")
                .map(|o| {
                    let offer = o.as_array().and_then(|a| a.first()).unwrap_or(o);
                    (
                        offer
                            .get("price")
                            .and_then(|p| {
                                p.as_str()
                                    .map(|s| s.to_string())
                                    .or_else(|| p.as_f64().map(|f| f.to_string()))
                            }),
                        offer.get("priceCurrency").and_then(json_string),
                    )
                })
                .unwrap_or((None, None));
            MenuItem {
                name: n.get("name").and_then(json_string),
                description: n.get("description").and_then(json_string),
                price,
                currency,
            }
        })
        .collect()
}
