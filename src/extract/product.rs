//! Part of the `extract` module.

use serde::Serialize;

use crate::html_meta::iter_json_ld_blocks;
use super::{json_ld_value_is_type, json_string};

/// A structured product extracted from JSON-LD `Product` schema.
#[derive(Debug, Serialize)]
pub struct ProductEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sku: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mpn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gtin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub variants: Vec<ProductVariant>,
}

/// A single product variant (from `offers` or `Offer`).
#[derive(Debug, Serialize)]
pub struct ProductVariant {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Extract a structured product from JSON-LD `Product` blocks.
/// Returns `None` if no Product JSON-LD is found.
pub fn extract_product(html: &str) -> Option<ProductEntry> {
    let mut product: Option<ProductEntry> = None;

    for json in iter_json_ld_blocks(html) {
        if !json_ld_value_is_type(&json, "Product") {
            continue;
        }

        let name = json.get("name").and_then(json_string);
        let brand = json.get("brand").and_then(|b| {
            b.as_str()
                .map(|s| s.to_string())
                .or_else(|| b.get("name").and_then(json_string))
        });
        let description = json.get("description").and_then(json_string);
        let category = json.get("category").and_then(json_string);
        let sku = json.get("sku").and_then(json_string);
        let mpn = json.get("mpn").and_then(json_string);
        let gtin = json
            .get("gtin13")
            .and_then(json_string)
            .or_else(|| json.get("gtin12").and_then(json_string))
            .or_else(|| json.get("gtin").and_then(json_string));
        let image = json.get("image").and_then(|img| {
            img.as_str()
                .map(|s| s.to_string())
                .or_else(|| img.get("url").and_then(json_string))
                .or_else(|| {
                    img.as_array()
                        .and_then(|arr| arr.first())
                        .and_then(|v| {
                            v.as_str()
                                .map(|s| s.to_string())
                                .or_else(|| v.get("url").and_then(json_string))
                        })
                })
        });
        let url = json.get("url").and_then(json_string);

        let variants = extract_variants(&json);

        let entry = ProductEntry {
            name,
            brand,
            description,
            category,
            sku,
            mpn,
            gtin,
            image,
            url,
            variants,
        };

        // First Product block wins; subsequent ones are ignored.
        if product.is_none() {
            product = Some(entry);
        }
    }

    product
}

/// Extract variant/offer info from a Product JSON-LD block.
fn extract_variants(product: &serde_json::Value) -> Vec<ProductVariant> {
    let mut variants = Vec::new();
    if let Some(offers) = product.get("offers") {
        if let Some(arr) = offers.as_array() {
            for offer in arr {
                if let Some(v) = offer_to_variant(offer) {
                    variants.push(v);
                }
            }
        } else if let Some(v) = offer_to_variant(offers) {
            variants.push(v);
        }
    }
    variants
}

fn offer_to_variant(offer: &serde_json::Value) -> Option<ProductVariant> {
    let price = offer.get("price").and_then(json_string);
    let currency = offer
        .get("priceCurrency")
        .and_then(json_string);
    let availability = offer.get("availability").and_then(json_string);
    let condition = offer.get("itemCondition").and_then(json_string);
    let url = offer.get("url").and_then(json_string);

    if price.is_none()
        && currency.is_none()
        && availability.is_none()
        && condition.is_none()
        && url.is_none()
    {
        return None;
    }

    Some(ProductVariant {
        price,
        currency,
        availability,
        condition,
        url,
    })
}

