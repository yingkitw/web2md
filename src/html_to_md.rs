//! Lightweight HTML-to-Markdown converter used instead of external crates.

use crate::html_util::{decode_html_entities, find_ci};

/// Convert an HTML fragment to Markdown.
pub fn parse_html(html: &str) -> String {
    let mut parser = HtmlToMd::new(html);
    let md = parser.parse_document();
    clean_markdown(&md)
}

struct HtmlToMd<'a> {
    html: &'a str,
    pos: usize,
    parents: Vec<String>,
}

impl<'a> HtmlToMd<'a> {
    fn new(html: &'a str) -> Self {
        Self {
            html,
            pos: 0,
            parents: Vec::new(),
        }
    }

    fn parse_document(&mut self) -> String {
        self.parse_nodes(false)
    }

    fn parse_nodes(&mut self, in_pre: bool) -> String {
        let mut out = String::new();
        while self.pos < self.html.len() {
            if let Some(rel) = self.html[self.pos..].find('<') {
                let abs = self.pos + rel;
                if abs > self.pos {
                    let text = &self.html[self.pos..abs];
                    self.append_text(&mut out, text, in_pre);
                }
                self.pos = abs;
                if self.html[self.pos..].starts_with("<!--") {
                    if let Some(end) = self.html[self.pos..].find("-->") {
                        self.pos += end + 3;
                        continue;
                    }
                }
                if self.html[self.pos..].starts_with("<![CDATA[") {
                    if let Some(end) = self.html[self.pos..].find("]]>") {
                        let content = &self.html[self.pos + 9..self.pos + end];
                        self.append_text(&mut out, content, in_pre);
                        self.pos += end + 3;
                        continue;
                    }
                }
                if self.try_consume_close_tag() {
                    break;
                }
                if let Some(node) = self.parse_element() {
                    out.push_str(&node);
                } else {
                    // Malformed tag — treat '<' as text.
                    self.append_text(&mut out, "<", in_pre);
                    self.pos += 1;
                }
            } else {
                self.append_text(&mut out, &self.html[self.pos..], in_pre);
                self.pos = self.html.len();
            }
        }
        out
    }

    fn try_consume_close_tag(&mut self) -> bool {
        if !self.html[self.pos..].starts_with("</") {
            return false;
        }
        let rest = &self.html[self.pos + 2..];
        let name_end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .unwrap_or(rest.len());
        if name_end == 0 {
            return false;
        }
        let tag = rest[..name_end].to_ascii_lowercase();
        if self.parents.last().is_some_and(|p| p == &tag) {
            if let Some(gt) = self.html[self.pos..].find('>') {
                self.pos += gt + 1;
                return true;
            }
        }
        false
    }

