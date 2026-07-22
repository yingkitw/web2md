//! Deterministic page-element extractors: links, images, and structured products.
//!
//! These mirror Firecrawl's `links`, `images`, and `product` formats but are
//! fully deterministic (no LLM, no SaaS) — they parse HTML and JSON-LD locally.

use serde::Serialize;

use crate::html_meta::{extract_attr, iter_json_ld_blocks};
use crate::html_util::{decode_html_entities, find_ci, strip_html_tags};

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

/// A single image extracted from the page.
#[derive(Debug, Serialize)]
pub struct ImageEntry {
    pub src: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Extract all `<img>` image URLs from HTML, resolving relative URLs against `base_url`.
/// Returns deduplicated images in document order.
pub fn extract_images(html: &str, base_url: &str) -> Vec<ImageEntry> {
    let mut images = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut pos = 0;

    while pos < html.len() {
        let Some(start) = find_ci(&html[pos..], "<img") else {
            break;
        };
        let start = pos + start;
        let Some(end) = html[start..].find('>') else {
            break;
        };
        let tag = &html[start..=start + end];
        let tag_end = start + end + 1;

        if let Some(src) = extract_attr(tag, "src")
            && let Some(resolved) = resolve_url(&src, base_url)
                && seen.insert(resolved.clone()) {
                    let alt = extract_attr(tag, "alt")
                        .filter(|s| !s.is_empty())
                        .map(|s| decode_html_entities(&s));
                    let title = extract_attr(tag, "title")
                        .filter(|s| !s.is_empty())
                        .map(|s| decode_html_entities(&s));
                    images.push(ImageEntry {
                        src: resolved,
                        alt,
                        title,
                    });
                }
        pos = tag_end;
    }

    images
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_links_resolves_relative_urls() {
        let html = r#"<a href="/about">About Us</a><a href="contact">Contact</a>"#;
        let links = extract_links(html, "https://example.com/blog/post");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].url, "https://example.com/about");
        assert_eq!(links[0].text, "About Us");
        assert_eq!(links[1].url, "https://example.com/blog/contact");
        assert_eq!(links[1].text, "Contact");
    }

    #[test]
    fn extract_links_skips_empty_and_fragment_hrefs() {
        let html = r##"<a href="#top">Top</a><a href="">Empty</a><a href="/ok">OK</a>"##;
        let links = extract_links(html, "https://example.com/");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://example.com/ok");
    }

    #[test]
    fn extract_links_deduplicates() {
        let html = r#"<a href="/a">A</a><a href="/a">A again</a>"#;
        let links = extract_links(html, "https://example.com/");
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn extract_links_strips_inner_html_tags() {
        let html = r#"<a href="/x"><b>Bold</b> Link</a>"#;
        let links = extract_links(html, "https://example.com/");
        assert_eq!(links[0].text, "Bold Link");
    }

    #[test]
    fn extract_images_resolves_src() {
        let html = r#"<img src="/img/logo.png" alt="Logo"><img src="https://cdn.com/x.jpg">"#;
        let images = extract_images(html, "https://example.com/page");
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].src, "https://example.com/img/logo.png");
        assert_eq!(images[0].alt.as_deref(), Some("Logo"));
        assert_eq!(images[1].src, "https://cdn.com/x.jpg");
        assert!(images[1].alt.is_none());
    }

    #[test]
    fn extract_images_deduplicates() {
        let html = r#"<img src="/a.png"><img src="/a.png" alt="dup">"#;
        let images = extract_images(html, "https://example.com/");
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn extract_images_skips_data_urls() {
        let html = r#"<img src="data:image/png;base64,abc"><img src="/real.png">"#;
        let images = extract_images(html, "https://example.com/");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].src, "https://example.com/real.png");
    }

    #[test]
    fn extract_product_from_json_ld() {
        let html = r#"<html><head>
            <script type="application/ld+json">
            {"@type":"Product","name":"Acme Widget","brand":{"@type":"Brand","name":"Acme"},
             "sku":"W-100","description":"A great widget","category":"Tools",
             "offers":{"@type":"Offer","price":"19.99","priceCurrency":"USD",
                       "availability":"https://schema.org/InStock"}}
            </script>
            </head><body></body></html>"#;
        let product = extract_product(html).unwrap();
        assert_eq!(product.name.as_deref(), Some("Acme Widget"));
        assert_eq!(product.brand.as_deref(), Some("Acme"));
        assert_eq!(product.sku.as_deref(), Some("W-100"));
        assert_eq!(product.description.as_deref(), Some("A great widget"));
        assert_eq!(product.category.as_deref(), Some("Tools"));
        assert_eq!(product.variants.len(), 1);
        assert_eq!(product.variants[0].price.as_deref(), Some("19.99"));
        assert_eq!(product.variants[0].currency.as_deref(), Some("USD"));
    }

    #[test]
    fn extract_product_returns_none_without_json_ld() {
        let html = "<html><body><p>No product here</p></body></html>";
        assert!(extract_product(html).is_none());
    }

    #[test]
    fn extract_product_handles_multiple_offers() {
        let html = r#"<html><head>
            <script type="application/ld+json">
            {"@type":"Product","name":"Widget",
             "offers":[{"@type":"Offer","price":"10.00","priceCurrency":"USD"},
                        {"@type":"Offer","price":"8.50","priceCurrency":"EUR"}]}
            </script></head><body></body></html>"#;
        let product = extract_product(html).unwrap();
        assert_eq!(product.variants.len(), 2);
        assert_eq!(product.variants[0].price.as_deref(), Some("10.00"));
        assert_eq!(product.variants[1].price.as_deref(), Some("8.50"));
    }

    #[test]
    fn extract_product_handles_image_as_array() {
        let html = r#"<html><head>
            <script type="application/ld+json">
            {"@type":"Product","name":"Widget","image":["https://x.com/a.jpg","https://x.com/b.jpg"]}
            </script></head><body></body></html>"#;
        let product = extract_product(html).unwrap();
        assert_eq!(product.image.as_deref(), Some("https://x.com/a.jpg"));
    }
}
