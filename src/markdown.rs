use anyhow::Result;

/// Case-insensitive byte search. Returns byte position of `needle` in `haystack`.
pub(crate) fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let needle_lower: Vec<u8> = needle.bytes().map(|b| b.to_ascii_lowercase()).collect();
    let h = haystack.as_bytes();
    if needle_lower.len() > h.len() {
        return None;
    }
    'outer: for i in 0..=h.len() - needle_lower.len() {
        for (j, &n) in needle_lower.iter().enumerate() {
            if h[i + j].to_ascii_lowercase() != n {
                continue 'outer;
            }
        }
        return Some(i);
    }
    None
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
        if let Some(lang) = cls.strip_prefix("language-") {
            if !lang.is_empty() {
                return Some(lang.to_string());
            }
        }
    }
    None
}

/// Convert raw HTML to clean Markdown.
/// Strips scripts, styles, and non-essential markup to minimize token output.
pub struct PageToMarkdown;

impl PageToMarkdown {
    /// Convert HTML string to Markdown.
    /// When `include_images` is false, strips `<img>` tags to reduce token output.
    /// When `main_content` is true, extracts only the content of `<article>`, `<main>`, or `[role="main"]` elements.
    pub fn convert(html: &str, include_images: bool, keep_header: bool, main_content: bool) -> Result<String> {
        let html = if main_content {
            Self::extract_main_content(html)
        } else {
            html.to_string()
        };
        let html = Self::strip_scripts_and_styles(&html);
        let html = Self::strip_iframe_tags(&html);
        let html = Self::strip_noise_tags(&html, keep_header);
        let html = Self::strip_html_comments(&html);
        let languages = Self::extract_code_languages(&html);
        let html = if include_images { html } else { Self::strip_img_tags(&html) };
        let md = html2md::parse_html(&html);
        let md = Self::inject_code_languages(&md, &languages);
        let md = Self::deduplicate_blocks(&md);
        Ok(Self::clean(&md))
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
    /// Looks for `<article>`, `<main>`, or `<div role="main">` tags (in priority order).
    /// Returns the inner HTML of the first match. If none found, returns the full HTML.
    fn extract_main_content(html: &str) -> String {
        for (tag, close_tag) in [("article", "</article>"), ("main", "</main>")] {
            if let Some(start) = find_ci(html, &format!("<{}", tag)) {
                if let Some(gt) = html[start..].find('>') {
                    let content_start = start + gt + 1;
                    if let Some(end) = find_ci(&html[content_start..], close_tag) {
                        return html[content_start..content_start + end].to_string();
                    }
                }
            }
        }
        // Look for <div role="main">
        let mut i = 0;
        while i < html.len() {
            if let Some(pos) = find_ci(&html[i..], "<div") {
                let pos = i + pos;
                if let Some(gt) = html[pos..].find('>') {
                    let tag = &html[pos..=pos + gt];
                    if find_ci(tag, "role=\"main\"").is_some() || find_ci(tag, "role='main'").is_some() {
                        let content_start = pos + gt + 1;
                        if let Some(end) = find_ci(&html[content_start..], "</div>") {
                            return html[content_start..content_start + end].to_string();
                        }
                    }
                    i = pos + gt + 1;
                    continue;
                }
            }
            break;
        }
        // Readability fallback: score top-level <div> elements by text density
        Self::readability_extract(html)
    }

    /// Readability-based content extraction.
    /// Scores top-level `<div>` and `<section>` blocks by text density and link density.
    /// Returns the inner HTML of the highest-scoring block, or the full HTML if no suitable block is found.
    fn readability_extract(html: &str) -> String {
        let candidates = Self::find_top_level_blocks(html);
        if candidates.is_empty() {
            return html.to_string();
        }

        let mut best_score = 0i64;
        let mut best_html = None;

        for (content, _tag) in &candidates {
            let text_len = Self::count_text_chars(content) as i64;
            let link_text_len = Self::count_link_text_chars(content) as i64;
            // Score: text content minus link text (navigation penalized)
            let score = text_len - link_text_len;
            if score > best_score {
                best_score = score;
                best_html = Some(content.clone());
            }
        }

        // Only use readability result if the best block has meaningful content (>100 chars of non-link text)
        if best_score > 100 {
            best_html.unwrap_or_else(|| html.to_string())
        } else {
            html.to_string()
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_paragraph() {
        let html = "<p>Hello world</p>";
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(md.contains("Hello world"));
    }

    #[test]
    fn heading_conversion() {
        let html = "<h1>Title</h1><h2>Subtitle</h2>";
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
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
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(!md.contains("alert"));
        assert!(!md.contains("color:red"));
        assert!(md.contains("Content"));
    }

    #[test]
    fn strips_images_when_false() {
        let html = r#"<p>Text before</p><img src="a.png" alt="pic"><p>Text after</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(!md.contains("a.png"));
        assert!(!md.contains("pic"));
        assert!(md.contains("Text before"));
        assert!(md.contains("Text after"));
    }

    #[test]
    fn keeps_images_when_true() {
        let html = r#"<p>Text before</p><img src="a.png" alt="pic"><p>Text after</p>"#;
        let md = PageToMarkdown::convert(html, true, false, false).unwrap();
        assert!(md.contains("a.png"));
        assert!(md.contains("pic"));
        assert!(md.contains("Text before"));
        assert!(md.contains("Text after"));
    }

    #[test]
    fn strips_self_closing_images() {
        let html = r#"<p>Before</p><img src="b.png" alt="self"/><p>After</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
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
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(!md.contains("iframe"));
        assert!(!md.contains("video.ibm.com"));
        assert!(md.contains("Before"));
        assert!(md.contains("After"));
    }

    #[test]
    fn strips_iframe_tags_self_closing() {
        let html = r#"<p>Before</p><iframe src="map.html"/><p>After</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(!md.contains("iframe"));
        assert!(!md.contains("map.html"));
        assert!(md.contains("Before"));
        assert!(md.contains("After"));
    }

    #[test]
    fn strips_nav_tags() {
        let html = r#"<nav><a href="/">Home</a><a href="/about">About</a></nav><p>Content</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(!md.contains("Home"));
        assert!(!md.contains("About"));
        assert!(md.contains("Content"));
    }

    #[test]
    fn strips_footer_tags() {
        let html = r#"<p>Article</p><footer>Copyright 2025</footer>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(md.contains("Article"));
        assert!(!md.contains("Copyright"));
    }

    #[test]
    fn strips_aside_tags() {
        let html = r#"<p>Main text</p><aside>Sidebar content</aside>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(md.contains("Main text"));
        assert!(!md.contains("Sidebar"));
    }

    #[test]
    fn strips_noscript_tags() {
        let html = r#"<noscript>Please enable JS</noscript><p>Visible</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(!md.contains("enable JS"));
        assert!(md.contains("Visible"));
    }

    #[test]
    fn strips_form_tags() {
        let html = r#"<form action="/submit"><input type="text"/><button>Go</button></form><p>Text</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(!md.contains("submit"));
        assert!(!md.contains("button"));
        assert!(md.contains("Text"));
    }

    #[test]
    fn strips_html_comments() {
        let html = r#"<p>Before</p><!-- this is a comment --><p>After</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(!md.contains("comment"));
        assert!(md.contains("Before"));
        assert!(md.contains("After"));
    }

    #[test]
    fn strips_noise_tags_case_insensitive() {
        let html = r#"<NAV>Menu</NAV><p>Body</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(!md.contains("Menu"));
        assert!(md.contains("Body"));
    }

    #[test]
    fn preserves_content_between_noise_tags() {
        let html = r#"<nav>Nav</nav><p>First</p><footer>Foot</footer><p>Second</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(!md.contains("Nav"));
        assert!(!md.contains("Foot"));
        assert!(md.contains("First"));
        assert!(md.contains("Second"));
    }

    #[test]
    fn code_block_language_preserved() {
        let html = r#"<pre><code class="language-rust">fn main() {}</code></pre>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(md.contains("```rust"), "expected language annotation, got: {}", md);
        assert!(md.contains("fn main()"));
    }

    #[test]
    fn code_block_language_python() {
        let html = r#"<pre><code class="language-python">print("hello")</code></pre>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(md.contains("```python"), "expected language annotation, got: {}", md);
        assert!(md.contains("print"));
    }

    #[test]
    fn code_block_no_language_stays_plain() {
        let html = r#"<pre><code>plain code</code></pre>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(md.contains("```"));
        assert!(!md.contains("```rust"));
        assert!(!md.contains("```python"));
        assert!(md.contains("plain code"));
    }

    #[test]
    fn multiple_code_blocks_with_languages() {
        let html = r#"<pre><code class="language-rust">let x = 1;</code></pre><p>text</p><pre><code class="language-go">fmt.Println()</code></pre>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(md.contains("```rust"));
        assert!(md.contains("```go"));
        assert!(md.contains("let x = 1;"));
        assert!(md.contains("fmt.Println()"));
    }

    #[test]
    fn strips_header_tags_by_default() {
        let html = r#"<header><h1>Site Title</h1><nav>Menu</nav></header><p>Article body</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(!md.contains("Site Title"));
        assert!(!md.contains("Menu"));
        assert!(md.contains("Article body"));
    }

    #[test]
    fn keeps_header_when_requested() {
        let html = r#"<header><h1>Article Title</h1></header><p>Article body</p>"#;
        let md = PageToMarkdown::convert(html, false, true, false).unwrap();
        assert!(md.contains("Article Title"));
        assert!(md.contains("Article body"));
    }

    #[test]
    fn dedup_removes_duplicate_paragraphs() {
        let html = r#"<p>This is a long paragraph that should be deduplicated when it appears twice.</p><p>This is a long paragraph that should be deduplicated when it appears twice.</p><p>Unique content here.</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        let count = md.matches("deduplicated").count();
        assert_eq!(count, 1, "duplicate paragraph should appear once, got: {}", md);
        assert!(md.contains("Unique content"));
    }

    #[test]
    fn dedup_keeps_short_blocks() {
        let html = r#"<h1>Title</h1><h1>Title</h1><p>Body text</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(md.contains("Title"));
        assert!(md.contains("Body text"));
    }

    #[test]
    fn dedup_preserves_unique_blocks() {
        let html = r#"<p>First unique paragraph with enough text to exceed the threshold.</p><p>Second unique paragraph with different content entirely.</p><p>Third unique paragraph also different from the rest.</p>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(md.contains("First unique"));
        assert!(md.contains("Second unique"));
        assert!(md.contains("Third unique"));
    }

    #[test]
    fn main_content_extracts_article_tag() {
        let html = r#"<nav>Home About</nav><article><h1>Article Title</h1><p>This is the main article content that should be extracted.</p></article><footer>Copyright 2025</footer>"#;
        let md = PageToMarkdown::convert(html, false, false, true).unwrap();
        assert!(md.contains("Article Title"));
        assert!(md.contains("main article content"));
        assert!(!md.contains("Copyright"));
    }

    #[test]
    fn main_content_extracts_main_tag() {
        let html = r#"<div>Sidebar noise</div><main><p>Main content goes here with enough text to be meaningful.</p></main><aside>Related links</aside>"#;
        let md = PageToMarkdown::convert(html, false, false, true).unwrap();
        assert!(md.contains("Main content"));
        assert!(!md.contains("Sidebar noise"));
    }

    #[test]
    fn main_content_extracts_role_main_div() {
        let html = r#"<div class="sidebar">Sidebar</div><div role="main"><p>This is the main content extracted via role attribute.</p></div>"#;
        let md = PageToMarkdown::convert(html, false, false, true).unwrap();
        assert!(md.contains("role attribute"));
        assert!(!md.contains("Sidebar"));
    }

    #[test]
    fn main_content_falls_back_to_full_html() {
        let html = r#"<div><p>Just some content without semantic tags.</p></div>"#;
        let md = PageToMarkdown::convert(html, false, false, true).unwrap();
        assert!(md.contains("Just some content"));
    }

    #[test]
    fn main_content_disabled_by_default() {
        let html = r#"<nav>Navigation</nav><article><p>Article content here.</p></article><footer>Footer</footer>"#;
        let md = PageToMarkdown::convert(html, false, false, false).unwrap();
        assert!(md.contains("Article content"));
    }

    #[test]
    fn readability_extracts_content_div_from_layout() {
        let html = r#"<div><a href="/">Home</a><a href="/about">About</a><a href="/contact">Contact</a><a href="/blog">Blog</a><a href="/shop">Shop</a></div><div><h2>Article Title</h2><p>This is a substantial article body with enough text content to score well in the readability algorithm. It contains meaningful paragraphs of text that should be extracted as the main content of the page, far exceeding the navigation links above.</p><p>Another paragraph with additional content to further increase the text density score of this content block compared to the navigation block which consists mostly of short link texts.</p></div>"#;
        let md = PageToMarkdown::convert(html, false, false, true).unwrap();
        assert!(md.contains("substantial article body"));
        assert!(!md.contains("Shop"));
    }

    #[test]
    fn readability_falls_back_when_no_semantic_tags() {
        let html = r#"<div><a href="/">Home</a><a href="/about">About</a></div><div><p>This is the main content paragraph with enough text to exceed the readability threshold for extraction. It has substantial body text that should be identified as the primary content block by the scoring algorithm.</p></div>"#;
        let md = PageToMarkdown::convert(html, false, false, true).unwrap();
        assert!(md.contains("main content paragraph"));
        assert!(!md.contains("About"));
    }

    #[test]
    fn readability_returns_full_html_when_no_good_candidate() {
        let html = r#"<div><p>Short.</p></div><div><p>Brief.</p></div>"#;
        let md = PageToMarkdown::convert(html, false, false, true).unwrap();
        assert!(md.contains("Short") || md.contains("Brief"));
    }

    #[test]
    fn readability_prefers_text_over_navigation() {
        let nav_html = r#"<div><a href="/a">Link A</a><a href="/b">Link B</a><a href="/c">Link C</a></div>"#;
        let content_html = r#"<div><p>This block has substantial text content that should score higher than the navigation block which only contains short link texts. The readability algorithm should correctly identify this as the main content area of the page.</p></div>"#;
        let html = format!("{}{}", nav_html, content_html);
        let md = PageToMarkdown::convert(&html, false, false, true).unwrap();
        assert!(md.contains("substantial text content"));
        assert!(!md.contains("Link A"));
        assert!(!md.contains("Link B"));
        assert!(!md.contains("Link C"));
    }

    #[test]
    fn readability_section_tag_supported() {
        let html = r#"<section><a href="/x">Short</a><a href="/y">Short2</a></section><section><p>This section contains the primary article content with enough text to be identified by the readability scoring algorithm as the main content block on the page, exceeding the navigation section in text density.</p></section>"#;
        let md = PageToMarkdown::convert(html, false, false, true).unwrap();
        assert!(md.contains("primary article content"));
        assert!(!md.contains("Short2"));
    }
}
