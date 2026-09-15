use anyhow::Result;
use std::collections::HashSet;
use url::Url;

use crate::html_util::{find_ci, strip_html_tags};

/// Count characters that appear as Markdown link labels `[text](url)`.
fn markdown_link_text_chars(md: &str) -> usize {
    let mut total = 0usize;
    let bytes = md.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'['
            && let Some(close) = md[i + 1..].find(']') {
                let label = &md[i + 1..i + 1 + close];
                let after = &md[i + 1 + close..];
                if after.starts_with("](")
                    && let Some(end) = after[2..].find(')') {
                        let _ = end;
                        total += label.chars().count();
                        i += 1 + close + 2 + end + 1;
                        continue;
                    }
            }
        i += 1;
    }
    total
}

/// Parse `[text](url)` starting at `start` (index of `[`). Returns (text, end_index_after_close).
fn parse_md_link_at(md: &str, start: usize) -> Option<(String, usize)> {
    if start >= md.len() || md.as_bytes()[start] != b'[' {
        return None;
    }
    let close = md[start + 1..].find(']')?;
    let text = md[start + 1..start + 1 + close].to_string();
    let after_idx = start + 1 + close;
    let after = &md[after_idx..];
    if !after.starts_with("](") {
        return None;
    }
    let end = after[2..].find(')')?;
    Some((text, after_idx + 2 + end + 1))
}

/// True when any JSON-LD `@type` equals `type_name` (case-insensitive).
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

fn json_ld_type_contains(html: &str, type_name: &str) -> bool {
    crate::html_meta::iter_json_ld_blocks(html).any(|json| json_ld_value_is_type(&json, type_name))
}

fn json_ld_brand_name(product: &serde_json::Value) -> Option<String> {
    let brand = product.get("brand")?;
    if let Some(s) = brand.as_str() {
        return Some(s.to_string());
    }
    brand
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Normalize a Markdown block for deduplication comparison.
/// Collapses whitespace and trims, so blocks differing only in spacing are treated as duplicates.
fn normalize_block(block: &str) -> String {
    block.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract language from a `<code class="language-xxx">` tag's class attribute.
/// Returns None if no language class is found.
fn extract_language_class(tag: &str) -> Option<String> {
    let class_pos = find_ci(tag, "class=")?;
    let after = &tag[class_pos + 6..];
    let mut i = 0;
    while i < after.len() && after.as_bytes()[i].is_ascii_whitespace() {
        i += 1;
    }
    let quote = *after.as_bytes().get(i)? as char;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let val_start = i + 1;
    let val_end = after[val_start..].find(quote)? + val_start;
    let class_val = &after[val_start..val_end];
    for cls in class_val.split_whitespace() {
        if let Some(lang) = cls.strip_prefix("language-")
            && !lang.is_empty() {
                return Some(lang.to_string());
            }
    }
    None
}

fn push_plain_fragment(out: &mut String, text: &str, after_block: bool) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if !out.is_empty() && !after_block && !out.ends_with(' ') && !out.ends_with('\n') {
        out.push(' ');
    }
    out.push_str(trimmed);
}

fn trim_trailing_plain_space(out: &mut String) {
    while out.ends_with(' ') {
        out.pop();
    }
}

fn clean_plain_text(text: &str) -> String {
    let mut out = String::new();
    let mut blank_run = 0usize;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run <= 2 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    out.trim().to_string()
}
mod comments;
mod content;
mod structured;
mod strip;
#[cfg(test)]
mod tests;



/// Page-type-specific conversion tweaks (article / forum / product / page).
struct ExtractionProfile {
    /// Prefer Trafilatura-style main-content extraction.
    prefer_main_content: bool,
    /// Keep images even when the caller asked to strip them (product pages).
    prefer_images: bool,
}

impl ExtractionProfile {
    fn for_page_type(page_type: &str) -> Self {
        match page_type {
            "article" => Self {
                prefer_main_content: true,
                prefer_images: false,
            },
            "product" => Self {
                prefer_main_content: true,
                prefer_images: true,
            },
            // Keep full thread chrome; comments are appended separately.
            "forum" => Self {
                prefer_main_content: false,
                prefer_images: false,
            },
            _ => Self {
                prefer_main_content: false,
                prefer_images: false,
            },
        }
    }
}

/// Options for HTML → Markdown conversion.
#[derive(Clone, Debug)]
pub struct ConvertOptions {
    pub include_images: bool,
    pub keep_header: bool,
    pub main_content: bool,
    /// Prefer less noise (stricter main-content threshold, more boilerplate stripping).
    pub favor_precision: bool,
    /// Prefer more text (looser threshold, skip boilerplate stripping).
    pub favor_recall: bool,
    /// Append forum/thread comments when detected (default: true).
    pub include_comments: bool,
    /// Keep HTML tables in Markdown output (default: true).
    pub include_tables: bool,
    /// Keep Markdown links as `[text](url)` (default: true); when false, emit link text only.
    pub include_links: bool,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            include_images: false,
            keep_header: false,
            main_content: false,
            favor_precision: false,
            favor_recall: false,
            include_comments: true,
            include_tables: true,
            include_links: true,
        }
    }
}

