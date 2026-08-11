//! Deterministic page-element extractors: links, images, products, videos, attributes, menus.
//!
//! These mirror Firecrawl's `links`, `images`, `product`, `video`, `attributes`, and `menu`
//! formats but are fully deterministic (no LLM, no SaaS) — they parse HTML and JSON-LD locally.

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

/// Extract all `<img>` image URLs and CSS `background-image: url(...)` URLs from HTML,
/// resolving relative URLs against `base_url`. Returns deduplicated images in document order.
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

    // Extract background-image URLs from inline style="..." attributes
    extract_bg_from_style_attrs(html, '"', &mut images, &mut seen, base_url);
    // Also handle style='...'
    extract_bg_from_style_attrs(html, '\'', &mut images, &mut seen, base_url);

    // Extract from <style>...</style> blocks
    pos = 0;
    while pos < html.len() {
        let Some(tag_start) = find_ci(&html[pos..], "<style") else {
            break;
        };
        let tag_start = pos + tag_start;
        let Some(content_start_rel) = html[tag_start..].find('>') else {
            break;
        };
        let content_start = tag_start + content_start_rel + 1;
        let Some(end_rel) = find_ci(&html[content_start..], "</style>") else {
            break;
        };
        let style = &html[content_start..content_start + end_rel];
        pos = content_start + end_rel + 8;
        for url in extract_bg_urls_from_style(style) {
            if let Some(resolved) = resolve_url(&url, base_url)
                && seen.insert(resolved.clone()) {
                    images.push(ImageEntry {
                        src: resolved,
                        alt: None,
                        title: None,
                    });
                }
        }
    }

    images
}

/// Walk `style=<quote>...<quote>` attributes and collect background `url(...)` values.
fn extract_bg_from_style_attrs(
    html: &str,
    quote: char,
    images: &mut Vec<ImageEntry>,
    seen: &mut std::collections::HashSet<String>,
    base_url: &str,
) {
    let needle = format!("style={quote}");
    let mut pos = 0;
    while pos < html.len() {
        let Some(style_start) = find_ci(&html[pos..], &needle) else {
            break;
        };
        let style_start = pos + style_start + needle.len();
        let Some(style_end) = html[style_start..].find(quote) else {
            break;
        };
        let style = &html[style_start..style_start + style_end];
        pos = style_start + style_end + 1;
        for url in extract_bg_urls_from_style(style) {
            if let Some(resolved) = resolve_url(&url, base_url)
                && seen.insert(resolved.clone()) {
                    images.push(ImageEntry {
                        src: resolved,
                        alt: None,
                        title: None,
                    });
                }
        }
    }
}

/// Extract `url(...)` values from `background-image` or `background` CSS declarations.
fn extract_bg_urls_from_style(style: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let lower = style.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..].find("url(") {
        let abs = search_from + rel;
        // Only accept url() that belongs to a background / background-image property.
        let before = &lower[..abs];
        let is_bg = before.rfind("background").is_some_and(|i| {
            let frag = &before[i..];
            frag.starts_with("background-image")
                || frag.starts_with("background:")
                || frag.starts_with("background ")
                || frag.starts_with("background\t")
                || frag.starts_with("background\n")
        });
        if !is_bg {
            search_from = abs + 4;
            continue;
        }
        let after = &style[abs + 4..];
        let rest_trimmed = after.trim_start();
        let trim_len = after.len() - rest_trimmed.len();
        let (url, consumed_after_trim) = if let Some(s) = rest_trimmed.strip_prefix('\'') {
            match s.find('\'') {
                Some(end) => (s[..end].to_string(), end + 1),
                None => break,
            }
        } else if let Some(s) = rest_trimmed.strip_prefix('"') {
            match s.find('"') {
                Some(end) => (s[..end].to_string(), end + 1),
                None => break,
            }
        } else if let Some(end) = rest_trimmed.find(')') {
            (rest_trimmed[..end].trim().to_string(), end)
        } else {
            break;
        };
        if !url.is_empty() {
            urls.push(url);
        }
        search_from = abs + 4 + trim_len + consumed_after_trim;
    }
    urls
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

