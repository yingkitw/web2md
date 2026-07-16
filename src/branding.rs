//! Deterministic brand/design extraction from inline `<style>` blocks and
//! `<link rel="stylesheet">` markup in HTML.
//!
//! Pulls:
//! - Top-N hex colors by frequency (with prefixed `#`).
//! - Font families referenced in `font-family:` declarations.
//! - Background-color declarations.
//! - Primary heading sizes (`font-size:` near `<h1>` rules, if any).
//!
//! No LLM, no JS rendering. Output mirrors Firecrawl's `branding` shape but
//! stays free and deterministic.

use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct BrandingProfile {
    /// CSS color scheme guess ("light", "dark", or "unknown") inferred from
    /// body background luminance.
    pub color_scheme: String,
    /// Top color tokens by frequency across the document.
    pub colors: Vec<ColorStat>,
    /// Distinct font families referenced in any `font-family:` declaration.
    pub fonts: Vec<String>,
    /// Background-color tokens (one per unique value).
    pub background_colors: Vec<String>,
    /// Heading sizes declared on `h1` / `h2` / `h3` selectors, in source order.
    pub heading_sizes: Vec<HeadingSize>,
}

#[derive(Debug, Serialize)]
pub struct ColorStat {
    pub value: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct HeadingSize {
    pub selector: String,
    pub size: String,
}

/// Extract a BrandingProfile from raw HTML. Inline `<style>` blocks are
/// scanned; external stylesheets are not fetched (the request would be
/// extra round-trips and the deterministic extraction is still useful without).
pub fn extract_branding(html: &str) -> BrandingProfile {
    let css = collect_inline_css(html);
    let mut color_counts: HashMap<String, usize> = HashMap::new();
    let mut fonts: Vec<String> = Vec::new();
    let mut background_colors: Vec<String> = Vec::new();
    let mut heading_sizes: Vec<HeadingSize> = Vec::new();

    for sheet in &css {
        for decl in sheet.split(';') {
            let decl = decl.trim();
            let Some(idx) = decl.find(':') else {
                continue;
            };
            let prop = decl[..idx].trim().to_ascii_lowercase();
            let val = decl[idx + 1..].trim().to_string();
            if prop.ends_with("color") || prop == "background" {
                if let Some(token) = first_color_token(&val) {
                    *color_counts.entry(token.clone()).or_insert(0) += 1;
                    if prop == "background-color" || prop == "background" {
                        background_colors.push(token);
                    }
                }
            }
            if prop == "font-family" {
                for f in split_font_family(&val) {
                    if !fonts.contains(&f) {
                        fonts.push(f);
                    }
                }
            }
        }
    }

    // Extract h1/h2/h3 font-size per declaration block (best-effort).
    for sheet in &css {
        let mut idx = 0usize;
        while let Some(pos) = sheet[idx..].find('{') {
            let open = idx + pos;
            let Some(close) = sheet[open..].find('}') else { break };
            let body = &sheet[open + 1..open + close];
            let selector_end = open;
            // selector is between previous `}` and this `{`
            let selector_start = sheet[..open]
                .rfind('}')
                .map(|i| i + 1)
                .unwrap_or(0);
            let selector = sheet[selector_start..selector_end]
                .trim()
                .to_string();
            if selector.split(',').any(|s| {
                let t = s.trim().to_ascii_lowercase();
                t == "h1" || t == "h2" || t == "h3"
            }) {
                let size_re =
                    Regex::new(r"(?i)font-size\s*:\s*([^;]+)").unwrap();
                if let Some(c) = size_re.captures(body) {
                    heading_sizes.push(HeadingSize {
                        selector,
                        size: c[1].trim().to_string(),
                    });
                }
            }
            idx = open + close + 1;
        }
    }

    // Order colors by frequency descending; keep top 20.
    let mut sorted: Vec<(String, usize)> = color_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    sorted.truncate(20);
    let colors: Vec<ColorStat> = sorted
        .into_iter()
        .map(|(value, count)| ColorStat { value, count })
        .collect();

    // Deduplicate background_colors.
    background_colors.sort();
    background_colors.dedup();

    // Crude color-scheme guess: if average tone of top color is dark, mark "dark".
    let color_scheme = if let Some(top) = colors.first() {
        if is_dark_hex(&top.value) {
            "dark".to_string()
        } else {
            "light".to_string()
        }
    } else {
        "unknown".to_string()
    };

    BrandingProfile {
        color_scheme,
        colors,
        fonts,
        background_colors,
        heading_sizes,
    }
}

fn collect_inline_css(html: &str) -> Vec<String> {
    let re = match Regex::new(r"(?is)<style[^>]*>(.*?)</style>") {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<String> = Vec::new();
    for cap in re.captures_iter(html) {
        out.push(cap[1].to_string());
    }
    out
}

fn first_color_token(value: &str) -> Option<String> {
    // Accept `#abc` / `#aabbcc` / `#aabbccdd` (lowercased to canonical form).
    let hex_re = Regex::new(r"#[0-9a-fA-F]{3,8}").ok()?;
    if let Some(c) = hex_re.captures(value) {
        let raw = c[0].to_ascii_lowercase();
        return Some(normalize_hex(&raw));
    }
    // Otherwise pick up the first bare color name.
    let names = [
        "white", "black", "red", "green", "blue", "yellow", "orange", "purple",
        "pink", "gray", "grey", "brown", "cyan", "magenta", "navy", "teal",
        "silver", "gold", "maroon",
    ];
    let lower = value.to_ascii_lowercase();
    for n in names {
        if lower.contains(n) {
            return Some(n.to_string());
        }
    }
    None
}

fn normalize_hex(s: &str) -> String {
    // Convert 3/4 digit hex to 6/8 form so duplicates collapse.
    let body = &s[1..];
    if body.len() == 3 || body.len() == 4 {
        let mut out = String::with_capacity(body.len() * 2);
        for c in body.chars() {
            out.push(c);
            out.push(c);
        }
        format!("#{}", out)
    } else if body.len() == 6 || body.len() == 8 {
        s.to_string()
    } else {
        s.to_string()
    }
}

fn split_font_family(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn is_dark_hex(s: &str) -> bool {
    let body = s.trim_start_matches('#');
    if body.len() != 6 && body.len() != 8 {
        return false;
    }
    let r = u8::from_str_radix(&body[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&body[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&body[4..6], 16).unwrap_or(255);
    let luminance = 0.299 * (r as f64) + 0.587 * (g as f64) + 0.114 * (b as f64);
    luminance < 96.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_top_colors_and_fonts() {
        let html = r#"
        <html><head>
        <style>
          body { background-color: #ffffff; color: #111111; font-family: 'Inter', sans-serif; }
          h1 { font-size: 36px; color: #111111; }
          h2 { font-size: 24px; }
          .accent { color: #ff6b35; background: #fff; }
          .accent2 { color: #ff6b35; }
        </style>
        </head></html>
        "#;
        let p = extract_branding(html);
        let top_color = p.colors.first().expect("at least one color");
        assert_eq!(top_color.value, "#111111");
        assert!(top_color.count >= 1);
        assert!(p.fonts.contains(&"Inter".to_string()));
        assert_eq!(p.heading_sizes.len(), 2);
        assert!(
            p.background_colors.iter().any(|c| c == "#ffffff" || c == "#fff"),
            "expected a white background color, got {:?}",
            p.background_colors
        );
    }

    #[test]
    fn uses_names_when_no_hex() {
        let html = "<style>p { color: red; } p { color: red; background-color: white; }</style>";
        let p = extract_branding(html);
        assert!(p.colors.iter().any(|c| c.value == "red" && c.count == 2));
        assert!(p.background_colors.contains(&"white".to_string()));
    }

    #[test]
    fn normalizes_short_hex() {
        assert_eq!(normalize_hex("#fff"), "#ffffff");
        assert_eq!(normalize_hex("#abc"), "#aabbcc");
        assert_eq!(normalize_hex("#aabbcc"), "#aabbcc");
    }

    #[test]
    fn scheme_guess_distinguishes_dark() {
        let dark_html = "<style>body { color: #000; }</style>";
        let light_html = "<style>body { color: #ffffff; }</style>";
        assert_eq!(extract_branding(dark_html).color_scheme, "dark");
        assert_eq!(extract_branding(light_html).color_scheme, "light");
    }
}