    fn parse_element(&mut self) -> Option<String> {
        if !self.html[self.pos..].starts_with('<') {
            return None;
        }
        let tag_start = self.pos;
        let gt = self.html[self.pos..].find('>')?;
        let tag_str = &self.html[self.pos..=self.pos + gt];
        self.pos += gt + 1;

        if tag_str.starts_with("<!") || tag_str.starts_with("<?") {
            return Some(String::new());
        }

        let tag_name = parse_tag_name(tag_str)?;
        let attrs = tag_str;
        let self_closing = tag_str.ends_with("/>") || is_void_tag(&tag_name);

        let in_pre = self.parents.iter().any(|t| t == "pre");
        let mut out = String::new();

        match tag_name.as_str() {
            "br" => out.push_str("  \n"),
            "hr" => {
                out.push('\n');
                out.push_str("---");
                out.push('\n');
            }
            "img" => {
                let src = get_attr(attrs, "src").unwrap_or_default();
                let alt = get_attr(attrs, "alt").unwrap_or_default();
                let title = get_attr(attrs, "title").map(|t| format!(" \"{t}\"")).unwrap_or_default();
                out.push_str(&format!("![{alt}]({src}{title})"));
            }
            "pre" => {
                out.push_str("\n```\n");
                if self_closing {
                    out.push_str("\n```\n");
                } else {
                    self.parents.push(tag_name.clone());
                    let inner = self.parse_nodes(true);
                    self.parents.pop();
                    out.push_str(&inner);
                    out.push_str("\n```\n");
                }
                return Some(out);
            }
            "code" if in_pre => {
                if self_closing {
                    return Some(String::new());
                }
                self.parents.push(tag_name.clone());
                let inner = self.parse_nodes(true);
                self.parents.pop();
                return Some(inner);
            }
            "code" => {
                if self_closing {
                    return Some("`".to_string());
                }
                self.parents.push(tag_name.clone());
                let inner = self.parse_nodes(false);
                self.parents.pop();
                return Some(format!("`{inner}`"));
            }
            "p" => {
                out.push_str("\n\n");
                if !self_closing {
                    self.parents.push(tag_name.clone());
                    out.push_str(&self.parse_nodes(false));
                    self.parents.pop();
                }
                out.push_str("\n\n");
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                out.push_str("\n\n");
                if !self_closing {
                    self.parents.push(tag_name.clone());
                    let inner = self.parse_nodes(false).trim().to_string();
                    self.parents.pop();
                    match tag_name.as_str() {
                        "h1" => {
                            out.push_str(&inner);
                            out.push_str("\n==========\n");
                        }
                        "h2" => {
                            out.push_str(&inner);
                            out.push_str("\n----------\n");
                        }
                        "h3" => out.push_str(&format!("### {inner} ###\n")),
                        "h4" => out.push_str(&format!("#### {inner} ####\n")),
                        "h5" => out.push_str(&format!("##### {inner} #####\n")),
                        "h6" => out.push_str(&format!("###### {inner} ######\n")),
                        _ => out.push_str(&inner),
                    }
                }
                out.push('\n');
            }
            "a" => {
                let href = get_attr(attrs, "href").unwrap_or_default();
                if self_closing {
                    out.push_str(&format!("[]({href})"));
                } else {
                    self.parents.push(tag_name.clone());
                    let inner = self.parse_nodes(false);
                    self.parents.pop();
                    out.push_str(&format!("[{inner}]({href})"));
                }
            }
            "strong" | "b" => {
                if !self_closing {
                    self.parents.push(tag_name.clone());
                    let inner = self.parse_nodes(false);
                    self.parents.pop();
                    let trimmed = inner.trim();
                    if !trimmed.is_empty() {
                        out.push_str("**");
                        out.push_str(trimmed);
                        out.push_str("**");
                    }
                }
            }
            "em" | "i" => {
                if !self_closing {
                    self.parents.push(tag_name.clone());
                    let inner = self.parse_nodes(false);
                    self.parents.pop();
                    let trimmed = inner.trim();
                    if !trimmed.is_empty() {
                        out.push('*');
                        out.push_str(trimmed);
                        out.push('*');
                    }
                }
            }
            "del" | "s" => {
                if !self_closing {
                    self.parents.push(tag_name.clone());
                    let inner = self.parse_nodes(false);
                    self.parents.pop();
                    let trimmed = inner.trim();
                    if !trimmed.is_empty() {
                        out.push_str("~~");
                        out.push_str(trimmed);
                        out.push_str("~~");
                    }
                }
            }
            "blockquote" | "q" | "cite" => {
                out.push('\n');
                if !self_closing {
                    self.parents.push(tag_name.clone());
                    let inner = self.parse_nodes(false);
                    self.parents.pop();
                    for line in inner.lines() {
                        let t = line.trim();
                        if !t.is_empty() {
                            out.push_str("> ");
                            out.push_str(t);
                            out.push('\n');
                        }
                    }
                }
            }
            "ul" | "ol" | "menu" => {
                out.push_str("\n\n");
                if !self_closing {
                    self.parents.push(tag_name.clone());
                    out.push_str(&self.parse_list_items(&tag_name));
                    self.parents.pop();
                }
                out.push_str("\n\n");
            }
            "li" => {
                // Handled by parse_list_items; bare <li> outside a list.
                if !self_closing {
                    self.parents.push(tag_name.clone());
                    out.push_str("* ");
                    out.push_str(&self.parse_nodes(false).trim());
                    self.parents.pop();
                    out.push('\n');
                }
            }
            "div" | "section" | "header" | "footer" | "article" | "main" | "nav" | "aside"
            | "figure" | "figcaption" | "details" | "summary" => {
                out.push_str("\n\n");
                if !self_closing {
                    self.parents.push(tag_name.clone());
                    out.push_str(&self.parse_nodes(false));
                    self.parents.pop();
                }
                out.push_str("\n\n");
            }
            "table" => {
                out.push('\n');
                if !self_closing {
                    self.parents.push(tag_name.clone());
                    out.push_str(&self.parse_table());
                    self.parents.pop();
                }
                out.push('\n');
            }
            "html" | "head" | "body" | "span" | "label" | "small" | "sub" | "sup" | "time"
            | "abbr" | "mark" | "td" | "th" | "tr" | "tbody" | "thead" | "tfoot" | "dl" | "dt"
            | "dd" | "form" | "input" | "button" | "select" | "option" | "textarea" | "video"
            | "audio" | "source" | "iframe" | "noscript" | "script" | "style" | "meta" | "link"
            | "title" => {
                if !self_closing {
                    self.parents.push(tag_name.clone());
                    out.push_str(&self.parse_nodes(false));
                    self.parents.pop();
                }
            }
            _ => {
                if !self_closing {
                    self.parents.push(tag_name.clone());
                    out.push_str(&self.parse_nodes(false));
                    self.parents.pop();
                }
            }
        }

        let _ = tag_start;
        Some(out)
    }

