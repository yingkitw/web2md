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
        convert_children(body, &mut out, false, false);
    }
    clean_markdown(&out)
}

fn convert_children(element: ElementRef<'_>, out: &mut String, in_pre: bool, in_code: bool) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => append_text(out, text, in_pre, in_code),
            Node::Element(_) => {
                if let Some(el) = ElementRef::wrap(child) {
                    convert_element(el, out, in_pre, in_code);
                }
            }
            _ => {}
        }
    }
}

fn convert_element(element: ElementRef<'_>, out: &mut String, in_pre: bool, in_code: bool) {
    let tag = element.value().name();
    match tag {
        "br" => out.push_str("  \n"),
        "hr" => {
            out.push('\n');
            out.push_str("---");
            out.push('\n');
        }
        "img" => {
            let src = element.value().attr("src").unwrap_or("");
            let alt = element.value().attr("alt").unwrap_or("");
            let title = element
                .value()
                .attr("title")
                .map(|t| format!(" \"{t}\""))
                .unwrap_or_default();
            out.push_str(&format!("![{alt}]({src}{title})"));
        }
        "pre" => {
            out.push_str("\n```\n");
            convert_children(element, out, true, false);
            out.push_str("\n```\n");
        }
        "code" if in_pre => convert_children(element, out, true, true),
        "code" => {
            let mut inner = String::new();
            convert_children(element, &mut inner, false, true);
            out.push('`');
            out.push_str(inner.trim());
            out.push('`');
        }
        "p" => {
            out.push_str("\n\n");
            convert_children(element, out, in_pre, in_code);
            out.push_str("\n\n");
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            out.push_str("\n\n");
            let mut inner = String::new();
            convert_children(element, &mut inner, false, false);
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
            let mut inner = String::new();
            convert_children(element, &mut inner, false, false);
            out.push_str(&format!("[{inner}]({href})"));
        }
        "strong" | "b" => {
            let mut inner = String::new();
            convert_children(element, &mut inner, false, false);
            let trimmed = inner.trim();
            if !trimmed.is_empty() {
                out.push_str("**");
                out.push_str(trimmed);
                out.push_str("**");
            }
        }
        "em" | "i" => {
            let mut inner = String::new();
            convert_children(element, &mut inner, false, false);
            let trimmed = inner.trim();
            if !trimmed.is_empty() {
                out.push('*');
                out.push_str(trimmed);
                out.push('*');
            }
        }
        "del" | "s" => {
            let mut inner = String::new();
            convert_children(element, &mut inner, false, false);
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
            convert_children(element, &mut inner, false, false);
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
            convert_children(element, &mut inner, in_pre, in_code);
            out.push_str(inner.trim());
            out.push('\n');
        }
        "div" | "section" | "header" | "footer" | "article" | "main" | "nav" | "aside"
        | "figure" | "figcaption" | "details" | "summary" => {
            out.push_str("\n\n");
            convert_children(element, out, in_pre, in_code);
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
        | "title" => convert_children(element, out, in_pre, in_code),
        _ => convert_children(element, out, in_pre, in_code),
    }
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
        convert_children(li, &mut inner, false, false);
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
                    convert_children(cell, &mut inner, false, false);
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
    out.trim().to_string()
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
}