/// A single video extracted from the page.
#[derive(Debug, Serialize)]
pub struct VideoEntry {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Thumbnail / poster image URL (≈ Firecrawl `thumbnail`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    /// Duration in seconds when known (from `duration` attr or JSON-LD VideoObject).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

/// Extract all video URLs from HTML: `<video src>`, `<video><source src>`,
/// `<iframe>` embeds (YouTube, Vimeo, etc.), and JSON-LD `VideoObject` blocks.
/// URLs are resolved against `base_url`. Returns deduplicated entries in document order.
pub fn extract_videos(html: &str, base_url: &str) -> Vec<VideoEntry> {
    let mut videos = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut pos = 0;

    // Extract from <video src="..."> and <video><source src="...">
    while pos < html.len() {
        let Some(start) = find_ci(&html[pos..], "<video") else {
            break;
        };
        let start = pos + start;
        let Some(end) = html[start..].find('>') else {
            break;
        };
        let tag = &html[start..=start + end];
        let tag_end = start + end + 1;

        let title = extract_attr(tag, "title").filter(|s| !s.is_empty());
        let thumbnail = extract_attr(tag, "poster")
            .filter(|s| !s.is_empty())
            .and_then(|s| resolve_url(&s, base_url));
        let duration = extract_attr(tag, "duration")
            .and_then(|s| parse_duration_seconds(&s));

        // Check for src attribute on <video> tag itself
        if let Some(src) = extract_attr(tag, "src")
            && let Some(resolved) = resolve_url(&src, base_url)
                && seen.insert(resolved.clone()) {
                    videos.push(VideoEntry {
                        url: resolved,
                        source: Some("video".to_string()),
                        title: title.clone(),
                        thumbnail: thumbnail.clone(),
                        duration,
                    });
                }

        // Look for <source> tags within the <video> element
        let video_close = find_ci(&html[tag_end..], "</video>").unwrap_or(200);
        let video_inner = &html[tag_end..tag_end + video_close.min(html.len() - tag_end)];
        let mut source_pos = 0;
        while source_pos < video_inner.len() {
            let Some(s_start) = find_ci(&video_inner[source_pos..], "<source") else {
                break;
            };
            let s_start = source_pos + s_start;
            let Some(s_end) = video_inner[s_start..].find('>') else {
                break;
            };
            let source_tag = &video_inner[s_start..=s_start + s_end];
            if let Some(src) = extract_attr(source_tag, "src")
                && let Some(resolved) = resolve_url(&src, base_url)
                    && seen.insert(resolved.clone()) {
                        videos.push(VideoEntry {
                            url: resolved,
                            source: Some("source".to_string()),
                            title: title.clone(),
                            thumbnail: thumbnail.clone(),
                            duration,
                        });
                    }
            source_pos = s_start + s_end + 1;
        }

        pos = tag_end;
    }

    // Extract from <iframe> embeds (YouTube, Vimeo, etc.)
    pos = 0;
    while pos < html.len() {
        let Some(start) = find_ci(&html[pos..], "<iframe") else {
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
                && is_video_embed(&resolved)
                    && seen.insert(resolved.clone()) {
                        let source = embed_source_name(&resolved);
                        let title = extract_attr(tag, "title").filter(|s| !s.is_empty());
                        videos.push(VideoEntry {
                            url: resolved,
                            source: Some(source.to_string()),
                            title,
                            thumbnail: None,
                            duration: None,
                        });
                    }
        pos = tag_end;
    }

    // Merge JSON-LD VideoObject blocks (title / thumbnail / duration enrichment).
    for json in iter_json_ld_blocks(html) {
        if !json_ld_value_is_type(&json, "VideoObject") {
            continue;
        }
        let Some(url) = json
            .get("contentUrl")
            .and_then(json_string)
            .or_else(|| json.get("embedUrl").and_then(json_string))
            .or_else(|| json.get("url").and_then(json_string))
            .and_then(|u| resolve_url(&u, base_url))
        else {
            continue;
        };
        let title = json.get("name").and_then(json_string);
        let thumbnail = json
            .get("thumbnailUrl")
            .and_then(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| {
                        v.as_array()
                            .and_then(|a| a.first())
                            .and_then(json_string)
                    })
            })
            .and_then(|u| resolve_url(&u, base_url));
        let duration = json
            .get("duration")
            .and_then(json_string)
            .and_then(|s| parse_duration_seconds(&s));

        if let Some(existing) = videos.iter_mut().find(|v| v.url == url) {
            if existing.title.is_none() {
                existing.title = title;
            }
            if existing.thumbnail.is_none() {
                existing.thumbnail = thumbnail;
            }
            if existing.duration.is_none() {
                existing.duration = duration;
            }
        } else if seen.insert(url.clone()) {
            videos.push(VideoEntry {
                url,
                source: Some("json-ld".to_string()),
                title,
                thumbnail,
                duration,
            });
        }
    }

    videos
}

/// Parse a duration string into seconds. Accepts plain numbers or ISO-8601
/// (`PT1H2M3S`, `PT90S`, `PT1M30S`).
fn parse_duration_seconds(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    if let Ok(n) = trimmed.parse::<f64>() {
        return Some(n);
    }
    let upper = trimmed.to_ascii_uppercase();
    if !upper.starts_with("PT") {
        return None;
    }
    let mut rest = &upper[2..];
    let mut total = 0.0;
    if let Some(h_pos) = rest.find('H') {
        let hours: f64 = rest[..h_pos].parse().ok()?;
        total += hours * 3600.0;
        rest = &rest[h_pos + 1..];
    }
    if let Some(m_pos) = rest.find('M') {
        let mins: f64 = rest[..m_pos].parse().ok()?;
        total += mins * 60.0;
        rest = &rest[m_pos + 1..];
    }
    if let Some(s_pos) = rest.find('S') {
        let secs: f64 = rest[..s_pos].parse().ok()?;
        total += secs;
    }
    if total > 0.0 {
        Some(total)
    } else {
        None
    }
}

/// Check if a URL is a known video embed (YouTube, Vimeo, Dailymotion, etc.).
fn is_video_embed(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("youtube.com/embed/")
        || lower.contains("youtube-nocookie.com/embed/")
        || lower.contains("player.vimeo.com")
        || lower.contains("dailymotion.com/embed")
        || lower.contains("player.twitch.tv")
        || lower.contains("wistia.com")
        || lower.contains("loom.com/embed")
    || lower.contains("videopress.com/embed")
}

/// Extract the platform name from a video embed URL.
fn embed_source_name(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("youtube") {
        "youtube"
    } else if lower.contains("vimeo") {
        "vimeo"
    } else if lower.contains("dailymotion") {
        "dailymotion"
    } else if lower.contains("twitch") {
        "twitch"
    } else if lower.contains("wistia") {
        "wistia"
    } else if lower.contains("loom") {
        "loom"
    } else if lower.contains("videopress") {
        "videopress"
    } else {
        "embed"
    }
}

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
    fn extract_images_from_background_css() {
        let html = r#"<div style="background-image: url('/bg.jpg')">Hello</div>"#;
        let images = extract_images(html, "https://example.com/");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].src, "https://example.com/bg.jpg");
    }

    #[test]
    fn extract_images_from_background_css_quoted() {
        let html = r#"<div style="background: url('/img/hero.png') no-repeat">Hero</div>"#;
        let images = extract_images(html, "https://example.com/");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].src, "https://example.com/img/hero.png");
    }

    #[test]
    fn extract_images_from_single_quoted_style() {
        let html = r#"<div style='background-image: url("/hero.jpg")'>Hero</div>"#;
        let images = extract_images(html, "https://example.com/");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].src, "https://example.com/hero.jpg");
    }

    #[test]
    fn extract_images_from_style_block() {
        let html = r#"<style>.hero { background-image: url('/css-bg.png'); }</style><p>Hi</p>"#;
        let images = extract_images(html, "https://example.com/");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].src, "https://example.com/css-bg.png");
    }

    #[test]
    fn extract_images_dedup_across_img_and_bg() {
        let html = r#"<img src="/shared.png"><div style="background-image: url('/shared.png')">BG</div>"#;
        let images = extract_images(html, "https://example.com/");
        assert_eq!(images.len(), 1);
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

    #[test]
    fn extract_videos_from_video_src() {
        let html = r#"<video src="https://example.com/video.mp4" poster="https://example.com/poster.jpg" title="Demo" duration="PT1M30S"></video>"#;
        let videos = extract_videos(html, "https://example.com/");
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].url, "https://example.com/video.mp4");
        assert_eq!(videos[0].source.as_deref(), Some("video"));
        assert_eq!(videos[0].thumbnail.as_deref(), Some("https://example.com/poster.jpg"));
        assert_eq!(videos[0].title.as_deref(), Some("Demo"));
        assert_eq!(videos[0].duration, Some(90.0));
    }

    #[test]
    fn extract_videos_from_source_tags() {
        let html = r#"<video><source src="https://example.com/video.webm" type="video/webm"><source src="https://example.com/video.mp4" type="video/mp4"></video>"#;
        let videos = extract_videos(html, "https://example.com/");
        assert_eq!(videos.len(), 2);
        assert_eq!(videos[0].url, "https://example.com/video.webm");
        assert_eq!(videos[0].source.as_deref(), Some("source"));
        assert_eq!(videos[1].url, "https://example.com/video.mp4");
    }

    #[test]
    fn extract_videos_from_iframe_embeds() {
        let html = r#"<iframe src="https://www.youtube.com/embed/dQw4w9WgXcQ"></iframe>"#;
        let videos = extract_videos(html, "https://example.com/");
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].source.as_deref(), Some("youtube"));
    }

    #[test]
    fn extract_videos_skips_non_video_iframes() {
        let html = r#"<iframe src="https://example.com/ads.html"></iframe>"#;
        let videos = extract_videos(html, "https://example.com/");
        assert_eq!(videos.len(), 0);
    }

    #[test]
    fn extract_videos_deduplicates() {
        let html = r#"<video src="https://example.com/video.mp4"></video><video src="https://example.com/video.mp4"></video>"#;
        let videos = extract_videos(html, "https://example.com/");
        assert_eq!(videos.len(), 1);
    }

    #[test]
    fn extract_videos_from_json_ld_video_object() {
        let html = r#"<html><head>
            <script type="application/ld+json">
            {"@type":"VideoObject","name":"Talk","contentUrl":"https://cdn.example.com/talk.mp4",
             "thumbnailUrl":"https://cdn.example.com/talk.jpg","duration":"PT2M"}
            </script></head><body></body></html>"#;
        let videos = extract_videos(html, "https://example.com/");
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].title.as_deref(), Some("Talk"));
        assert_eq!(videos[0].thumbnail.as_deref(), Some("https://cdn.example.com/talk.jpg"));
        assert_eq!(videos[0].duration, Some(120.0));
        assert_eq!(videos[0].source.as_deref(), Some("json-ld"));
    }

    #[test]
    fn extract_attributes_collects_values() {
        let html = r#"<a href="/a">A</a><a href="/b">B</a><img src="/x.png" data-id="1">"#;
        let specs = vec!["a:href".into(), "img:data-id".into()];
        let results = extract_attributes(html, &specs);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].values, vec!["/a", "/b"]);
        assert_eq!(results[1].values, vec!["1"]);
    }

    #[test]
    fn extract_menu_from_json_ld() {
        let html = r#"<html><head>
            <script type="application/ld+json">
            {"@type":"Menu","name":"Dinner",
             "hasMenuSection":[{
               "@type":"MenuSection","name":"Mains",
               "hasMenuItem":[{
                 "@type":"MenuItem","name":"Pasta",
                 "description":"Tomato sauce",
                 "offers":{"@type":"Offer","price":"14.50","priceCurrency":"USD"}
               }]
             }]}
            </script></head><body></body></html>"#;
        let menu = extract_menu(html).unwrap();
        assert_eq!(menu.name.as_deref(), Some("Dinner"));
        assert_eq!(menu.currency.as_deref(), Some("USD"));
        assert_eq!(menu.sections.len(), 1);
        assert_eq!(menu.sections[0].name.as_deref(), Some("Mains"));
        assert_eq!(menu.sections[0].items[0].name.as_deref(), Some("Pasta"));
        assert_eq!(menu.sections[0].items[0].price.as_deref(), Some("14.50"));
    }

    #[test]
    fn extract_menu_returns_none_without_json_ld() {
        assert!(extract_menu("<html><body>No menu</body></html>").is_none());
    }
}