    fn parse_list_items(&mut self, list_type: &str) -> String {
        let mut out = String::new();
        let mut index = 1usize;
        while self.pos < self.html.len() {
            self.skip_ws_and_comments();
            if self.try_consume_close_tag() {
                break;
            }
            if !self.html[self.pos..].starts_with('<') {
                self.pos += 1;
                continue;
            }
            let Some(tag_name) = parse_tag_name(&self.html[self.pos..self.html.len().min(self.pos + 64)])
            else {
                self.pos += 1;
                continue;
            };
            if tag_name != "li" {
                break;
            }
            let gt = match self.html[self.pos..].find('>') {
                Some(g) => g,
                None => break,
            };
            self.pos += gt + 1;

            if out.is_empty() || out.ends_with('\n') {
                // ok
            } else {
                out.push('\n');
            }
            match list_type {
                "ol" => {
                    out.push_str(&format!("{index}. "));
                    index += 1;
                }
                _ => out.push_str("* "),
            }
            self.parents.push("li".to_string());
            let inner = self.parse_nodes(false);
            self.parents.pop();
            out.push_str(inner.trim());
            out.push('\n');
        }
        out
    }

    fn parse_table(&mut self) -> String {
        let mut rows: Vec<Vec<String>> = Vec::new();
        while self.pos < self.html.len() {
            self.skip_ws_and_comments();
            if self.try_consume_close_tag() {
                break;
            }
            if !self.html[self.pos..].starts_with('<') {
                self.pos += 1;
                continue;
            }
            let Some(tag_name) = parse_tag_name(&self.html[self.pos..self.html.len().min(self.pos + 64)])
            else {
                self.pos += 1;
                continue;
            };
            match tag_name.as_str() {
                "tr" => {
                    let gt = self.html[self.pos..].find('>').unwrap_or(0);
                    self.pos += gt + 1;
                    self.parents.push("tr".to_string());
                    let row = self.parse_table_row();
                    self.parents.pop();
                    if !row.is_empty() {
                        rows.push(row);
                    }
                }
                "thead" | "tbody" | "tfoot" => {
                    let gt = self.html[self.pos..].find('>').unwrap_or(0);
                    self.pos += gt + 1;
                    self.parents.push(tag_name.clone());
                    // Continue loop to find tr elements.
                }
                _ => break,
            }
        }
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

    fn parse_table_row(&mut self) -> Vec<String> {
        let mut cells = Vec::new();
        while self.pos < self.html.len() {
            self.skip_ws_and_comments();
            if self.try_consume_close_tag() {
                break;
            }
            if !self.html[self.pos..].starts_with('<') {
                self.pos += 1;
                continue;
            }
            let Some(tag_name) = parse_tag_name(&self.html[self.pos..self.html.len().min(self.pos + 64)])
            else {
                self.pos += 1;
                continue;
            };
            if tag_name != "td" && tag_name != "th" {
                break;
            }
            let gt = self.html[self.pos..].find('>').unwrap_or(0);
            self.pos += gt + 1;
            self.parents.push(tag_name.clone());
            let inner = self.parse_nodes(false);
            self.parents.pop();
            cells.push(inner.trim().replace('\n', " "));
        }
        cells
    }

    fn should_escape_text(&self, in_pre: bool) -> bool {
        !in_pre && !self.parents.iter().any(|p| p == "code")
    }

    fn append_text(&self, out: &mut String, text: &str, in_pre: bool) {
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
        let escaped = if self.should_escape_text(in_pre) {
            escape_markdown_text(&collapsed, at_line_start(out))
        } else {
            collapsed
        };
        if !out.is_empty() {
            let last = out.chars().last().unwrap();
            let first = escaped.chars().next().unwrap();
            if !last.is_whitespace() && !first.is_whitespace() {
                out.push(' ');
            }
        }
        out.push_str(&escaped);
    }

    fn skip_ws_and_comments(&mut self) {
        while self.pos < self.html.len() {
            if self.html[self.pos..].starts_with("<!--") {
                if let Some(end) = self.html[self.pos..].find("-->") {
                    self.pos += end + 3;
                    continue;
                }
            }
            if self.html.as_bytes().get(self.pos).is_some_and(|b| b.is_ascii_whitespace()) {
                self.pos += 1;
            } else {
                break;
            }
        }
    }
}

fn parse_tag_name(tag: &str) -> Option<String> {
    let rest = tag.strip_prefix('<')?;
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    if rest.is_empty() || rest.starts_with('!') {
        return None;
    }
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    Some(rest[..end].to_ascii_lowercase())
}

fn is_void_tag(name: &str) -> bool {
    matches!(
        name,
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta"
            | "param" | "source" | "track" | "wbr"
    )
}

fn get_attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let pos = find_ci(tag, &needle)?;
    let after = &tag[pos + needle.len()..];
    let mut i = 0;
    while i < after.len() && after.as_bytes()[i].is_ascii_whitespace() {
        i += 1;
    }
    let quote = *after.as_bytes().get(i)? as char;
    if quote != '"' && quote != '\'' {
        // Unquoted attribute value.
        let val_end = after[i..]
            .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
            .unwrap_or(after.len() - i);
        return Some(after[i..i + val_end].to_string());
    }
    let val_start = i + 1;
    let val_end = after[val_start..].find(quote)? + val_start;
    Some(after[val_start..val_end].to_string())
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
}
