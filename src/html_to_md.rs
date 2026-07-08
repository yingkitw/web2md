//! Lightweight HTML-to-Markdown converter used instead of external crates.
//! Parses HTML with `scraper` (html5ever) for tolerance of malformed markup.

use crate::html_util::decode_html_entities;
use scraper::{ElementRef, Html, Node, Selector};

/// Convert an HTML fragment to Markdown.
pub fn parse_html(html: &str) -> String {
    let document = Html::parse_document(html);
    let body_sel = Selector::parse("body").expect("valid selector");
    let mut out = String::new();
    if let Some(body) = document.select(&body_sel).next() {
        convert_children(body, &mut out, false, false, false);
    }
    clean_markdown(&out)
}

/// Convert HTML to Markdown, invoking `on_block` once per top-level content block.
pub fn parse_html_progressive(html: &str, mut on_block: impl FnMut(String)) {
    let document = Html::parse_document(html);
    let body_sel = Selector::parse("body").expect("valid selector");
    if let Some(body) = document.select(&body_sel).next() {
        let root = unwrap_progressive_root(body);
        emit_progressive_blocks(root, &mut on_block);
    }
}

/// Skip single wrapper containers so progressive output is not one giant block.
fn unwrap_progressive_root(mut element: ElementRef<'_>) -> ElementRef<'_> {
    loop {
        let mut child_elements = Vec::new();
        let mut has_text = false;
        for child in element.children() {
            match child.value() {
                Node::Text(text) => {
                    if !collapse_whitespace(&decode_html_entities(text)).is_empty() {
                        has_text = true;
                    }
                }
                Node::Element(_) => {
                    if let Some(el) = ElementRef::wrap(child) {
                        child_elements.push(el);
                    }
                }
                _ => {}
            }
        }
        if has_text || child_elements.len() != 1 {
            break;
        }
        let only = child_elements[0];
        if !matches!(
            only.value().name(),
            "div" | "section" | "main" | "article" | "span"
        ) {
            break;
        }
        element = only;
    }
    element
}

fn emit_progressive_blocks(element: ElementRef<'_>, on_block: &mut impl FnMut(String)) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => {
                let collapsed = collapse_whitespace(&decode_html_entities(text));
                if !collapsed.is_empty() {
                    on_block(collapsed);
                }
            }
            Node::Element(_) => {
                if let Some(el) = ElementRef::wrap(child) {
                    let mut block = String::new();
                    convert_element(el, &mut block, false, false, false);
                    let cleaned = clean_markdown(&block);
                    if !cleaned.is_empty() {
                        on_block(cleaned);
                    }
                }
            }
            _ => {}
        }
    }
}

fn convert_children(
    element: ElementRef<'_>,
    out: &mut String,
    in_pre: bool,
    in_code: bool,
    in_anchor: bool,
) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => append_text(out, text, in_pre, in_code),
            Node::Element(_) => {
                if let Some(el) = ElementRef::wrap(child) {
                    convert_element(el, out, in_pre, in_code, in_anchor);
                }
            }
            _ => {}
        }
    }
}