/// Convert raw HTML to clean Markdown.
/// Strips scripts, styles, and non-essential markup to minimize token output.
pub struct PageToMarkdown;

impl PageToMarkdown {
    /// Convert HTML string to Markdown.
    /// When `include_images` is false, strips `<img>` tags to reduce token output.
    /// When `main_content` is true, extracts only the content of `<article>`, `<main>`, or `[role="main"]` elements.
    /// When `exclude_selectors` is non-empty, removes HTML elements matching the given CSS-like selectors (`.class` or `#id`) before conversion.
    /// Page-type profiles may also enable main-content extraction (article/product) or keep images (product).
    pub fn convert(
        html: &str,
        include_images: bool,
        keep_header: bool,
        main_content: bool,
        exclude_selectors: &[String],
    ) -> Result<String> {
        Self::convert_with(
            html,
            &ConvertOptions {
                include_images,
                keep_header,
                main_content,
                ..Default::default()
            },
            exclude_selectors,
        )
    }

    /// Convert with full Trafilatura-style options (precision/recall/comments).
    pub fn convert_with(
        html: &str,
        opts: &ConvertOptions,
        exclude_selectors: &[String],
    ) -> Result<String> {
        let original_html = html.to_string();
        let profile = ExtractionProfile::for_page_type(Self::detect_page_type(&original_html));
        let use_main = if opts.favor_recall {
            opts.main_content
        } else {
            opts.main_content || profile.prefer_main_content || opts.favor_precision
        };
        let use_images = opts.include_images || profile.prefer_images;
        let html = if use_main {
            Self::extract_main_content(&original_html, opts.favor_precision, opts.favor_recall)
        } else {
            original_html.clone()
        };
        let html = Self::strip_scripts_and_styles(&html);
        let html = Self::strip_iframe_tags(&html);
        let html = Self::strip_noise_tags(&html, opts.keep_header);
        let html = Self::strip_boilerplate_elements(&html);
        let html = Self::strip_html_comments(&html);
        let html = Self::strip_by_selectors(&html, exclude_selectors);
        let html = if opts.include_tables {
            html
        } else {
            Self::strip_table_tags(&html)
        };
        let languages = Self::extract_code_languages(&html);
        let html = if use_images {
            html
        } else {
            Self::strip_img_tags(&html)
        };
        let md = crate::html_to_md::parse_html(&html);
        let md = Self::inject_code_languages(&md, &languages);
        let md = Self::deduplicate_blocks(&md);
        let md = Self::clean_markdown_noise(&md);
        let md = Self::clean(&md);
        let md = Self::append_page_profile_extras(&original_html, md);
        let md = if opts.include_links {
            md
        } else {
            Self::strip_markdown_links(&md)
        };

        if opts.include_comments
            && let Some(comments) = Self::extract_comments(&original_html) {
                let comments = if opts.include_links {
                    comments
                } else {
                    Self::strip_markdown_links(&comments)
                };
                return Ok(format!("{}\n\n{}", md, comments));
            }
        Ok(md)
    }

    /// Estimate extraction confidence in `0.0..=1.0` from source HTML and resulting Markdown.
    /// Higher scores mean agents can trust the Markdown; lower scores suggest LLM fallback.
    pub fn extraction_quality(html: &str, markdown: &str) -> f64 {
        let md = markdown.trim();
        if md.is_empty() {
            return 0.0;
        }

        let mut score = 0.0;

        // Substantive length (cap contribution at ~500 chars)
        let len = md.chars().count();
        score += ((len as f64) / 500.0).min(1.0) * 0.30;

        // Structure signals in Markdown
        if md.lines().any(|l| l.starts_with('#')) {
            score += 0.15;
        }
        if md.contains("\n\n") || len > 120 {
            score += 0.10;
        }

        // Metadata / semantic HTML cues
        if find_ci(html, "<article").is_some()
            || find_ci(html, "<main").is_some()
            || find_ci(html, "role=\"main\"").is_some()
            || find_ci(html, "role='main'").is_some()
        {
            score += 0.15;
        }
        if find_ci(html, "<title").is_some() {
            score += 0.05;
        }
        if crate::html_meta::extract_meta_content(html, "name", "author").is_some()
            || crate::html_meta::extract_json_ld_field(html, "author").is_some()
        {
            score += 0.05;
        }
        if crate::html_meta::extract_meta_content(html, "property", "article:published_time")
            .is_some()
            || crate::html_meta::extract_json_ld_field(html, "datePublished").is_some()
        {
            score += 0.05;
        }

        // Prefer prose over link-heavy chrome
        let link_chars = markdown_link_text_chars(md);
        let link_ratio = if len == 0 {
            1.0
        } else {
            (link_chars as f64) / (len as f64)
        };
        score += (1.0 - link_ratio.min(1.0)) * 0.15;

        // Very short extractions are capped
        if len < 40 {
            score = score.min(0.30);
        }

        ((score * 100.0).round() / 100.0).clamp(0.0, 1.0)
    }

