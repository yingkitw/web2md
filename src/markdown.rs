use anyhow::Result;
use url::Url;

use crate::html_util::find_ci;

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
        if let Some(lang) = cls.strip_prefix("language-") {
            if !lang.is_empty() {
                return Some(lang.to_string());
            }
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

/// Convert raw HTML to clean Markdown.
/// Strips scripts, styles, and non-essential markup to minimize token output.
pub struct PageToMarkdown;

impl PageToMarkdown {
    /// Convert HTML string to Markdown.
    /// When `include_images` is false, strips `<img>` tags to reduce token output.
    /// When `main_content` is true, extracts only the content of `<article>`, `<main>`, or `[role="main"]` elements.
    /// When `exclude_selectors` is non-empty, removes HTML elements matching the given CSS-like selectors (`.class` or `#id`) before conversion.
    pub fn convert(html: &str, include_images: bool, keep_header: bool, main_content: bool, exclude_selectors: &[String]) -> Result<String> {
        let original_html = html.to_string();
        let html = if main_content {
            Self::extract_main_content(&original_html)
        } else {
            original_html.clone()
        };
        let html = Self::strip_scripts_and_styles(&html);
        let html = Self::strip_iframe_tags(&html);
        let html = Self::strip_noise_tags(&html, keep_header);
        let html = Self::strip_html_comments(&html);
        let html = Self::strip_by_selectors(&html, exclude_selectors);
        let languages = Self::extract_code_languages(&html);
        let html = if include_images { html } else { Self::strip_img_tags(&html) };
        let md = crate::html_to_md::parse_html(&html);
        let md = Self::inject_code_languages(&md, &languages);
        let md = Self::deduplicate_blocks(&md);
        let md = Self::clean(&md);

        // Append extracted comments for forum/thread pages
        if let Some(comments) = Self::extract_comments(&original_html) {
            Ok(format!("{}\n\n{}", md, comments))
        } else {
            Ok(md)
        }
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

    /// Remove `<img>` tags (case-insensitive). Self-closing (`/>`) or plain (`>`) both handled.
    fn strip_img_tags(html: &str) -> String {
        let mut out = String::with_capacity(html.len());
        let mut i = 0;
        while i < html.len() {
            if let Some(start) = find_ci(&html[i..], "<img") {
                let start = i + start;
                if let Some(end) = html[start..].find('>') {
                    out.push_str(&html[i..start]);
                    i = start + end + 1;
                    continue;
                }
            }
            out.push_str(&html[i..]);
            break;
        }
        out
    }

    /// Remove `<script>` and `<style>` blocks (case-insensitive, non-greedy)
    fn strip_scripts_and_styles(html: &str) -> String {
        let mut out = String::with_capacity(html.len());
        let mut i = 0;
        while i < html.len() {
            let script_start = find_ci(&html[i..], "<script").map(|p| i + p);
            let style_start = find_ci(&html[i..], "<style").map(|p| i + p);
            let next = match (script_start, style_start) {
                (Some(s), Some(st)) => Some((s.min(st), s == s.min(st))),
                (Some(s), None) => Some((s, true)),
                (None, Some(st)) => Some((st, false)),
                (None, None) => None,
            };
            if let Some((start, is_script)) = next {
                let close = if is_script { "</script>" } else { "</style>" };
                if let Some(end) = find_ci(&html[start..], close) {
                    out.push_str(&html[i..start]);
                    i = start + end + close.len();
                    continue;
                }
            }
            out.push_str(&html[i..]);
            break;
        }
        out
    }

    /// Remove non-content HTML tags: `<nav>`, `<footer>`, `<aside>`, `<noscript>`, `<form>`.
    /// These are navigation, structural, or interactive elements that add noise to Markdown output.
    fn strip_noise_tags(html: &str, keep_header: bool) -> String {
        let mut tags = vec![
            ("nav", "</nav>"),
            ("footer", "</footer>"),
            ("aside", "</aside>"),
            ("noscript", "</noscript>"),
            ("form", "</form>"),
        ];
        if !keep_header {
            tags.push(("header", "</header>"));
        }
        let mut result = html.to_string();
        for (open, close) in &tags {
            result = Self::strip_tag_pair(&result, open, close);
        }
        result
    }

    /// Generic case-insensitive removal of `<tag>...</tag>` blocks.
    fn strip_tag_pair(html: &str, tag: &str, close_tag: &str) -> String {
        let open = format!("<{}", tag);
        let mut out = String::with_capacity(html.len());
        let mut i = 0;
        while i < html.len() {
            if let Some(start) = find_ci(&html[i..], &open) {
                let start = i + start;
                if let Some(end) = find_ci(&html[start..], close_tag) {
                    out.push_str(&html[i..start]);
                    i = start + end + close_tag.len();
                    continue;
                }
                // unclosed: skip to next `>` to avoid eating the rest of the document
                if let Some(gt) = html[start..].find('>') {
                    out.push_str(&html[i..start]);
                    i = start + gt + 1;
                    continue;
                }
            }
            out.push_str(&html[i..]);
            break;
        }
        out
    }

    /// Remove HTML comments `<!-- ... -->` (case-insensitive on delimiters).
    fn strip_html_comments(html: &str) -> String {
        let mut out = String::with_capacity(html.len());
        let mut i = 0;
        while i < html.len() {
            if let Some(start) = find_ci(&html[i..], "<!--") {
                let start = i + start;
                if let Some(end) = find_ci(&html[start..], "-->") {
                    out.push_str(&html[i..start]);
                    i = start + end + 3;
                    continue;
                }
            }
            out.push_str(&html[i..]);
            break;
        }
        out
    }

    /// Remove HTML elements matching CSS-like selectors from the HTML.
    /// Supports `.classname` (class selector) and `#id` (ID selector).
    /// Multiple selectors can be provided; each is applied in order.
    fn strip_by_selectors(html: &str, selectors: &[String]) -> String {
        let mut result = html.to_string();
        for selector in selectors {
            let trimmed = selector.trim();
            if let Some(class_name) = trimmed.strip_prefix('.') {
                if !class_name.is_empty() {
                    result = Self::strip_elements_by_attr(&result, "class", class_name);
                }
            } else if let Some(id_val) = trimmed.strip_prefix('#') {
                if !id_val.is_empty() {
                    result = Self::strip_elements_by_attr(&result, "id", id_val);
                }
            }
        }
        result
    }

    /// Remove HTML elements where an attribute contains the given value.
    /// Matches `class="foo bar"` for value "foo" (word-boundary match),
    /// and `id="exact"` for value "exact" (exact match).
    fn strip_elements_by_attr(html: &str, attr: &str, value: &str) -> String {
        let mut out = String::with_capacity(html.len());
        let mut i = 0;
        let lower_attr = format!("{}=\"", attr);

        while i < html.len() {
            // Find the next opening tag
            if let Some(rel_start) = html[i..].find('<') {
                let tag_start = i + rel_start;
                // Find the end of this opening tag
                if let Some(rel_end) = html[tag_start..].find('>') {
                    let tag_end = tag_start + rel_end;
                    let tag_content = &html[tag_start..=tag_end];
                    let tag_lower = tag_content.to_ascii_lowercase();

                    // Check if this tag has the attribute we're looking for
                    if let Some(attr_pos) = find_ci(&tag_lower, &lower_attr) {
                        let val_start = attr_pos + lower_attr.len();
                        if let Some(quote_end) = tag_lower[val_start..].find('"') {
                            let attr_val = &tag_lower[val_start..val_start + quote_end];
                            let matches = if attr == "id" {
                                attr_val == value.to_ascii_lowercase()
                            } else {
                                attr_val.split_whitespace().any(|w| w == value.to_ascii_lowercase().as_str())
                            };

                            if matches {
                                // Determine the tag name
                                let after_lt = &tag_content[1..];
                                let tag_name: String = after_lt
                                    .chars()
                                    .take_while(|c| c.is_alphanumeric())
                                    .collect();

                                // Copy everything before this tag
                                out.push_str(&html[i..tag_start]);

                                if tag_name.is_empty() {
                                    i = tag_end + 1;
                                    continue;
                                }

                                let void_tags = ["br", "hr", "img", "input", "meta", "link", "area", "base", "col", "embed", "source", "track", "wbr"];
                                if void_tags.contains(&tag_name.as_str()) {
                                    i = tag_end + 1;
                                    continue;
                                }

                                // Find the matching close tag
                                let close_tag = format!("</{}", tag_name);
                                if let Some(close_pos) = find_ci(&html[tag_end + 1..], &close_tag) {
                                    let close_start = tag_end + 1 + close_pos;
                                    if let Some(close_end) = html[close_start..].find('>') {
                                        i = close_start + close_end + 1;
                                        continue;
                                    }
                                }

                                // No close tag found, just remove the opening tag
                                i = tag_end + 1;
                                continue;
                            }
                        }
                    }

                    // Tag doesn't match — copy it and advance past it
                    out.push_str(&html[i..tag_end + 1]);
                    i = tag_end + 1;
                    continue;
                }
            }
            out.push_str(&html[i..]);
            break;
        }
        out
    }

    /// Extract language annotations from `<code class="language-xxx">` tags, in document order.
    /// Returns a list of languages corresponding to code blocks in the HTML.
    fn extract_code_languages(html: &str) -> Vec<String> {
        let mut languages = Vec::new();
        let mut i = 0;
        while i < html.len() {
            if let Some(pos) = find_ci(&html[i..], "<code") {
                let pos = i + pos;
                if let Some(gt) = html[pos..].find('>') {
                    let tag = &html[pos..=pos + gt];
                    if let Some(lang) = extract_language_class(tag) {
                        languages.push(lang);
                    }
                    i = pos + gt + 1;
                    continue;
                }
            }
            break;
        }
        languages
    }

    /// Inject language annotations into fenced code blocks that lack them.
    /// Pairs with `extract_code_languages` — languages are matched in order.
    fn inject_code_languages(md: &str, languages: &[String]) -> String {
        if languages.is_empty() {
            return md.to_string();
        }
        let mut result = String::with_capacity(md.len());
        let mut lang_idx = 0;
        let lines: Vec<&str> = md.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim();
            if trimmed == "```" && lang_idx < languages.len() {
                result.push_str(&format!("```{}", languages[lang_idx]));
                lang_idx += 1;
            } else {
                result.push_str(line);
            }
            if i + 1 < lines.len() {
                result.push('\n');
            }
            i += 1;
        }
        result
    }

    /// Remove `<iframe>` tags including their closing tag (case-insensitive).
    fn strip_iframe_tags(html: &str) -> String {
        let mut out = String::with_capacity(html.len());
        let mut i = 0;
        while i < html.len() {
            if let Some(start) = find_ci(&html[i..], "<iframe") {
                let start = i + start;
                if let Some(end) = find_ci(&html[start..], "</iframe>") {
                    out.push_str(&html[i..start]);
                    i = start + end + "</iframe>".len();
                    continue;
                }
                // self-closing or unclosed: find next >
                if let Some(end) = html[start..].find('>') {
                    out.push_str(&html[i..start]);
                    i = start + end + 1;
                    continue;
                }
            }
            out.push_str(&html[i..]);
            break;
        }
        out
    }

    /// Extract the main content area from HTML.
    /// Trafilatura-style fallback chain: collect candidates from semantic tags, block
    /// readability, and paragraph clustering; pick the highest-scoring result, then
    /// strip link-heavy boilerplate paragraphs (jusText-style).
    fn extract_main_content(html: &str) -> String {
        let mut candidates: Vec<(String, i64)> = Vec::new();
        candidates.extend(Self::semantic_content_candidates(html));

        for (content, _) in Self::find_top_level_blocks(html) {
            candidates.push((content.clone(), Self::content_quality_score(&content)));
        }

        if let Some(cluster) = Self::paragraph_cluster_html(html) {
            let score = Self::content_quality_score(&cluster);
            candidates.push((cluster, score));
        }

        let best = candidates.into_iter().max_by_key(|(_, score)| *score);
        if let Some((content, score)) = best {
            if score > 100 {
                return Self::strip_boilerplate_paragraphs(&content);
            }
        }

        html.to_string()
    }

    /// Score HTML fragments for main-content selection (text density minus link density).
    fn content_quality_score(html: &str) -> i64 {
        let text_len = Self::count_text_chars(html) as i64;
        let link_text_len = Self::count_link_text_chars(html) as i64;
        text_len - link_text_len
    }

    /// Collect inner HTML from semantic main-content tags.
    fn semantic_content_candidates(html: &str) -> Vec<(String, i64)> {
        const SEMANTIC_BONUS: i64 = 150;
        let mut candidates = Vec::new();
        for (tag, close_tag) in [("article", "</article>"), ("main", "</main>")] {
            if let Some(content) = Self::extract_tag_inner(html, tag, close_tag) {
                candidates.push((
                    content.clone(),
                    Self::content_quality_score(&content) + SEMANTIC_BONUS,
                ));
            }
        }
        let mut i = 0;
        while i < html.len() {
            if let Some(pos) = find_ci(&html[i..], "<div") {
                let pos = i + pos;
                if let Some(gt) = html[pos..].find('>') {
                    let tag = &html[pos..=pos + gt];
                    if find_ci(tag, "role=\"main\"").is_some() || find_ci(tag, "role='main'").is_some() {
                        let content_start = pos + gt + 1;
                        if let Some(end) = find_ci(&html[content_start..], "</div>") {
                            let content = html[content_start..content_start + end].to_string();
                            candidates.push((
                                content.clone(),
                                Self::content_quality_score(&content) + SEMANTIC_BONUS,
                            ));
                        }
                    }
                    i = pos + gt + 1;
                    continue;
                }
            }
            break;
        }
        candidates
    }

    fn extract_tag_inner(html: &str, tag: &str, close_tag: &str) -> Option<String> {
        let start = find_ci(html, &format!("<{tag}"))?;
        let gt = html[start..].find('>')?;
        let content_start = start + gt + 1;
        let end = find_ci(&html[content_start..], close_tag)?;
        Some(html[content_start..content_start + end].to_string())
    }

    /// Returns the highest-scoring contiguous paragraph window, if above threshold.
    fn paragraph_cluster_html(html: &str) -> Option<String> {
        let paragraphs = Self::find_paragraph_blocks(html);
        if paragraphs.is_empty() {
            return None;
        }

        let scores: Vec<usize> = paragraphs
            .iter()
            .map(|(content, _, _)| Self::count_text_chars(content))
            .collect();

        let window_size = 5.min(paragraphs.len());
        let mut best_start = 0;
        let mut best_score = 0usize;

        for start in 0..=paragraphs.len() - window_size {
            let score: usize = scores[start..start + window_size].iter().sum();
            if score > best_score {
                best_score = score;
                best_start = start;
            }
        }

        if best_score < 100 {
            return None;
        }

        let end = (best_start + window_size).min(paragraphs.len());
        let html_start = paragraphs[best_start].1;
        let html_end = paragraphs[end - 1].2;
        Some(html[html_start..html_end].to_string())
    }

    /// Remove short or link-heavy `<p>` blocks (jusText-style boilerplate filter).
    fn strip_boilerplate_paragraphs(html: &str) -> String {
        let paragraphs = Self::find_paragraph_blocks(html);
        if paragraphs.is_empty() {
            return html.to_string();
        }

        let mut remove = vec![false; paragraphs.len()];
        for (i, (inner, _, _)) in paragraphs.iter().enumerate() {
            let text_len = Self::count_text_chars(inner) as i64;
            let link_len = Self::count_link_text_chars(inner) as i64;
            if text_len < 30 || (text_len > 0 && link_len * 2 >= text_len) {
                remove[i] = true;
            }
        }

        if !remove.iter().any(|&r| r) {
            return html.to_string();
        }

        let mut out = String::with_capacity(html.len());
        let mut last = 0;
        for (i, (_, start, end)) in paragraphs.iter().enumerate() {
            if !remove[i] {
                out.push_str(&html[last..*start]);
                out.push_str(&html[*start..*end]);
            } else {
                out.push_str(&html[last..*start]);
            }
            last = *end;
        }
        out.push_str(&html[last..]);
        out
    }

    /// Find all `<p>` blocks in the HTML.
    /// Returns a list of (inner_html, start_byte, end_byte) tuples.
    fn find_paragraph_blocks(html: &str) -> Vec<(String, usize, usize)> {
        let mut blocks = Vec::new();
        let mut i = 0;
        while i < html.len() {
            if let Some(pos) = find_ci(&html[i..], "<p") {
                let pos = i + pos;
                // Ensure this is a <p> tag, not <pre>, <param>, etc.
                let after = &html[pos + 2..];
                let next_char = after.chars().next();
                if next_char != Some(' ') && next_char != Some('>') && next_char != Some('\t') && next_char != Some('\n') && next_char != Some('\r') {
                    i = pos + 2;
                    continue;
                }
                if let Some(gt) = html[pos..].find('>') {
                    let content_start = pos + gt + 1;
                    if let Some(end) = find_ci(&html[content_start..], "</p>") {
                        let end_abs = content_start + end;
                        blocks.push((
                            html[content_start..end_abs].to_string(),
                            pos,
                            end_abs + "</p>".len(),
                        ));
                        i = end_abs + "</p>".len();
                        continue;
                    }
                    // Unclosed <p>: take everything until the next <p> or block-level tag
                    i = content_start;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        blocks
    }

    /// Find top-level `<div>` and `<section>` blocks in HTML.
    /// Returns a list of (inner_html, tag_name) pairs, respecting nesting depth.
    fn find_top_level_blocks(html: &str) -> Vec<(String, &'static str)> {
        let mut blocks = Vec::new();
        for tag in &["div", "section"] {
            let open = format!("<{}", tag);
            let close = format!("</{}>", tag);
            let mut i = 0;
            while i < html.len() {
                if let Some(pos) = find_ci(&html[i..], &open) {
                    let pos = i + pos;
                    if let Some(gt) = html[pos..].find('>') {
                        let content_start = pos + gt + 1;
                        // Find matching close tag, respecting nesting
                        if let Some(end) = Self::find_matching_close(html, content_start, &open, &close) {
                            blocks.push((html[content_start..end].to_string(), *tag));
                            i = end + close.len();
                            continue;
                        }
                    }
                    i = pos + 1;
                } else {
                    break;
                }
            }
        }
        blocks
    }

    /// Find the matching close tag for an HTML block, respecting nesting depth.
    fn find_matching_close(html: &str, start: usize, open: &str, close: &str) -> Option<usize> {
        let mut depth = 1;
        let mut i = start;
        while i < html.len() {
            let rest = &html[i..];
            let next_open = find_ci(rest, open).map(|p| i + p);
            let next_close = find_ci(rest, close).map(|p| i + p);
            match (next_open, next_close) {
                (Some(o), Some(c)) if o < c => {
                    depth += 1;
                    i = o + open.len();
                }
                (_, Some(c)) => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(c);
                    }
                    i = c + close.len();
                }
                _ => return None,
            }
        }
        None
    }

    /// Count visible text characters in HTML (excluding tag content and whitespace).
    fn count_text_chars(html: &str) -> usize {
        let mut count = 0;
        let mut in_tag = false;
        for c in html.chars() {
            if c == '<' {
                in_tag = true;
            } else if c == '>' {
                in_tag = false;
            } else if !in_tag && !c.is_ascii_whitespace() {
                count += 1;
            }
        }
        count
    }

    /// Count text characters inside `<a>` tags (used as a proxy for link/navigation density).
    fn count_link_text_chars(html: &str) -> usize {
        let mut count = 0;
        let mut i = 0;
        while i < html.len() {
            if let Some(pos) = find_ci(&html[i..], "<a") {
                let pos = i + pos;
                // Ensure this is an <a> tag, not <article>, <aside>, etc.
                let after = &html[pos + 2..];
                let next_char = after.chars().next();
                if next_char != Some(' ') && next_char != Some('>') && next_char != Some('\t') && next_char != Some('\n') && next_char != Some('\r') {
                    i = pos + 2;
                    continue;
                }
                if let Some(gt) = html[pos..].find('>') {
                    let content_start = pos + gt + 1;
                    if let Some(end) = find_ci(&html[content_start..], "</a>") {
                        let end_abs = content_start + end;
                        let link_text = &html[content_start..end_abs];
                        count += Self::count_text_chars(link_text);
                        i = end_abs + "</a>".len();
                        continue;
                    }
                }
                i = pos + 1;
            } else {
                break;
            }
        }
        count
    }

    /// Remove duplicate paragraph-level blocks from Markdown.
    /// Blocks are separated by blank lines. Only substantial blocks (>20 chars of normalized text)
    /// are deduplicated — short blocks like headings or single words are kept.
    fn deduplicate_blocks(md: &str) -> String {
        use std::collections::HashSet;

        let mut seen: HashSet<String> = HashSet::new();
        let mut result = String::with_capacity(md.len());
        let mut current_block = String::new();

        for line in md.lines() {
            if line.trim().is_empty() {
                if !current_block.is_empty() {
                    let normalized = normalize_block(&current_block);
                    if normalized.len() <= 20 || seen.insert(normalized) {
                        result.push_str(&current_block);
                        result.push('\n');
                    }
                    current_block.clear();
                }
                result.push('\n');
            } else {
                current_block.push_str(line);
                current_block.push('\n');
            }
        }

        if !current_block.is_empty() {
            let normalized = normalize_block(&current_block);
            if normalized.len() <= 20 || seen.insert(normalized) {
                result.push_str(&current_block);
            }
        }

        result
    }

    /// Post-process: collapse excessive whitespace and trim
    fn clean(md: &str) -> String {
        let mut out = String::new();
        let mut blank_lines = 0;

        for line in md.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                blank_lines += 1;
                if blank_lines <= 2 {
                    out.push('\n');
                }
            } else {
                blank_lines = 0;
                out.push_str(trimmed);
                out.push('\n');
            }
        }
        out.trim().to_string()
    }

    /// Detect if a page looks like a forum/thread with comments.
    /// Returns true if multiple comment-like containers are found.
    fn looks_like_forum_page(html: &str) -> bool {
        let comment_indicators = [
            "class=\"comment",
            "class='comment",
            "class=\"post-body",
            "class='post-body",
            "class=\"comment-body",
            "class='comment-body",
            "class=\"comment-content",
            "class='comment-content",
            "class=\"message-body",
            "class='message-body",
            "id=\"comment-",
            "id='comment-",
            "data-comment-id",
            "data-testid=\"comment",
        ];
        let mut count = 0;
        for indicator in &comment_indicators {
            count += find_ci(html, indicator).map(|_| 1).unwrap_or(0);
            if count >= 2 {
                return true;
            }
        }
        false
    }

    /// Extract comments from forum/thread pages.
    /// Detects comment containers, extracts author and text, and formats as Markdown.
    /// Returns None if no comments are found or the page doesn't look like a forum.
    fn extract_comments(html: &str) -> Option<String> {
        if !Self::looks_like_forum_page(html) {
            return None;
        }

        let comments = Self::find_comment_blocks(html);
        if comments.len() < 2 {
            return None;
        }

        let mut out = String::new();
        out.push_str("## Comments\n\n");
        for (author, text, depth) in &comments {
            let indent = "  ".repeat(*depth);
            if let Some(a) = author {
                out.push_str(&format!("{}**{}**:\n\n", indent, a));
            }
            let comment_md = crate::html_to_md::parse_html(text);
            let comment_md = Self::clean(&comment_md);
            for line in comment_md.lines() {
                out.push_str(&format!("{}> {}\n", indent, line));
            }
            out.push('\n');
        }
        Some(out.trim().to_string())
    }

    /// Find comment blocks in HTML.
    /// Returns a list of (author, inner_html, nesting_depth) tuples.
    fn find_comment_blocks(html: &str) -> Vec<(Option<String>, String, usize)> {
        let mut blocks = Vec::new();
        let comment_patterns = [
            "class=\"comment",
            "class='comment",
            "id=\"comment-",
            "id='comment-",
            "data-testid=\"comment",
        ];

        let mut i = 0;
        while i < html.len() {
            let mut found = false;
            for pattern in &comment_patterns {
                if let Some(pos) = find_ci(&html[i..], pattern) {
                    let pos = i + pos;
                    // Find the enclosing tag start (walk back to '<')
                    let tag_start = html[..pos].rfind('<').unwrap_or(pos);
                    if let Some(gt) = html[tag_start..].find('>') {
                        let tag = &html[tag_start..=tag_start + gt];
                        let tag_name = Self::extract_tag_name(tag);
                        let close_tag = format!("</{}>", tag_name);
                        let content_start = tag_start + gt + 1;
                        if let Some(end) = Self::find_matching_close(
                            html,
                            content_start,
                            &format!("<{}", tag_name),
                            &close_tag,
                        ) {
                            let inner = &html[content_start..end];
                            // Check the container tag for data-author first, then inner HTML
                            let author = Self::extract_comment_author(tag)
                                .or_else(|| Self::extract_comment_author(inner));
                            let depth = Self::estimate_nesting_depth(html, tag_start);
                            let text = Self::extract_comment_text(inner);
                            if !text.is_empty() {
                                blocks.push((author, text, depth));
                            }
                            i = end + close_tag.len();
                            found = true;
                            break;
                        }
                    }
                }
            }
            if !found {
                break;
            }
        }
        blocks
    }

    /// Extract the tag name from an opening tag string like `<div class="...">`.
    fn extract_tag_name(tag: &str) -> String {
        let after_lt = &tag[1..];
        let end = after_lt
            .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
            .unwrap_or(after_lt.len());
        after_lt[..end].to_string()
    }

    /// Extract author name from comment inner HTML.
    /// Looks for common author patterns: `<a class="author">`, `<span class="author">`,
    /// `data-author="..."`, `<cite>`, or `<address>`.
    fn extract_comment_author(html: &str) -> Option<String> {
        // data-author attribute
        if let Some(pos) = find_ci(html, "data-author=") {
            let after = &html[pos + 12..];
            if let Some(val) = Self::extract_quoted_value(after) {
                return Some(val);
            }
        }
        // <a class="author"> or <span class="author">
        for tag in &["<a", "<span", "<div", "<strong", "<b"] {
            let mut i = 0;
            while i < html.len() {
                if let Some(pos) = find_ci(&html[i..], tag) {
                    let pos = i + pos;
                    if let Some(gt) = html[pos..].find('>') {
                        let tag_str = &html[pos..=pos + gt];
                        if find_ci(tag_str, "author").is_some()
                            || find_ci(tag_str, "username").is_some()
                            || find_ci(tag_str, "user-name").is_some()
                        {
                            let content_start = pos + gt + 1;
                            let close = format!("</{}", &tag[1..]);
                            if let Some(end) = find_ci(&html[content_start..], &close) {
                                let author_html = &html[content_start..content_start + end];
                                let author = Self::strip_tags(author_html);
                                let author = author.trim().to_string();
                                if !author.is_empty() {
                                    return Some(author);
                                }
                            }
                        }
                        i = pos + gt + 1;
                        continue;
                    }
                }
                break;
            }
        }
        // <cite> or <address>
        for tag in &["<cite", "<address"] {
            if let Some(pos) = find_ci(html, tag) {
                if let Some(gt) = html[pos..].find('>') {
                    let content_start = pos + gt + 1;
                    let close = format!("</{}", &tag[1..]);
                    if let Some(end) = find_ci(&html[content_start..], &close) {
                        let author = Self::strip_tags(&html[content_start..content_start + end]);
                        let author = author.trim().to_string();
                        if !author.is_empty() {
                            return Some(author);
                        }
                    }
                }
            }
        }
        None
    }

    /// Extract the main text content from a comment block, excluding metadata.
    /// Looks for content containers first, then falls back to the full inner HTML.
    fn extract_comment_text(html: &str) -> String {
        let content_patterns = [
            "class=\"comment-body",
            "class='comment-body",
            "class=\"comment-content",
            "class='comment-content",
            "class=\"post-body",
            "class='post-body",
            "class=\"message-body",
            "class='message-body",
            "class=\"usertext-body",
            "class='usertext-body",
            "class=\"md",
            "class='md",
        ];
        for pattern in &content_patterns {
            if let Some(pos) = find_ci(html, pattern) {
                let tag_start = html[..pos].rfind('<').unwrap_or(pos);
                if let Some(gt) = html[tag_start..].find('>') {
                    let tag = &html[tag_start..=tag_start + gt];
                    let tag_name = Self::extract_tag_name(tag);
                    let close_tag = format!("</{}>", tag_name);
                    let content_start = tag_start + gt + 1;
                    if let Some(end) =
                        Self::find_matching_close(html, content_start, &format!("<{}", tag_name), &close_tag)
                    {
                        return html[content_start..end].to_string();
                    }
                }
            }
        }
        // Fallback: return the full inner HTML
        html.to_string()
    }

    /// Estimate nesting depth by counting ancestor comment containers.
    fn estimate_nesting_depth(html: &str, pos: usize) -> usize {
        let before = &html[..pos];
        let mut depth: usize = 0;
        let mut i = 0;
        while i < before.len() {
            let mut found = false;
            for pattern in &["class=\"comment", "class='comment", "id=\"comment-"] {
                if let Some(p) = find_ci(&before[i..], pattern) {
                    let p = i + p;
                    let tag_start = before[..p].rfind('<').unwrap_or(p);
                    if let Some(gt) = before[tag_start..].find('>') {
                        let tag = &before[tag_start..=tag_start + gt];
                        let tag_name = Self::extract_tag_name(tag);
                        let close_tag = format!("</{}>", tag_name);
                        let content_start = tag_start + gt + 1;
                        if let Some(end) = Self::find_matching_close(
                            before,
                            content_start,
                            &format!("<{}", tag_name),
                            &close_tag,
                        ) {
                            if end >= pos {
                                depth += 1;
                            }
                            i = end + close_tag.len();
                            found = true;
                            break;
                        }
                    }
                }
            }
            if !found {
                break;
            }
        }
        depth.saturating_sub(1)
    }

    /// Extract the value from a quoted attribute string like ="value" or ='value'.
    fn extract_quoted_value(s: &str) -> Option<String> {
        let mut i = 0;
        while i < s.len() && s.as_bytes()[i].is_ascii_whitespace() {
            i += 1;
        }
        let quote = *s.as_bytes().get(i)? as char;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let val_start = i + 1;
        let val_end = s[val_start..].find(quote)? + val_start;
        Some(s[val_start..val_end].to_string())
    }

    /// Strip all HTML tags from a string, leaving only text.
    fn strip_tags(html: &str) -> String {
        let mut out = String::with_capacity(html.len());
        let mut in_tag = false;
        for c in html.chars() {
            if c == '<' {
                in_tag = true;
            } else if c == '>' {
                in_tag = false;
            } else if !in_tag {
                out.push(c);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_paragraph() {
        let html = "<p>Hello world</p>";
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("Hello world"));
    }

    #[test]
    fn heading_conversion() {
        let html = "<h1>Title</h1><h2>Subtitle</h2>";
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("Title"));
        assert!(md.contains("Subtitle"));
    }

    #[test]
    fn removes_scripts_and_styles() {
        let html = r#"
            <html>
            <head><style>body{color:red}</style></head>
            <body>
                <script>alert('x')</script>
                <p>Content</p>
            </body>
            </html>
        "#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("alert"));
        assert!(!md.contains("color:red"));
        assert!(md.contains("Content"));
    }

    #[test]
    fn strips_images_when_false() {
        let html = r#"<p>Text before</p><img src="a.png" alt="pic"><p>Text after</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("a.png"));
        assert!(!md.contains("pic"));
        assert!(md.contains("Text before"));
        assert!(md.contains("Text after"));
    }

    #[test]
    fn keeps_images_when_true() {
        let html = r#"<p>Text before</p><img src="a.png" alt="pic"><p>Text after</p>"#;
        let md = PageToMarkdown::convert(html, true, false, false, &[]).unwrap();
        assert!(md.contains("a.png"));
        assert!(md.contains("pic"));
        assert!(md.contains("Text before"));
        assert!(md.contains("Text after"));
    }

    #[test]
    fn strips_self_closing_images() {
        let html = r#"<p>Before</p><img src="b.png" alt="self"/><p>After</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("b.png"));
        assert!(!md.contains("self"));
        assert!(md.contains("Before"));
        assert!(md.contains("After"));
    }

    #[test]
    fn strips_iframe_tags() {
        let html = r#"
            <p>Before</p>
            <iframe src="https://video.ibm.com/embed/123" allowfullscreen></iframe>
            <p>After</p>
        "#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("iframe"));
        assert!(!md.contains("video.ibm.com"));
        assert!(md.contains("Before"));
        assert!(md.contains("After"));
    }

    #[test]
    fn strips_iframe_tags_self_closing() {
        let html = r#"<p>Before</p><iframe src="map.html"/><p>After</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("iframe"));
        assert!(!md.contains("map.html"));
        assert!(md.contains("Before"));
        assert!(md.contains("After"));
    }

    #[test]
    fn strips_nav_tags() {
        let html = r#"<nav><a href="/">Home</a><a href="/about">About</a></nav><p>Content</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("Home"));
        assert!(!md.contains("About"));
        assert!(md.contains("Content"));
    }

    #[test]
    fn strips_footer_tags() {
        let html = r#"<p>Article</p><footer>Copyright 2025</footer>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("Article"));
        assert!(!md.contains("Copyright"));
    }

    #[test]
    fn strips_aside_tags() {
        let html = r#"<p>Main text</p><aside>Sidebar content</aside>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("Main text"));
        assert!(!md.contains("Sidebar"));
    }

    #[test]
    fn strips_noscript_tags() {
        let html = r#"<noscript>Please enable JS</noscript><p>Visible</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("enable JS"));
        assert!(md.contains("Visible"));
    }

    #[test]
    fn strips_form_tags() {
        let html = r#"<form action="/submit"><input type="text"/><button>Go</button></form><p>Text</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("submit"));
        assert!(!md.contains("button"));
        assert!(md.contains("Text"));
    }

    #[test]
    fn strips_html_comments() {
        let html = r#"<p>Before</p><!-- this is a comment --><p>After</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("comment"));
        assert!(md.contains("Before"));
        assert!(md.contains("After"));
    }

    #[test]
    fn strips_noise_tags_case_insensitive() {
        let html = r#"<NAV>Menu</NAV><p>Body</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("Menu"));
        assert!(md.contains("Body"));
    }

    #[test]
    fn preserves_content_between_noise_tags() {
        let html = r#"<nav>Nav</nav><p>First</p><footer>Foot</footer><p>Second</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("Nav"));
        assert!(!md.contains("Foot"));
        assert!(md.contains("First"));
        assert!(md.contains("Second"));
    }

    #[test]
    fn code_block_language_preserved() {
        let html = r#"<pre><code class="language-rust">fn main() {}</code></pre>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("```rust"), "expected language annotation, got: {}", md);
        assert!(md.contains("fn main()"));
    }

    #[test]
    fn code_block_language_python() {
        let html = r#"<pre><code class="language-python">print("hello")</code></pre>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("```python"), "expected language annotation, got: {}", md);
        assert!(md.contains("print"));
    }

    #[test]
    fn code_block_no_language_stays_plain() {
        let html = r#"<pre><code>plain code</code></pre>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("```"));
        assert!(!md.contains("```rust"));
        assert!(!md.contains("```python"));
        assert!(md.contains("plain code"));
    }

    #[test]
    fn multiple_code_blocks_with_languages() {
        let html = r#"<pre><code class="language-rust">let x = 1;</code></pre><p>text</p><pre><code class="language-go">fmt.Println()</code></pre>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("```rust"));
        assert!(md.contains("```go"));
        assert!(md.contains("let x = 1;"));
        assert!(md.contains("fmt.Println()"));
    }

    #[test]
    fn strips_header_tags_by_default() {
        let html = r#"<header><h1>Site Title</h1><nav>Menu</nav></header><p>Article body</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("Site Title"));
        assert!(!md.contains("Menu"));
        assert!(md.contains("Article body"));
    }

    #[test]
    fn keeps_header_when_requested() {
        let html = r#"<header><h1>Article Title</h1></header><p>Article body</p>"#;
        let md = PageToMarkdown::convert(html, false, true, false, &[]).unwrap();
        assert!(md.contains("Article Title"));
        assert!(md.contains("Article body"));
    }

    #[test]
    fn dedup_removes_duplicate_paragraphs() {
        let html = r#"<p>This is a long paragraph that should be deduplicated when it appears twice.</p><p>This is a long paragraph that should be deduplicated when it appears twice.</p><p>Unique content here.</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        let count = md.matches("deduplicated").count();
        assert_eq!(count, 1, "duplicate paragraph should appear once, got: {}", md);
        assert!(md.contains("Unique content"));
    }

    #[test]
    fn dedup_keeps_short_blocks() {
        let html = r#"<h1>Title</h1><h1>Title</h1><p>Body text</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("Title"));
        assert!(md.contains("Body text"));
    }

    #[test]
    fn dedup_preserves_unique_blocks() {
        let html = r#"<p>First unique paragraph with enough text to exceed the threshold.</p><p>Second unique paragraph with different content entirely.</p><p>Third unique paragraph also different from the rest.</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("First unique"));
        assert!(md.contains("Second unique"));
        assert!(md.contains("Third unique"));
    }

    #[test]
    fn main_content_extracts_article_tag() {
        let html = r#"<nav>Home About</nav><article><h1>Article Title</h1><p>This is the main article content that should be extracted.</p></article><footer>Copyright 2025</footer>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        assert!(md.contains("Article Title"));
        assert!(md.contains("main article content"));
        assert!(!md.contains("Copyright"));
    }

    #[test]
    fn main_content_extracts_main_tag() {
        let html = r#"<div>Sidebar noise</div><main><p>Main content goes here with enough text to be meaningful.</p></main><aside>Related links</aside>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        assert!(md.contains("Main content"));
        assert!(!md.contains("Sidebar noise"));
    }

    #[test]
    fn main_content_extracts_role_main_div() {
        let html = r#"<div class="sidebar">Sidebar</div><div role="main"><p>This is the main content extracted via role attribute.</p></div>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        assert!(md.contains("role attribute"));
        assert!(!md.contains("Sidebar"));
    }

    #[test]
    fn main_content_falls_back_to_full_html() {
        let html = r#"<div><p>Just some content without semantic tags.</p></div>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        assert!(md.contains("Just some content"));
    }

    #[test]
    fn main_content_disabled_by_default() {
        let html = r#"<nav>Navigation</nav><article><p>Article content here.</p></article><footer>Footer</footer>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("Article content"));
    }

    #[test]
    fn fallback_chain_picks_highest_scoring_candidate() {
        let html = r#"<article><p>Short.</p></article><div><p>This div block has substantially more article text than the short article tag above, so the fallback chain should prefer it as the main content candidate when scoring all sources together.</p><p>Another paragraph adds even more text density to ensure this block wins over the semantic article wrapper.</p></div>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        assert!(md.contains("substantially more article text"));
        assert!(!md.contains("Short."));
    }

    #[test]
    fn boilerplate_strips_link_heavy_paragraphs() {
        let html = r#"<article><p><a href="/a">Link A</a> <a href="/b">Link B</a> <a href="/c">Link C</a></p><p>This is substantial article content with enough text to survive boilerplate stripping and represent the real main body of the page for extraction purposes.</p></article>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        assert!(md.contains("substantial article content"));
        assert!(!md.contains("Link A"));
        assert!(!md.contains("Link B"));
    }

    #[test]
    fn readability_extracts_content_div_from_layout() {
        let html = r#"<div><a href="/">Home</a><a href="/about">About</a><a href="/contact">Contact</a><a href="/blog">Blog</a><a href="/shop">Shop</a></div><div><h2>Article Title</h2><p>This is a substantial article body with enough text content to score well in the readability algorithm. It contains meaningful paragraphs of text that should be extracted as the main content of the page, far exceeding the navigation links above.</p><p>Another paragraph with additional content to further increase the text density score of this content block compared to the navigation block which consists mostly of short link texts.</p></div>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        assert!(md.contains("substantial article body"));
        assert!(!md.contains("Shop"));
    }

    #[test]
    fn readability_falls_back_when_no_semantic_tags() {
        let html = r#"<div><a href="/">Home</a><a href="/about">About</a></div><div><p>This is the main content paragraph with enough text to exceed the readability threshold for extraction. It has substantial body text that should be identified as the primary content block by the scoring algorithm.</p></div>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        assert!(md.contains("main content paragraph"));
        assert!(!md.contains("About"));
    }

    #[test]
    fn readability_returns_full_html_when_no_good_candidate() {
        let html = r#"<div><p>Short.</p></div><div><p>Brief.</p></div>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        assert!(md.contains("Short") || md.contains("Brief"));
    }

    #[test]
    fn readability_prefers_text_over_navigation() {
        let nav_html = r#"<div><a href="/a">Link A</a><a href="/b">Link B</a><a href="/c">Link C</a></div>"#;
        let content_html = r#"<div><p>This block has substantial text content that should score higher than the navigation block which only contains short link texts. The readability algorithm should correctly identify this as the main content area of the page.</p></div>"#;
        let html = format!("{}{}", nav_html, content_html);
        let md = PageToMarkdown::convert(&html, false, false, true, &[]).unwrap();
        assert!(md.contains("substantial text content"));
        assert!(!md.contains("Link A"));
        assert!(!md.contains("Link B"));
        assert!(!md.contains("Link C"));
    }

    #[test]
    fn readability_section_tag_supported() {
        let html = r#"<section><a href="/x">Short</a><a href="/y">Short2</a></section><section><p>This section contains the primary article content with enough text to be identified by the readability scoring algorithm as the main content block on the page, exceeding the navigation section in text density.</p></section>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        assert!(md.contains("primary article content"));
        assert!(!md.contains("Short2"));
    }

    #[test]
    fn paragraph_readability_extracts_dense_cluster() {
        let html = r#"<div><a href="/">Home</a><a href="/about">About</a></div><p>Short nav text.</p><p>This is the first paragraph of the main article content with substantial text that should be captured by the paragraph-level readability scoring algorithm as part of a dense cluster.</p><p>Here is the second paragraph continuing the article with more meaningful content that contributes to the overall text density of this cluster of paragraphs.</p><p>The third paragraph adds even more substantial text content to ensure the sliding window picks up this cluster as the highest scoring region of the page.</p><p>Finally the fourth paragraph rounds out the content cluster with additional text that should push the combined score well above the threshold for extraction.</p><div><a href="/privacy">Privacy</a><a href="/terms">Terms</a></div>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        assert!(md.contains("first paragraph"));
        assert!(md.contains("fourth paragraph"));
        assert!(!md.contains("Privacy"));
        assert!(!md.contains("Terms"));
    }

    #[test]
    fn paragraph_readability_falls_back_when_no_divs() {
        let html = r#"<p>Brief intro.</p><p>This is a substantial article paragraph with enough text content to be identified as the main content by the paragraph-level readability scoring algorithm when no div or section containers are present.</p><p>Another paragraph with additional content to build up the text density score of this cluster for extraction by the sliding window approach.</p><p>More content here to ensure the window score exceeds the threshold for meaningful extraction by the algorithm.</p>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        assert!(md.contains("substantial article paragraph"));
    }

    #[test]
    fn paragraph_readability_skips_short_paragraphs() {
        let html = r#"<p>OK.</p><p>Sure.</p><p>Yep.</p><p>No.</p><p>Fine.</p>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        // All paragraphs are too short — should return full HTML
        assert!(md.contains("OK") || md.contains("Sure") || md.contains("Yep"));
    }

    #[test]
    fn paragraph_readability_extracts_best_window() {
        let html = r#"<p>Nav link one.</p><p>Nav link two.</p><p>Nav link three.</p><p>This paragraph contains the real article content that should be extracted by the paragraph readability algorithm because it has substantially more text than the navigation paragraphs above and below it.</p><p>Continuing the real article content with another substantial paragraph that should be part of the extracted window along with the previous paragraph.</p><p>Footer text one.</p><p>Footer text two.</p>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        assert!(md.contains("real article content"));
    }

    #[test]
    fn comments_extracted_from_forum_page() {
        let html = r#"<html><head><title>Forum Thread</title></head><body>
            <h1>Discussion Topic</h1>
            <p>Original post content here.</p>
            <div class="comment" id="comment-1">
                <span class="author">Alice</span>
                <div class="comment-body"><p>First comment with some text.</p></div>
            </div>
            <div class="comment" id="comment-2">
                <span class="author">Bob</span>
                <div class="comment-body"><p>Second comment agreeing with Alice.</p></div>
            </div>
            <div class="comment" id="comment-3">
                <span class="author">Charlie</span>
                <div class="comment-body"><p>Third comment with a different perspective on the topic.</p></div>
            </div>
        </body></html>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("## Comments"));
        assert!(md.contains("Alice"));
        assert!(md.contains("Bob"));
        assert!(md.contains("Charlie"));
        assert!(md.contains("First comment"));
        assert!(md.contains("Second comment"));
        assert!(md.contains("Third comment"));
    }

    #[test]
    fn comments_not_extracted_from_non_forum_page() {
        let html = r#"<html><head><title>Article</title></head><body>
            <h1>Regular Article</h1>
            <p>This is a normal article with no comments section.</p>
            <p>It has multiple paragraphs of content.</p>
        </body></html>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("## Comments"));
    }

    #[test]
    fn comments_extracted_with_data_author() {
        let html = r#"<html><body>
            <h1>Thread</h1>
            <p>Post content.</p>
            <div class="comment" data-author="johndoe">
                <div class="comment-body"><p>Great post, thanks for sharing.</p></div>
            </div>
            <div class="comment" data-author="janedoe">
                <div class="comment-body"><p>I disagree with some points.</p></div>
            </div>
        </body></html>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("## Comments"));
        assert!(md.contains("johndoe"));
        assert!(md.contains("janedoe"));
        assert!(md.contains("Great post"));
    }

    #[test]
    fn comments_not_extracted_with_only_one_comment() {
        let html = r#"<html><body>
            <h1>Page</h1>
            <p>Content.</p>
            <div class="comment" id="comment-1">
                <span class="author">Solo</span>
                <div class="comment-body"><p>Only one comment here.</p></div>
            </div>
        </body></html>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(!md.contains("## Comments"));
    }

    #[test]
    fn comments_nested_with_indentation() {
        let html = r#"<html><body>
            <h1>Thread</h1>
            <p>Post.</p>
            <div class="comment" id="comment-1">
                <span class="author">Parent</span>
                <div class="comment-body"><p>Top level comment.</p></div>
                <div class="comment" id="comment-2">
                    <span class="author">Child</span>
                    <div class="comment-body"><p>Reply to parent.</p></div>
                </div>
            </div>
            <div class="comment" id="comment-3">
                <span class="author">Another</span>
                <div class="comment-body"><p>Another top level comment.</p></div>
            </div>
        </body></html>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("## Comments"));
        assert!(md.contains("Parent"));
        assert!(md.contains("Child"));
        assert!(md.contains("Top level comment"));
        assert!(md.contains("Reply to parent"));
    }

    #[test]
    fn comments_extracted_from_reddit_style() {
        let html = r#"<html><body>
            <h1>Reddit Post</h1>
            <p>Post content.</p>
            <div class="comment" data-testid="comment-1">
                <div data-author="redditor1">
                    <div class="md"><p>Reddit style comment.</p></div>
                </div>
            </div>
            <div class="comment" data-testid="comment-2">
                <div data-author="redditor2">
                    <div class="md"><p>Another Reddit comment.</p></div>
                </div>
            </div>
        </body></html>"#;
        let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("## Comments"));
        assert!(md.contains("redditor1"));
        assert!(md.contains("Reddit style comment"));
    }

    #[test]
    fn absolutize_links_converts_relative_to_absolute() {
        let md = "[About](/about) [Contact](/contact-us)";
        let result = PageToMarkdown::absolutize_links(md, "https://example.com/page");
        assert!(result.contains("https://example.com/about"));
        assert!(result.contains("https://example.com/contact-us"));
    }

    #[test]
    fn absolutize_links_handles_protocol_relative() {
        let md = "[Link](//cdn.example.com/file)";
        let result = PageToMarkdown::absolutize_links(md, "https://example.com/page");
        assert!(result.contains("https://cdn.example.com/file"));
    }

    #[test]
    fn absolutize_links_leaves_absolute_unchanged() {
        let md = "[Link](https://other.com/page)";
        let result = PageToMarkdown::absolutize_links(md, "https://example.com/page");
        assert!(result.contains("https://other.com/page"));
    }

    #[test]
    fn absolutize_links_leaves_anchor_links_unchanged() {
        let md = "[Section](#section)";
        let result = PageToMarkdown::absolutize_links(md, "https://example.com/page");
        assert!(result.contains("#section"));
    }

    #[test]
    fn absolutize_links_resolves_relative_paths() {
        let md = "[Prev](../parent) [Next](./child)";
        let result = PageToMarkdown::absolutize_links(md, "https://example.com/blog/post");
        // ../parent from /blog/post → /parent (go up from post to blog, then up from blog to root)
        assert!(result.contains("https://example.com/parent"), "got: {}", result);
        // ./child from /blog/post → /blog/child (same directory as post)
        assert!(result.contains("https://example.com/blog/child"), "got: {}", result);
    }

    #[test]
    fn absolutize_links_handles_image_links() {
        let md = "![Photo](/images/photo.jpg)";
        let result = PageToMarkdown::absolutize_links(md, "https://example.com/page");
        assert!(result.contains("https://example.com/images/photo.jpg"));
    }

    #[test]
    fn absolutize_links_preserves_surrounding_text() {
        let md = "Hello [world](/world) and [universe](/universe) end";
        let result = PageToMarkdown::absolutize_links(md, "https://example.com");
        assert!(result.starts_with("Hello "));
        assert!(result.contains("https://example.com/world"));
        assert!(result.contains("https://example.com/universe"));
        assert!(result.ends_with("end"));
    }

    #[test]
    fn absolutize_links_invalid_base_returns_original() {
        let md = "[Link](/path)";
        let result = PageToMarkdown::absolutize_links(md, "not-a-url");
        assert_eq!(result, md);
    }

    #[test]
    fn absolutize_links_preserves_unicode_text() {
        let md = "[リンク](/page) 日本語テキスト";
        let result = PageToMarkdown::absolutize_links(md, "https://example.com");
        assert!(result.contains("https://example.com/page"));
        assert!(result.contains("日本語テキスト"));
    }

    #[test]
    fn exclude_selector_class_removes_element() {
        let html = r#"<p>Keep this</p><div class="ad">Advertisement</div><p>Also keep</p>"#;
        let selectors = vec![".ad".to_string()];
        let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
        assert!(md.contains("Keep this"));
        assert!(md.contains("Also keep"));
        assert!(!md.contains("Advertisement"));
    }

    #[test]
    fn exclude_selector_id_removes_element() {
        let html = r#"<p>Keep this</p><div id="sidebar">Sidebar content</div><p>Also keep</p>"#;
        let selectors = vec!["#sidebar".to_string()];
        let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
        assert!(md.contains("Keep this"));
        assert!(md.contains("Also keep"));
        assert!(!md.contains("Sidebar"));
    }

    #[test]
    fn exclude_selector_multiple_selectors() {
        let html = r#"<div class="ad">Ad</div><p>Keep</p><div id="promo">Promo</div><p>Also keep</p>"#;
        let selectors = vec![".ad".to_string(), "#promo".to_string()];
        let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
        assert!(md.contains("Keep"));
        assert!(md.contains("Also keep"));
        assert!(!md.contains("Ad"));
        assert!(!md.contains("Promo"));
    }

    #[test]
    fn exclude_selector_class_word_boundary() {
        let html = r#"<div class="ad-banner">Banner</div><p>Keep</p><div class="notad">Should keep</div>"#;
        let selectors = vec![".ad".to_string()];
        let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
        assert!(md.contains("Keep"));
        assert!(md.contains("Banner"));
        assert!(md.contains("Should keep"));
    }

    #[test]
    fn exclude_selector_class_multi_class_match() {
        let html = r#"<div class="box highlight">Highlighted</div><p>Keep</p>"#;
        let selectors = vec![".highlight".to_string()];
        let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
        assert!(md.contains("Keep"));
        assert!(!md.contains("Highlighted"));
    }

    #[test]
    fn exclude_selector_empty_does_nothing() {
        let html = r#"<p>Keep this</p><div class="ad">Ad</div>"#;
        let selectors: Vec<String> = vec![];
        let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
        assert!(md.contains("Keep this"));
        assert!(md.contains("Ad"));
    }

    #[test]
    fn exclude_selector_nested_elements_removed() {
        let html = r#"<div class="ad"><h2>Ad Title</h2><p>Ad content</p></div><p>Keep this</p>"#;
        let selectors = vec![".ad".to_string()];
        let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
        assert!(md.contains("Keep this"));
        assert!(!md.contains("Ad Title"));
        assert!(!md.contains("Ad content"));
    }

    #[test]
    fn to_plain_text_strips_markdown_syntax() {
        let md = "# Title\n\nHello **world** with [a link](https://example.com).";
        let text = PageToMarkdown::to_plain_text(md);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello world"));
        assert!(text.contains("a link"));
        assert!(!text.contains("**"));
        assert!(!text.contains("](https://"));
    }

    #[test]
    fn to_plain_text_preserves_code_content() {
        let md = "Use `println!` in Rust.";
        let text = PageToMarkdown::to_plain_text(md);
        assert!(text.contains("println!"));
        assert!(!text.contains('`'));
    }

    #[test]
    fn to_plain_text_strips_list_markers() {
        let md = "* one\n* two\n\nParagraph.";
        let text = PageToMarkdown::to_plain_text(md);
        assert!(text.contains("one"));
        assert!(text.contains("two"));
        assert!(text.contains("Paragraph"));
        assert!(!text.contains("* one"));
    }
}