fn convert_element(
    element: ElementRef<'_>,
    out: &mut String,
    in_pre: bool,
    in_code: bool,
    in_anchor: bool,
) {
    let tag = element.value().name();
    match tag {
        "br" => out.push_str("  \n"),
        "hr" => {
            out.push('\n');
            out.push_str("---");
            out.push('\n');
        }
        "img" => {
            if in_anchor {
                if let Some(alt) = element.value().attr("alt") {
                    if !alt.is_empty() {
                        append_text(out, alt, in_pre, in_code);
                    }
                }
            } else {
                ensure_inline_break(out);
                let src = element.value().attr("src").unwrap_or("");
                let alt = element.value().attr("alt").unwrap_or("");
                let title = element
                    .value()
                    .attr("title")
                    .map(|t| format!(" \"{t}\""))
                    .unwrap_or_default();
                out.push_str(&format!("![{alt}]({src}{title})"));
            }
        }
        "pre" => {
            out.push_str("\n```\n");
            convert_children(element, out, true, false, in_anchor);
            out.push_str("\n```\n");
        }
        "code" if in_pre => convert_children(element, out, true, true, in_anchor),
        "code" => {
            ensure_inline_break(out);
            let mut inner = String::new();
            convert_children(element, &mut inner, false, true, in_anchor);
            out.push('`');
            out.push_str(inner.trim());
            out.push('`');
        }
        "p" => {
            out.push_str("\n\n");
            convert_children(element, out, in_pre, in_code, in_anchor);
            out.push_str("\n\n");
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            out.push_str("\n\n");
            let mut inner = String::new();
            convert_children(element, &mut inner, false, false, in_anchor);
            let inner = inner.trim();
            match tag {
                "h1" => {
                    out.push_str(inner);
                    out.push_str("\n==========\n");
                }
                "h2" => {
                    out.push_str(inner);
                    out.push_str("\n----------\n");
                }
                "h3" => out.push_str(&format!("### {inner} ###\n")),
                "h4" => out.push_str(&format!("#### {inner} ####\n")),
                "h5" => out.push_str(&format!("##### {inner} #####\n")),
                "h6" => out.push_str(&format!("###### {inner} ######\n")),
                _ => out.push_str(inner),
            }
            out.push('\n');
        }
        "a" => {
            let href = element.value().attr("href").unwrap_or("");
            if href.is_empty() || href == "#" {
                convert_children(element, out, in_pre, in_code, in_anchor);
                return;
            }
            ensure_inline_break(out);
            let mut inner = String::new();
            convert_children(element, &mut inner, false, false, true);
            let label = link_label(element, &inner, href);
            out.push_str(&format!("[{label}]({href})"));
        }
        "strong" | "b" => {
            ensure_inline_break(out);
            let mut inner = String::new();
            convert_children(element, &mut inner, false, false, in_anchor);
            let trimmed = inner.trim();
            if !trimmed.is_empty() {
                out.push_str("**");
                out.push_str(trimmed);
                out.push_str("**");
            }
        }
        "em" | "i" => {
            let mut inner = String::new();
            convert_children(element, &mut inner, false, false, in_anchor);
            let trimmed = inner.trim();
            if !trimmed.is_empty() {
                out.push('*');
                out.push_str(trimmed);
                out.push('*');
            }
        }
        "del" | "s" => {
            let mut inner = String::new();
            convert_children(element, &mut inner, false, false, in_anchor);
            let trimmed = inner.trim();
            if !trimmed.is_empty() {
                out.push_str("~~");
                out.push_str(trimmed);
                out.push_str("~~");
            }
        }
        "blockquote" | "q" | "cite" => {
            out.push('\n');
            let mut inner = String::new();
            convert_children(element, &mut inner, false, false, in_anchor);
            for line in inner.lines() {
                let t = line.trim();
                if !t.is_empty() {
                    out.push_str("> ");
                    out.push_str(t);
                    out.push('\n');
                }
            }
        }
        "ul" | "ol" | "menu" => convert_list(element, out, tag == "ol"),
        "li" => {
            out.push_str("* ");
            let mut inner = String::new();
            convert_children(element, &mut inner, in_pre, in_code, in_anchor);
            out.push_str(inner.trim());
            out.push('\n');
        }
        "div" | "section" | "header" | "footer" | "article" | "main" | "nav" | "aside"
        | "figure" | "figcaption" | "details" | "summary" => {
            out.push_str("\n\n");
            convert_children(element, out, in_pre, in_code, in_anchor);
            out.push_str("\n\n");
        }
        "table" => {
            out.push('\n');
            out.push_str(&convert_table(element));
            out.push('\n');
        }
        "html" | "head" | "body" | "span" | "label" | "small" | "sub" | "sup" | "time"
        | "abbr" | "mark" | "td" | "th" | "tr" | "tbody" | "thead" | "tfoot" | "dl" | "dt"
        | "dd" | "form" | "input" | "button" | "select" | "option" | "textarea" | "video"
        | "audio" | "source" | "iframe" | "noscript" | "script" | "style" | "meta" | "link"
        | "title" => convert_children(element, out, in_pre, in_code, in_anchor),
        _ => convert_children(element, out, in_pre, in_code, in_anchor),
    }
}

