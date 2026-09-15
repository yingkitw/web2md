//! Boilerplate removal: stripping scripts, styles, nav/footer/aside noise, images,
//! tables, iframes, comments, selector-targeted elements, and Markdown noise cleanup.
use super::*;

impl PageToMarkdown {
    /// Remove `<img>` tags (case-insensitive). Self-closing (`/>`) or plain (`>`) both handled.
    pub(super) fn strip_img_tags(html: &str) -> String {
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

    /// Remove `<table>...</table>` blocks (case-insensitive).
    pub(super) fn strip_table_tags(html: &str) -> String {
        Self::strip_tag_pair(html, "table", "</table>")
    }

    /// Replace `[text](url)` and `![alt](url)` with visible text/alt only.
    pub(super) fn strip_markdown_links(md: &str) -> String {
        let mut result = String::with_capacity(md.len());
        let mut i = 0;
        while i < md.len() {
            let rest = &md[i..];
            if rest.starts_with("![")
                && let Some((alt, end)) = parse_md_link_at(md, i + 1) {
                    result.push_str(&alt);
                    i = end;
                    continue;
                }
            if rest.starts_with('[')
                && let Some((text, end)) = parse_md_link_at(md, i) {
                    result.push_str(&text);
                    i = end;
                    continue;
                }
            let ch = rest.chars().next().unwrap();
            result.push(ch);
            i += ch.len_utf8();
        }
        result
    }

    /// Remove `<script>` and `<style>` blocks (case-insensitive, non-greedy)
    pub(super) fn strip_scripts_and_styles(html: &str) -> String {
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
    pub(super) fn strip_noise_tags(html: &str, keep_header: bool) -> String {
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
    pub(super) fn strip_tag_pair(html: &str, tag: &str, close_tag: &str) -> String {
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

    /// Remove HTML elements whose `class` or `id` attribute matches common
    /// boilerplate patterns (cookie banners, social share, breadcrumbs,
    /// newsletter signups, popups, toolbars, ads). Deterministic, no LLM.
    pub(super) fn strip_boilerplate_elements(html: &str) -> String {
        let boilerplate_keywords = [
            "cookie",
            "consent",
            "gdpr",
            "social",
            "share",
            "sharing",
            "breadcrumb",
            "newsletter",
            "subscribe",
            "popup",
            "modal",
            "overlay",
            "toolbar",
            "scrollbar",
            "advert",
            "ad-block",
            "adsense",
            "related-posts",
            "related-articles",
            "cookie-notice",
            "cookie-banner",
        ];

        // Find opening tags with class/id matching boilerplate keywords.
        // We scan for `<div`, `<section`, `<aside`, `<span` with matching attrs,
        // then remove the element and its matching closing tag (nesting-aware).
        let tags_to_check = ["div", "section", "aside", "span", "ul", "li", "nav"];
        let mut result = html.to_string();

        for tag in &tags_to_check {
            let open = format!("<{}", tag);
            loop {
                let lower = result.to_ascii_lowercase();
                let mut found_pos = None;
                let mut search_from = 0;
                while let Some(start) = lower[search_from..].find(&open) {
                    let start = search_from + start;
                    // Find the end of the opening tag
                    if let Some(gt) = result[start..].find('>') {
                        let tag_content = &lower[start..=start + gt];
                        let is_boilerplate = boilerplate_keywords
                            .iter()
                            .any(|kw| tag_content.contains(&format!("class=\"{kw}"))
                                || tag_content.contains(&format!("class='{kw}"))
                                || tag_content.contains(&format!("id=\"{kw}"))
                                || tag_content.contains(&format!("id='{kw}"))
                                || tag_content.contains(&format!(" class=\"{kw} "))
                                || tag_content.contains(&format!(" class='{kw} "))
                                || tag_content.contains(&format!("class=\"{kw}-"))
                                || tag_content.contains(&format!("class='{kw}-"))
                                || tag_content.contains(&format!("id=\"{kw}-"))
                                || tag_content.contains(&format!("id='{kw}-")));
                        if is_boilerplate {
                            found_pos = Some(start);
                            break;
                        }
                    }
                    search_from = start + 1;
                }

                if let Some(start) = found_pos {
                    // Find matching closing tag (nesting-aware)
                    let open_str = format!("<{}", tag);
                    let close_str = format!("</{}>", tag);
                    if let Some(gt) = result[start..].find('>') {
                        let content_start = start + gt + 1;
                        if let Some(end_pos) = Self::find_matching_close(&result, content_start, &open_str, &close_str) {
                            result = format!("{}{}", &result[..start], &result[end_pos + close_str.len()..]);
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                } else {
                    break; // No more boilerplate elements for this tag
                }
            }
        }
        result
    }

    /// Post-process Markdown to remove common noise patterns:
    /// - Wikipedia-style citation markers: `^([[ N ]](#cite_note-...))`
    /// - `[edit]` links in section headings
    /// - Heading self-anchor links: `[Heading](#anchor)` → `Heading`
    /// - `[File:...]` links (Wikipedia file description pages)
    pub(super) fn clean_markdown_noise(md: &str) -> String {
        let mut result = md.to_string();

        // Remove Wikipedia-style citation markers: ^([[ N ]](#cite_note-...))
        // The pattern is: ^ ( [[ N ]] (#cite_note-...) )
        let citation_re = regex::Regex::new(
            r"\^\(\[\[\s*\d+\s*\]\]\(#[^)]*\)\)"
        ).unwrap();
        // Remove stacked citations (multiple in a row)
        loop {
            let before = result.clone();
            result = citation_re.replace_all(&result, "").to_string();
            if result == before {
                break;
            }
        }
        // Also remove bare citation links without the ^() wrapper: [[ N ]](#cite_note-...)
        let bare_citation_re = regex::Regex::new(
            r"\[\[\s*\d+\s*\]\]\(#[^)]*\)"
        ).unwrap();
        loop {
            let before = result.clone();
            result = bare_citation_re.replace_all(&result, "").to_string();
            if result == before {
                break;
            }
        }
        // Remove ^([[ note N ]](#cite_note-...)) patterns
        let note_re = regex::Regex::new(
            r"\^\(\[\[\s*note\s*\d+\s*\]\]\(#[^)]*\)\)"
        ).unwrap();
        result = note_re.replace_all(&result, "").to_string();

        // Remove [edit] links: [ [edit](url) ] or [edit](url)
        // URLs may contain parens (e.g. Wikipedia article URLs), so match
        // balanced parens by matching until the closing `)` followed by `]` or end.
        let edit_re = regex::Regex::new(
            r"\[?\[edit\]\([^)]*(?:\([^)]*\)[^)]*)*\)\]?"
        ).unwrap();
        result = edit_re.replace_all(&result, "").to_string();

        // Clean up leftover bracket fragments from partial matches:
        // `[ <url-suffix>) ]` patterns left after the edit link text was stripped
        // but the URL contained parens that broke the match.
        let leftover_re = regex::Regex::new(
            r"\[\s*&action=edit[^]]*\]"
        ).unwrap();
        result = leftover_re.replace_all(&result, "").to_string();

        // Remove [File:...] links (Wikipedia file description pages)
        let file_re = regex::Regex::new(r"\[File:[^\]]*\]\([^)]*\)").unwrap();
        result = file_re.replace_all(&result, "").to_string();

        // Remove [Image:...] links similarly
        let image_re = regex::Regex::new(r"\[Image:[^\]]*\]\([^)]*\)").unwrap();
        result = image_re.replace_all(&result, "").to_string();

        // Clean heading self-anchor links: ### [Heading](#anchor) ### → ### Heading ###
        // Only applies to heading lines (starting with # or containing ===/---)
        let heading_anchor_re = regex::Regex::new(
            r"^(#{1,6}\s+)\[([^\]]+)\]\(#[^)]+\)(\s+#{0,6})$"
        ).unwrap();
        result = heading_anchor_re.replace_all(&result, "$1$2$3").to_string();

        // Also handle setext-style headings with anchor links:
        // [Heading](#anchor)\n=== → Heading\n===
        let setext_re = regex::Regex::new(
            r"^\[([^\]]+)\]\(#[^)]+\)$"
        ).unwrap();
        let lines: Vec<&str> = result.lines().collect();
        let mut cleaned_lines = Vec::with_capacity(lines.len());
        for (i, line) in lines.iter().enumerate() {
            let next_is_setext = i + 1 < lines.len()
                && (lines[i + 1].trim().chars().all(|c| c == '=') && lines[i + 1].trim().len() >= 3
                    || lines[i + 1].trim().chars().all(|c| c == '-') && lines[i + 1].trim().len() >= 3);
            if next_is_setext {
                if let Some(caps) = setext_re.captures(line) {
                    cleaned_lines.push(caps.get(1).unwrap().as_str().to_string());
                    continue;
                }
            }
            cleaned_lines.push(line.to_string());
        }
        result = cleaned_lines.join("\n");

        // Clean up extra spaces left by removed elements
        let multi_space = regex::Regex::new(r"  +").unwrap();
        result = multi_space.replace_all(&result, " ").to_string();

        result
    }

    /// Remove HTML comments `<!-- ... -->` (case-insensitive on delimiters).
    pub(super) fn strip_html_comments(html: &str) -> String {
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
    pub(super) fn strip_by_selectors(html: &str, selectors: &[String]) -> String {
        let mut result = html.to_string();
        for selector in selectors {
            let trimmed = selector.trim();
            if let Some(class_name) = trimmed.strip_prefix('.') {
                if !class_name.is_empty() {
                    result = Self::strip_elements_by_attr(&result, "class", class_name);
                }
            } else if let Some(id_val) = trimmed.strip_prefix('#')
                && !id_val.is_empty() {
                    result = Self::strip_elements_by_attr(&result, "id", id_val);
                }
        }
        result
    }

    /// Remove HTML elements where an attribute contains the given value.
    /// Matches `class="foo bar"` for value "foo" (word-boundary match),
    /// and `id="exact"` for value "exact" (exact match).
    pub(super) fn strip_elements_by_attr(html: &str, attr: &str, value: &str) -> String {
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
    pub(super) fn extract_code_languages(html: &str) -> Vec<String> {
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
    pub(super) fn inject_code_languages(md: &str, languages: &[String]) -> String {
        let mut lang_idx = 0;
        Self::inject_code_languages_from(md, languages, &mut lang_idx)
    }

    pub(super) fn inject_code_languages_from(md: &str, languages: &[String], lang_idx: &mut usize) -> String {
        if languages.is_empty() {
            return md.to_string();
        }
        let mut result = String::with_capacity(md.len());
        let lines: Vec<&str> = md.lines().collect();
        let mut in_code_block = false;
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                if !in_code_block {
                    // Opening fence — inject language if available
                    in_code_block = true;
                    if *lang_idx < languages.len() {
                        result.push_str(&format!("```{}", languages[*lang_idx]));
                        *lang_idx += 1;
                    } else {
                        result.push_str(line);
                    }
                } else {
                    // Closing fence — do NOT inject language
                    in_code_block = false;
                    result.push_str(line);
                }
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
    pub(super) fn strip_iframe_tags(html: &str) -> String {
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
}
