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

/// Convert raw HTML to clean Markdown.
/// Strips scripts, styles, and non-essential markup to minimize token output.
pub struct PageToMarkdown;

impl PageToMarkdown {
    /// Convert HTML string to Markdown.
    /// When `include_images` is false, strips `<img>` tags to reduce token output.
    pub fn convert(html: &str, include_images: bool) -> Result<String> {
        let html = Self::strip_scripts_and_styles(html);
        let html = Self::strip_iframe_tags(&html);
        let html = if include_images { html } else { Self::strip_img_tags(&html) };
        let md = html2md::parse_html(&html);
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
        let md = PageToMarkdown::convert(html, false).unwrap();
        assert!(md.contains("Hello world"));
    }

    #[test]
    fn heading_conversion() {
        let html = "<h1>Title</h1><h2>Subtitle</h2>";
        let md = PageToMarkdown::convert(html, false).unwrap();
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
        let md = PageToMarkdown::convert(html, false).unwrap();
        assert!(!md.contains("alert"));
        assert!(!md.contains("color:red"));
        assert!(md.contains("Content"));
    }

    #[test]
    fn strips_images_when_false() {
        let html = r#"<p>Text before</p><img src="a.png" alt="pic"><p>Text after</p>"#;
        let md = PageToMarkdown::convert(html, false).unwrap();
        assert!(!md.contains("a.png"));
        assert!(!md.contains("pic"));
        assert!(md.contains("Text before"));
        assert!(md.contains("Text after"));
    }

    #[test]
    fn keeps_images_when_true() {
        let html = r#"<p>Text before</p><img src="a.png" alt="pic"><p>Text after</p>"#;
        let md = PageToMarkdown::convert(html, true).unwrap();
        assert!(md.contains("a.png"));
        assert!(md.contains("pic"));
        assert!(md.contains("Text before"));
        assert!(md.contains("Text after"));
    }

    #[test]
    fn strips_self_closing_images() {
        let html = r#"<p>Before</p><img src="b.png" alt="self"/><p>After</p>"#;
        let md = PageToMarkdown::convert(html, false).unwrap();
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
        let md = PageToMarkdown::convert(html, false).unwrap();
        assert!(!md.contains("iframe"));
        assert!(!md.contains("video.ibm.com"));
        assert!(md.contains("Before"));
        assert!(md.contains("After"));
    }

    #[test]
    fn strips_iframe_tags_self_closing() {
        let html = r#"<p>Before</p><iframe src="map.html"/><p>After</p>"#;
        let md = PageToMarkdown::convert(html, false).unwrap();
        assert!(!md.contains("iframe"));
        assert!(!md.contains("map.html"));
        assert!(md.contains("Before"));
        assert!(md.contains("After"));
    }
}