/// Pick link text from inner content or accessibility / URL fallbacks.
fn link_label(element: ElementRef<'_>, inner: &str, href: &str) -> String {
    let collapsed = inner.split_whitespace().collect::<Vec<_>>().join(" ");
    if !collapsed.is_empty() {
        return collapsed;
    }
    for attr in ["aria-label", "title", "label"] {
        if let Some(val) = element.value().attr(attr) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    href_label_from_url(href)
}

fn href_label_from_url(href: &str) -> String {
    if let Ok(url) = url::Url::parse(href) {
        if let Some(segments) = url.path_segments() {
            if let Some(seg) = segments.filter(|s| !s.is_empty()).last() {
                return seg.replace('-', " ");
            }
        }
        if let Some(host) = url.host_str() {
            return host.to_string();
        }
    }
    href.to_string()
}

fn convert_list(element: ElementRef<'_>, out: &mut String, ordered: bool) {
    out.push_str("\n\n");
    let mut index = 1usize;
    for child in element.children() {
        let Some(li) = ElementRef::wrap(child) else {
            continue;
        };
        if li.value().name() != "li" {
            continue;
        }
        if ordered {
            out.push_str(&format!("{index}. "));
            index += 1;
        } else {
            out.push_str("* ");
        }
        let mut inner = String::new();
        convert_children(li, &mut inner, false, false, false);
        out.push_str(inner.trim());
        out.push('\n');
    }
    out.push_str("\n\n");
}

fn convert_table(element: ElementRef<'_>) -> String {
    let tr_sel = Selector::parse("tr").expect("valid selector");
    let cell_sel = Selector::parse("td, th").expect("valid selector");
    let rows: Vec<Vec<String>> = element
        .select(&tr_sel)
        .map(|tr| {
            tr.select(&cell_sel)
                .map(|cell| {
                    let mut inner = String::new();
                    convert_children(cell, &mut inner, false, false, false);
                    inner.trim().replace('\n', " ")
                })
                .collect::<Vec<_>>()
        })
        .filter(|row: &Vec<String>| !row.is_empty())
        .collect();
    if rows.is_empty() {
        return String::new();
    }
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        out.push('|');
        for c in 0..col_count {
            let cell = row.get(c).map(|s| s.as_str()).unwrap_or("");
            out.push(' ');
            out.push_str(&cell.replace('|', "\\|"));
            out.push_str(" |");
        }
        out.push('\n');
        if i == 0 {
            out.push('|');
            for _ in 0..col_count {
                out.push_str(" --- |");
            }
            out.push('\n');
        }
    }
    out
}

fn append_text(out: &mut String, text: &str, in_pre: bool, in_code: bool) {
    let text = if in_pre {
        text.to_string()
    } else {
        decode_html_entities(text)
    };
    if in_pre {
        out.push_str(&text);
        return;
    }
    let collapsed = collapse_whitespace(&text);
    if collapsed.is_empty() {
        return;
    }
    let escaped = if !in_code {
        escape_markdown_text(&collapsed, at_line_start(out))
    } else {
        collapsed
    };
    if !in_code && !out.is_empty() {
        let last = out.chars().last().unwrap();
        let first = escaped.chars().next().unwrap();
        if !last.is_whitespace() && !first.is_whitespace() {
            out.push(' ');
        }
    }
    out.push_str(&escaped);
}

fn at_line_start(out: &str) -> bool {
    let line = match out.rfind('\n') {
        None => out,
        Some(i) => &out[i + 1..],
    };
    line.chars().all(|c| c.is_whitespace())
}

/// Escape characters that would be interpreted as Markdown syntax in plain text.
fn escape_markdown_text(text: &str, at_line_start: bool) -> String {
    let line_start_special = ['=', '>', '+', '-', '#'];
    let mut result = String::with_capacity(text.len() + 4);
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    if at_line_start {
        while i < chars.len() && chars[i].is_whitespace() {
            result.push(chars[i]);
            i += 1;
        }
        if i < chars.len() && line_start_special.contains(&chars[i]) {
            result.push('\\');
        }
    }

    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '<' | '>' | '*' | '\\' | '_' | '~') {
            result.push('\\');
        }
        result.push(c);
        i += 1;
    }
    result
}

fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

/// Insert a line break between adjacent inline Markdown (links/images) and a space elsewhere.
fn ensure_inline_break(out: &mut String) {
    if out.is_empty() {
        return;
    }
    if out.ends_with(')') {
        out.push('\n');
        return;
    }
    match out.chars().last() {
        Some(c) if !c.is_whitespace() => out.push(' '),
        _ => {}
    }
}

fn clean_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0usize;
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
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
    separate_adjacent_markdown(&out.trim().to_string())
}