    /// Classify the page as `article`, `forum`, `product`, or `page`.
    pub fn detect_page_type(html: &str) -> &'static str {
        if Self::looks_like_forum_page(html) {
            return "forum";
        }
        if json_ld_type_contains(html, "Product") {
            return "product";
        }
        if find_ci(html, "<article").is_some()
            || json_ld_type_contains(html, "Article")
            || json_ld_type_contains(html, "NewsArticle")
            || json_ld_type_contains(html, "BlogPosting")
            || crate::html_meta::extract_meta_content(html, "property", "article:published_time")
                .is_some()
        {
            return "article";
        }
        "page"
    }

    /// Convert HTML to Markdown progressively, invoking `on_block` for each content block.
    pub fn convert_progressive(
        html: &str,
        include_images: bool,
        keep_header: bool,
        main_content: bool,
        exclude_selectors: &[String],
        on_block: impl FnMut(String),
    ) -> Result<()> {
        Self::convert_progressive_with(
            html,
            &ConvertOptions {
                include_images,
                keep_header,
                main_content,
                ..Default::default()
            },
            exclude_selectors,
            on_block,
        )
    }

    /// Progressive conversion with full Trafilatura-style options.
    pub fn convert_progressive_with(
        html: &str,
        opts: &ConvertOptions,
        exclude_selectors: &[String],
        mut on_block: impl FnMut(String),
    ) -> Result<()> {
        let original_html = html.to_string();
        let profile = ExtractionProfile::for_page_type(Self::detect_page_type(&original_html));
        let use_main = if opts.favor_recall {
            opts.main_content
        } else {
            opts.main_content || profile.prefer_main_content || opts.favor_precision
        };
        let use_images = opts.include_images || profile.prefer_images;
        let html = if use_main {
            Self::extract_main_content(&original_html, opts.favor_precision, opts.favor_recall)
        } else {
            original_html.clone()
        };
        let html = Self::strip_scripts_and_styles(&html);
        let html = Self::strip_iframe_tags(&html);
        let html = Self::strip_noise_tags(&html, opts.keep_header);
        let html = Self::strip_boilerplate_elements(&html);
        let html = Self::strip_html_comments(&html);
        let html = Self::strip_by_selectors(&html, exclude_selectors);
        let html = if opts.include_tables {
            html
        } else {
            Self::strip_table_tags(&html)
        };
        let languages = Self::extract_code_languages(&html);
        let html = if use_images {
            html
        } else {
            Self::strip_img_tags(&html)
        };

        let mut lang_idx = 0usize;
        let mut seen = HashSet::new();
        crate::html_to_md::parse_html_progressive(&html, |block| {
            let block = Self::inject_code_languages_from(&block, &languages, &mut lang_idx);
            let block = Self::clean_markdown_noise(&block);
            let block = Self::clean(&block);
            let block = if opts.include_links {
                block
            } else {
                Self::strip_markdown_links(&block)
            };
            if block.is_empty() {
                return;
            }
            let key = normalize_block(&block);
            if !seen.insert(key) {
                return;
            }
            on_block(block);
        });

        if let Some(details) = Self::extract_product_details(&original_html) {
            on_block(details);
        }
        if opts.include_comments
            && let Some(comments) = Self::extract_comments(&original_html) {
                let comments = if opts.include_links {
                    comments
                } else {
                    Self::strip_markdown_links(&comments)
                };
                on_block(comments);
            }
        Ok(())
    }

    /// Append product JSON-LD details when the page is a product listing.
    fn append_page_profile_extras(html: &str, md: String) -> String {
        if let Some(details) = Self::extract_product_details(html) {
            if md.is_empty() {
                details
            } else {
                format!("{}\n\n{}", md, details)
            }
        } else {
            md
        }
    }

    /// Build a short Markdown summary from JSON-LD `Product` fields when present.
    fn extract_product_details(html: &str) -> Option<String> {
        if Self::detect_page_type(html) != "product" {
            return None;
        }
        let mut name = None;
        let mut brand = None;
        let mut sku = None;
        let mut price = None;
        let mut currency = None;

        for json in crate::html_meta::iter_json_ld_blocks(html) {
            if !json_ld_value_is_type(&json, "Product") {
                continue;
            }
            if name.is_none() {
                name = json.get("name").and_then(|v| v.as_str()).map(str::to_string);
            }
            if brand.is_none() {
                brand = json_ld_brand_name(&json);
            }
            if sku.is_none() {
                sku = json
                    .get("sku")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
            }
            if let Some(offers) = json.get("offers") {
                let offer = if let Some(arr) = offers.as_array() {
                    arr.first()
                } else {
                    Some(offers)
                };
                if let Some(o) = offer {
                    if price.is_none() {
                        price = o.get("price").and_then(|v| {
                            v.as_str()
                                .map(str::to_string)
                                .or_else(|| v.as_f64().map(|n| n.to_string()))
                                .or_else(|| v.as_i64().map(|n| n.to_string()))
                        });
                    }
                    if currency.is_none() {
                        currency = o
                            .get("priceCurrency")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                    }
                }
            }
        }

        let mut lines = Vec::new();
        if let Some(n) = name {
            lines.push(format!("- **Name**: {}", n));
        }
        if let Some(b) = brand {
            lines.push(format!("- **Brand**: {}", b));
        }
        if let Some(s) = sku {
            lines.push(format!("- **SKU**: {}", s));
        }
        match (price, currency) {
            (Some(p), Some(c)) => lines.push(format!("- **Price**: {} {}", p, c)),
            (Some(p), None) => lines.push(format!("- **Price**: {}", p)),
            _ => {}
        }
        if lines.is_empty() {
            return None;
        }
        Some(format!("## Product details\n\n{}", lines.join("\n")))
    }

    /// Convert relative URLs in Markdown links to absolute URLs using the given base URL.
    /// Processes `[text](url)` and `![alt](url)` patterns, leaving already-absolute URLs unchanged.
    pub fn absolutize_links(md: &str, base_url: &str) -> String {
        let base = match Url::parse(base_url) {
            Ok(u) => u,
            Err(_) => return md.to_string(),
        };

        let mut result = String::with_capacity(md.len());
        let mut i = 0;

        while i < md.len() {
            // Look for `](` pattern which indicates a Markdown link
            let bytes = md.as_bytes();
            if bytes[i] == b']' && i + 1 < md.len() && bytes[i + 1] == b'(' {
                // Find the closing ')'
                if let Some(close) = md[i + 2..].find(')') {
                    let url_start = i + 2;
                    let url_end = i + 2 + close;
                    let raw_url = &md[url_start..url_end];

                    // Skip empty URLs and anchor-only links
                    if raw_url.is_empty() || raw_url.starts_with('#') {
                        result.push_str(&md[i..=url_end]);
                        i = url_end + 1;
                        continue;
                    }

                    // Try to resolve the URL against the base
                    if let Ok(absolved) = base.join(raw_url) {
                        result.push_str("](");
                        result.push_str(absolved.as_str());
                        result.push(')');
                        i = url_end + 1;
                        continue;
                    }

                    // If resolution fails, keep the original
                    result.push_str(&md[i..=url_end]);
                    i = url_end + 1;
                    continue;
                }
            }
            // Push one character (UTF-8 safe) from the current position
            let ch = md[i..].chars().next().unwrap();
            result.push(ch);
            i += ch.len_utf8();
        }

        result
    }

    /// Strip Markdown syntax and return plain text for archival or NLP pipelines.
    pub fn to_plain_text(md: &str) -> String {
        use pulldown_cmark::{Event, Options, Parser, TagEnd};

        let parser = Parser::new_ext(md, Options::empty());
        let mut out = String::with_capacity(md.len());
        let mut after_block = false;

        for event in parser {
            match event {
                Event::Text(text) | Event::Code(text) => {
                    push_plain_fragment(&mut out, &text, after_block);
                    after_block = false;
                }
                Event::SoftBreak => {
                    if !out.is_empty() && !out.ends_with(' ') && !out.ends_with('\n') {
                        out.push(' ');
                    }
                }
                Event::HardBreak => {
                    trim_trailing_plain_space(&mut out);
                    out.push('\n');
                    after_block = true;
                }
                Event::Rule => {
                    trim_trailing_plain_space(&mut out);
                    out.push_str("\n---\n");
                    after_block = true;
                }
                Event::End(TagEnd::Paragraph)
                | Event::End(TagEnd::Heading(_))
                | Event::End(TagEnd::BlockQuote(_))
                | Event::End(TagEnd::CodeBlock)
                | Event::End(TagEnd::Table)
                | Event::End(TagEnd::TableHead)
                | Event::End(TagEnd::TableRow)
                | Event::End(TagEnd::TableCell)
                | Event::End(TagEnd::Item)
                | Event::End(TagEnd::List(_)) => {
                    trim_trailing_plain_space(&mut out);
                    out.push('\n');
                    after_block = true;
                }
                _ => {}
            }
        }

        clean_plain_text(&out)
    }
}
