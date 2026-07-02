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
    pub fn convert(html: &str, include_images: bool, keep_header: bool) -> Result<String> {
        let html = Self::strip_scripts_and_styles(html);
        let html = Self::strip_iframe_tags(&html);
        let html = Self::strip_noise_tags(&html, keep_header);
        let html = Self::strip_html_comments(&html);
        let languages = Self::extract_code_languages(&html);
        let html = if include_images { html } else { Self::strip_img_tags(&html) };
        let md = html2md::parse_html(&html);
        let md = Self::inject_code_languages(&md, &languages);
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
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(md.contains("Hello world"));
    }

    #[test]
    fn heading_conversion() {
        let html = "<h1>Title</h1><h2>Subtitle</h2>";
        let md = PageToMarkdown::convert(html, false, false).unwrap();
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
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(!md.contains("alert"));
        assert!(!md.contains("color:red"));
        assert!(md.contains("Content"));
    }

    #[test]
    fn strips_images_when_false() {
        let html = r#"<p>Text before</p><img src="a.png" alt="pic"><p>Text after</p>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(!md.contains("a.png"));
        assert!(!md.contains("pic"));
        assert!(md.contains("Text before"));
        assert!(md.contains("Text after"));
    }

    #[test]
    fn keeps_images_when_true() {
        let html = r#"<p>Text before</p><img src="a.png" alt="pic"><p>Text after</p>"#;
        let md = PageToMarkdown::convert(html, true, false).unwrap();
        assert!(md.contains("a.png"));
        assert!(md.contains("pic"));
        assert!(md.contains("Text before"));
        assert!(md.contains("Text after"));
    }

    #[test]
    fn strips_self_closing_images() {
        let html = r#"<p>Before</p><img src="b.png" alt="self"/><p>After</p>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
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
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(!md.contains("iframe"));
        assert!(!md.contains("video.ibm.com"));
        assert!(md.contains("Before"));
        assert!(md.contains("After"));
    }

    #[test]
    fn strips_iframe_tags_self_closing() {
        let html = r#"<p>Before</p><iframe src="map.html"/><p>After</p>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(!md.contains("iframe"));
        assert!(!md.contains("map.html"));
        assert!(md.contains("Before"));
        assert!(md.contains("After"));
    }

    #[test]
    fn strips_nav_tags() {
        let html = r#"<nav><a href="/">Home</a><a href="/about">About</a></nav><p>Content</p>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(!md.contains("Home"));
        assert!(!md.contains("About"));
        assert!(md.contains("Content"));
    }

    #[test]
    fn strips_footer_tags() {
        let html = r#"<p>Article</p><footer>Copyright 2025</footer>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(md.contains("Article"));
        assert!(!md.contains("Copyright"));
    }

    #[test]
    fn strips_aside_tags() {
        let html = r#"<p>Main text</p><aside>Sidebar content</aside>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(md.contains("Main text"));
        assert!(!md.contains("Sidebar"));
    }

    #[test]
    fn strips_noscript_tags() {
        let html = r#"<noscript>Please enable JS</noscript><p>Visible</p>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(!md.contains("enable JS"));
        assert!(md.contains("Visible"));
    }

    #[test]
    fn strips_form_tags() {
        let html = r#"<form action="/submit"><input type="text"/><button>Go</button></form><p>Text</p>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(!md.contains("submit"));
        assert!(!md.contains("button"));
        assert!(md.contains("Text"));
    }

    #[test]
    fn strips_html_comments() {
        let html = r#"<p>Before</p><!-- this is a comment --><p>After</p>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(!md.contains("comment"));
        assert!(md.contains("Before"));
        assert!(md.contains("After"));
    }

    #[test]
    fn strips_noise_tags_case_insensitive() {
        let html = r#"<NAV>Menu</NAV><p>Body</p>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(!md.contains("Menu"));
        assert!(md.contains("Body"));
    }

    #[test]
    fn preserves_content_between_noise_tags() {
        let html = r#"<nav>Nav</nav><p>First</p><footer>Foot</footer><p>Second</p>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(!md.contains("Nav"));
        assert!(!md.contains("Foot"));
        assert!(md.contains("First"));
        assert!(md.contains("Second"));
    }

    #[test]
    fn code_block_language_preserved() {
        let html = r#"<pre><code class="language-rust">fn main() {}</code></pre>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(md.contains("```rust"), "expected language annotation, got: {}", md);
        assert!(md.contains("fn main()"));
    }

    #[test]
    fn code_block_language_python() {
        let html = r#"<pre><code class="language-python">print("hello")</code></pre>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(md.contains("```python"), "expected language annotation, got: {}", md);
        assert!(md.contains("print"));
    }

    #[test]
    fn code_block_no_language_stays_plain() {
        let html = r#"<pre><code>plain code</code></pre>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(md.contains("```"));
        assert!(!md.contains("```rust"));
        assert!(!md.contains("```python"));
        assert!(md.contains("plain code"));
    }

    #[test]
    fn multiple_code_blocks_with_languages() {
        let html = r#"<pre><code class="language-rust">let x = 1;</code></pre><p>text</p><pre><code class="language-go">fmt.Println()</code></pre>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(md.contains("```rust"));
        assert!(md.contains("```go"));
        assert!(md.contains("let x = 1;"));
        assert!(md.contains("fmt.Println()"));
    }

    #[test]
    fn strips_header_tags_by_default() {
        let html = r#"<header><h1>Site Title</h1><nav>Menu</nav></header><p>Article body</p>"#;
        let md = PageToMarkdown::convert(html, false, false).unwrap();
        assert!(!md.contains("Site Title"));
        assert!(!md.contains("Menu"));
        assert!(md.contains("Article body"));
    }

    #[test]
    fn keeps_header_when_requested() {
        let html = r#"<header><h1>Article Title</h1></header><p>Article body</p>"#;
        let md = PageToMarkdown::convert(html, false, true).unwrap();
        assert!(md.contains("Article Title"));
        assert!(md.contains("Article body"));
    }
}