/// Break concatenated `[link](url)[link2](url)` patterns onto separate lines.
fn separate_adjacent_markdown(text: &str) -> String {
    text.replace(")[", ")\n[")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_paragraph() {
        assert!(parse_html("<p>Hello world</p>").contains("Hello world"));
    }

    #[test]
    fn converts_link() {
        let md = parse_html(r#"<a href="/about">About</a>"#);
        assert!(md.contains("[About](/about)"));
    }

    #[test]
    fn link_uses_aria_label_when_text_empty() {
        let md = parse_html(
            r#"<a href="https://example.com/trial" aria-label="Try the product"></a>"#,
        );
        assert!(md.contains("[Try the product](https://example.com/trial)"));
    }

    #[test]
    fn link_inside_anchor_uses_img_alt_not_nested_image() {
        let md = parse_html(
            r#"<a href="/case-study"><img src="logo.png" alt="Wimbledon logo"><span>Explore Wimbledon</span></a>"#,
        );
        assert!(md.contains("[Wimbledon logo Explore Wimbledon](/case-study)"));
        assert!(!md.contains("![Wimbledon logo]"));
    }

    #[test]
    fn adjacent_links_get_line_breaks() {
        let md = parse_html(
            r#"<div><a href="/one">First</a><a href="/two">Second</a></div>"#,
        );
        assert!(md.contains("[First](/one)"));
        assert!(md.contains("[Second](/two)"));
        assert!(!md.contains("(/one)[Second]"));
        assert!(md.contains('\n'));
    }

    #[test]
    fn converts_image() {
        let md = parse_html(r#"<img src="a.png" alt="pic">"#);
        assert!(md.contains("![pic](a.png)"));
    }

    #[test]
    fn converts_code_block() {
        let md = parse_html("<pre><code>fn main() {}</code></pre>");
        assert!(md.contains("```"));
        assert!(md.contains("fn main()"));
    }

    #[test]
    fn converts_list() {
        let md = parse_html("<ul><li>one</li><li>two</li></ul>");
        assert!(md.contains("one"));
        assert!(md.contains("two"));
    }

    #[test]
    fn decodes_html_entities_in_text() {
        let md = parse_html("<p>Tom &amp; Jerry</p>");
        assert!(md.contains("Tom & Jerry"));
    }

    #[test]
    fn converts_nested_bold_and_italic() {
        let md = parse_html("<p><strong>bold</strong> and <em>italic</em></p>");
        assert!(md.contains("**bold**"));
        assert!(md.contains("*italic*"));
    }

    #[test]
    fn converts_heading() {
        let md = parse_html("<h1>Title</h1>");
        assert!(md.contains("Title"));
    }

    #[test]
    fn escapes_bold_markers_in_text() {
        let md = parse_html("<p>* not a list item</p>");
        assert!(md.contains(r"\* not a list item"));
    }

    #[test]
    fn escapes_heading_marker_at_line_start() {
        let md = parse_html("<p># not a heading</p>");
        assert!(md.contains(r"\# not a heading"));
    }

    #[test]
    fn escapes_underscores_in_text() {
        let md = parse_html("<p>use __init__ method</p>");
        assert!(md.contains(r"use \_\_init\_\_ method"));
    }

    #[test]
    fn does_not_escape_inside_preformatted_text() {
        let md = parse_html("<pre>* raw # text</pre>");
        assert!(md.contains("* raw # text"));
        assert!(!md.contains(r"\* raw"));
    }

    #[test]
    fn does_not_escape_inside_inline_code() {
        let md = parse_html("<p>run <code>*args</code> here</p>");
        assert!(md.contains("`*args`"));
        assert!(!md.contains(r"\*args"));
    }

    #[test]
    fn handles_unclosed_tags() {
        let md = parse_html("<div><p>First paragraph<p>Second paragraph</div>");
        assert!(md.contains("First paragraph"));
        assert!(md.contains("Second paragraph"));
    }

    #[test]
    fn handles_mismatched_close_tags() {
        let md = parse_html("<p>Content</div><p>More content</p>");
        assert!(md.contains("Content"));
        assert!(md.contains("More content"));
    }

    #[test]
    fn converts_table() {
        let md = parse_html(
            "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>",
        );
        assert!(md.contains("| A | B |"));
        assert!(md.contains("| 1 | 2 |"));
    }

    #[test]
    fn parse_html_progressive_emits_multiple_blocks() {
        let html = r#"<body><div><h1>Title</h1><p>First</p><p>Second</p></div></body>"#;
        let mut blocks = Vec::new();
        parse_html_progressive(html, |block| blocks.push(block));
        assert!(blocks.len() >= 2, "expected multiple blocks, got {:?}", blocks);
        assert!(blocks.iter().any(|b| b.contains("Title")));
        assert!(blocks.iter().any(|b| b.contains("First")));
    }
}
