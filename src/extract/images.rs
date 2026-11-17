//! Part of the `extract` module.

use serde::Serialize;

use crate::html_meta::extract_attr;
use crate::html_util::{decode_html_entities, find_ci};
use super::resolve_url;

/// A single image extracted from the page.
#[derive(Debug, Serialize)]
pub struct ImageEntry {
    pub src: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Split a `srcset` attribute into its candidate URLs (drop density descriptors).
/// `data:` URLs are skipped — their embedded commas break the split.
fn srcset_candidates(raw: &str) -> Vec<String> {
    raw
        .split(',')
        .map(|part| {
            part.split_whitespace()
                .next()
                .unwrap_or("")
                .to_string()
        })
        .filter(|s| !s.is_empty() && !s.starts_with("data:"))
        .collect()
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
        // `srcset` variants (responsive images) — each distinct URL is an entry.
        if let Some(srcset) = extract_attr(tag, "srcset") {
            for url in srcset_candidates(&srcset) {
                if let Some(resolved) = resolve_url(&url, base_url)
                    && seen.insert(resolved.clone())
                {
                    images.push(ImageEntry {
                        src: resolved,
                        alt: None,
                        title: None,
                    });
                }
            }
        }
        pos = tag_end;
    }

    // `<picture><source srcset="...">` image sources (video/audio sources use `src` instead).
    pos = 0;
    while pos < html.len() {
        let Some(start) = find_ci(&html[pos..], "<source") else {
            break;
        };
        let start = pos + start;
        let Some(end) = html[start..].find('>') else {
            break;
        };
        let tag = &html[start..=start + end];
        pos = start + end + 1;
        let Some(srcset) = extract_attr(tag, "srcset") else {
            continue;
        };
        for url in srcset_candidates(&srcset) {
            if let Some(resolved) = resolve_url(&url, base_url)
                && seen.insert(resolved.clone())
            {
                images.push(ImageEntry {
                    src: resolved,
                    alt: None,
                    title: None,
                });
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Trafilatura 2.3 #946 parity: one URL across `<img>`, inline `style`,
    /// and `<style>` blocks must appear exactly once (cross-source dedup).
    #[test]
    fn images_dedup_across_img_inline_style_and_style_block() {
        let html = r#"<html><body>
        <img src="hero.png" alt="Hero">
        <div style="background-image: url('hero.png')"></div>
        <style>footer { background: url("hero.png") }</style>
        <style>.banner { background-image: url(banner.png) }</style>
        <img src="./hero.png" alt="Hero again">
        <img src="banner.png">
        </body></html>"#;
        let imgs = extract_images(html, "https://example.com/page");
        let srcs: Vec<&str> = imgs.iter().map(|i| i.src.as_str()).collect();
        assert_eq!(imgs.len(), 2, "got: {srcs:?}");
        assert!(srcs.contains(&"https://example.com/hero.png"));
        assert!(srcs.contains(&"https://example.com/banner.png"));
        // Alt preserved from the first <img> that introduced the URL.
        let hero = imgs
            .iter()
            .find(|i| i.src.ends_with("hero.png"))
            .expect("hero entry");
        assert_eq!(hero.alt.as_deref(), Some("Hero"));
    }

    /// Distinct URLs must not be collapsed by the dedup.
    #[test]
    fn images_keep_distinct_urls() {
        let html = r#"<html><body>
        <img src="a.png"><img src="b.png">
        </body></html>"#;
        let imgs = extract_images(html, "https://example.com/page");
        assert_eq!(imgs.len(), 2);
    }

    /// #80: `srcset` density variants and `<picture><source>` sources are extracted.
    #[test]
    fn images_extract_srcset_and_picture_sources() {
        let html = r#"<html><body>
        <img src="base.png" srcset="small.png 480w, large.png 1080w">
        <picture>
          <source srcset="photo.webp" type="image/webp">
          <img src="photo.png" alt="Photo">
        </picture>
        <img src="data:image/png;base64,AAAA,BBBB">
        </body></html>"#;
        let imgs = extract_images(html, "https://example.com/page");
        let srcs: Vec<&str> = imgs.iter().map(|i| i.src.as_str()).collect();
        assert!(srcs.contains(&"https://example.com/base.png"));
        assert!(srcs.contains(&"https://example.com/small.png"));
        assert!(srcs.contains(&"https://example.com/large.png"));
        assert!(srcs.contains(&"https://example.com/photo.webp"));
        assert!(srcs.contains(&"https://example.com/photo.png"));
        assert_eq!(imgs.len(), 5, "got: {srcs:?}");
        // data: URLs skipped (comma-split artifacts), alt taken from the <img>.
        assert!(!srcs.iter().any(|s| s.starts_with("data:")));
        assert_eq!(
            imgs.iter().find(|i| i.src.ends_with("photo.png")).and_then(|i| i.alt.clone()).as_deref(),
            Some("Photo")
        );
    }

    /// A srcset URL identical to `src` is deduplicated, not emitted twice.
    #[test]
    fn images_srcset_dedupes_against_src() {
        let html = r#"<img src="hero.png" srcset="hero.png 1x, hero@2x.png 2x">"#;
        let imgs = extract_images(html, "https://example.com/page");
        assert_eq!(imgs.len(), 2);
        assert!(imgs[0].src.ends_with("hero.png"));
        assert!(imgs[1].src.ends_with("hero@2x.png"));
    }
}

