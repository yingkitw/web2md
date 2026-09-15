//! ANSI terminal rendering for Markdown: styled output, numbered links,
//! and unicode table drawing.

pub(crate) struct AnsiRenderer {
    link_counter: usize,
    links: Vec<String>,
    number_links: bool,
    out: String,
    pending_link: Option<(usize, String)>,
    in_table: bool,
    table_rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    col_alignments: Vec<pulldown_cmark::Alignment>,
    _in_header: bool,
}

impl AnsiRenderer {
    fn new(number_links: bool) -> Self {
        Self {
            link_counter: 0,
            links: Vec::new(),
            number_links,
            out: String::new(),
            pending_link: None,
            in_table: false,
            table_rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: String::new(),
            col_alignments: Vec::new(),
            _in_header: false,
        }
    }

    fn render_chunk(&mut self, md: &str) -> String {
        use pulldown_cmark::{Event, Options, Tag, TagEnd};

        self.out.clear();
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_TABLES);
        let parser = pulldown_cmark::Parser::new_ext(md, opts);

        for event in parser {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Heading { level, .. } => {
                        let c = match level {
                            pulldown_cmark::HeadingLevel::H1 => "\x1b[1;91m",
                            pulldown_cmark::HeadingLevel::H2 => "\x1b[1;93m",
                            pulldown_cmark::HeadingLevel::H3 => "\x1b[1;92m",
                            pulldown_cmark::HeadingLevel::H4 => "\x1b[1;94m",
                            pulldown_cmark::HeadingLevel::H5 => "\x1b[1;95m",
                            pulldown_cmark::HeadingLevel::H6 => "\x1b[1;96m",
                        };
                        self.out.push_str(c);
                    }
                    Tag::Strong => self.out.push_str("\x1b[1m"),
                    Tag::Emphasis => self.out.push_str("\x1b[3m"),
                    Tag::Link { dest_url, .. } => {
                        if self.number_links && !self.in_table {
                            self.link_counter += 1;
                            self.pending_link = Some((self.link_counter, dest_url.to_string()));
                        }
                        if self.in_table {
                            self.current_cell.push_str("\x1b[4;36m");
                        } else {
                            self.out.push_str("\x1b[4;36m");
                        }
                    }
                    Tag::BlockQuote(_) => self.out.push_str("\x1b[90m▌ \x1b[3m"),
                    Tag::CodeBlock(_) => self.out.push_str("\x1b[48;5;235;38;5;250m"),
                    Tag::List(_) => {}
                    Tag::Item => self.out.push_str("  • "),
                    Tag::Table(aligns) => {
                        self.in_table = true;
                        self.col_alignments = aligns.to_vec();
                    }
                    Tag::TableHead => self._in_header = true,
                    Tag::TableRow => self.current_row = Vec::new(),
                    Tag::TableCell => self.current_cell.clear(),
                    _ => {}
                },
                Event::End(tag) => match tag {
                    TagEnd::Heading(_) => self.out.push_str("\x1b[0m\n"),
                    TagEnd::Paragraph => {
                        if !self.in_table {
                            self.out.push('\n');
                        }
                    }
                    TagEnd::Strong | TagEnd::Emphasis => {
                        if self.in_table {
                            self.current_cell.push_str("\x1b[0m");
                        } else {
                            self.out.push_str("\x1b[0m");
                        }
                    }
                    TagEnd::Link => {
                        if self.in_table {
                            self.current_cell.push_str("\x1b[0m");
                        } else {
                            if let Some((n, url)) = self.pending_link.take() {
                                self.links.push(url.clone());
                                self.out
                                    .push_str(&format!("\x1b[33m[{}]\x1b[0m ", n));
                                self.out.push_str(&fallback_link_label(&url));
                            }
                            self.out.push_str("\x1b[0m");
                        }
                    }
                    TagEnd::BlockQuote(_) => self.out.push_str("\x1b[0m\n"),
                    TagEnd::CodeBlock => self.out.push_str("\x1b[0m\n"),
                    TagEnd::Item => {}
                    TagEnd::TableCell => {
                        self.current_row.push(std::mem::take(&mut self.current_cell));
                    }
                    TagEnd::TableRow => {
                        self.table_rows.push(std::mem::take(&mut self.current_row));
                    }
                    TagEnd::TableHead => {
                        self._in_header = false;
                        if !self.current_row.is_empty() {
                            self.table_rows.push(std::mem::take(&mut self.current_row));
                        }
                    }
                    TagEnd::Table => {
                        self.in_table = false;
                        self.out.push_str(&render_ansi_table(
                            &self.table_rows,
                            &self.col_alignments,
                        ));
                        self.table_rows.clear();
                        self.col_alignments.clear();
                    }
                    _ => {}
                },
                Event::Text(text) => {
                    if self.in_table {
                        self.current_cell.push_str(&text);
                    } else if let Some((n, url)) = self.pending_link.take() {
                        self.links.push(url);
                        self.out
                            .push_str(&format!("\x1b[33m[{}]\x1b[0m ", n));
                        self.out.push_str(&text);
                    } else {
                        self.out.push_str(&text);
                    }
                }
                Event::Code(code) => {
                    let s = format!("\x1b[38;5;250m{}\x1b[0m", code);
                    if self.in_table {
                        self.current_cell.push_str(&s);
                    } else {
                        self.out.push_str(&s);
                    }
                }
                Event::Html(html) => {
                    if self.in_table {
                        self.current_cell.push_str(&html);
                    } else {
                        self.out.push_str(&html);
                    }
                }
                Event::SoftBreak => {
                    if self.in_table {
                        self.current_cell.push(' ');
                    } else {
                        self.out.push(' ');
                    }
                }
                Event::HardBreak => {
                    if self.in_table {
                        self.current_cell.push('\n');
                    } else {
                        self.out.push('\n');
                    }
                }
                Event::Rule => {
                    self.out.push_str(
                        "\x1b[90m────────────────────────────────────────\x1b[0m\n",
                    );
                }
                _ => {}
            }
        }

        self.out = fix_raw_links(
            &self.out,
            self.number_links,
            &mut self.link_counter,
            &mut self.links,
        );
        std::mem::take(&mut self.out)
    }

    fn into_links(self) -> Vec<String> {
        self.links
    }
}

/// Render Markdown with ANSI escape codes for terminal display.
/// Markdown syntax is stripped; visual effects (bold, color, underline) replace it.
pub(crate) fn render_markdown_ansi(md: &str, number_links: bool) -> (String, Vec<String>) {
    let mut renderer = AnsiRenderer::new(number_links);
    let rendered = renderer.render_chunk(md);
    (rendered, renderer.into_links())
}

/// Display label for links with no visible anchor text.
fn fallback_link_label(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(mut segments) = parsed.path_segments()
            && let Some(seg) = segments.rfind(|s| !s.is_empty()) {
                return seg.replace('-', " ");
            }
        if let Some(host) = parsed.host_str() {
            return host.to_string();
        }
    }
    url.to_string()
}

/// Strip ANSI escape sequences from a string to get the visual width.
pub(crate) fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
            continue;
        }
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Compute visual width of a string (ANSI codes excluded).
fn visual_width(s: &str) -> usize {
    strip_ansi(s).chars().count()
}

/// Pad a string to target visual width, preserving any ANSI prefixes/suffixes.
fn pad_visual(s: &str, width: usize, align: &pulldown_cmark::Alignment) -> String {
    let stripped = strip_ansi(s);
    let stripped_len = stripped.chars().count();
    if stripped_len >= width {
        return s.to_string();
    }
    let pad = width - stripped_len;
    match align {
        pulldown_cmark::Alignment::Right => format!("{}{}", " ".repeat(pad), s),
        pulldown_cmark::Alignment::Center => {
            let left = pad / 2;
            let right = pad - left;
            format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
        }
        _ => format!("{}{}", s, " ".repeat(pad)),
    }
}

/// Render buffered table rows as an ANSI-styled box-drawing table.
fn render_ansi_table(rows: &[Vec<String>], aligns: &[pulldown_cmark::Alignment]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return String::new();
    }

    let mut widths = vec![0usize; cols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(visual_width(cell));
        }
    }
    for w in &mut widths {
        *w = (*w).max(1);
    }

    let mut out = String::new();

    // Top border
    out.push_str("\x1b[90m┌");
    for (i, w) in widths.iter().enumerate() {
        out.push_str(&"─".repeat(w + 2));
        if i < widths.len() - 1 {
            out.push('┬');
        }
    }
    out.push_str("┐\x1b[0m\n");

    for (ri, row) in rows.iter().enumerate() {
        let is_header = ri == 0 && row.len() == cols;
        out.push_str("\x1b[90m│\x1b[0m ");
        #[allow(clippy::needless_range_loop)]
        for ci in 0..cols {
            let cell = row.get(ci).map(|s| s.as_str()).unwrap_or("");
            let align = aligns.get(ci).unwrap_or(&pulldown_cmark::Alignment::None);
            let padded = pad_visual(cell, widths[ci], align);
            if is_header {
                out.push_str("\x1b[1m");
                out.push_str(&padded);
                out.push_str("\x1b[0m");
            } else {
                out.push_str(&padded);
            }
            out.push_str(" \x1b[90m│\x1b[0m ");
        }
        out.push('\n');

        // Separator after header
        if is_header && rows.len() > 1 {
            out.push_str("\x1b[90m├");
            for (i, w) in widths.iter().enumerate() {
                out.push_str(&"─".repeat(w + 2));
                if i < widths.len() - 1 {
                    out.push('┼');
                }
            }
            out.push_str("┤\x1b[0m\n");
        }
    }

    // Bottom border
    out.push_str("\x1b[90m└");
    for (i, w) in widths.iter().enumerate() {
        out.push_str(&"─".repeat(w + 2));
        if i < widths.len() - 1 {
            out.push('┴');
        }
    }
    out.push_str("┘\x1b[0m\n");

    out
}

/// Post-process rendered output to catch raw `[text](url)` Markdown link patterns
/// that pulldown-cmark didn't parse as Link events (e.g., multi-line links from HTML conversion).
pub(crate) fn fix_raw_links(
    text: &str,
    number_links: bool,
    counter: &mut usize,
    links: &mut Vec<String>,
) -> String {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '[' {
            // Skip images: ![alt](url)
            let is_image = i > 0 && chars[i - 1] == '!';
            if !is_image {
                let text_start = i + 1;
                let mut j = text_start;
                let mut bracket_depth = 1;

                while j < chars.len() && bracket_depth > 0 {
                    match chars[j] {
                        '[' => bracket_depth += 1,
                        ']' => bracket_depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }

                if bracket_depth == 0 {
                    let link_text_end = j - 1;
                    if j < chars.len() && chars[j] == '(' {
                        let url_start = j + 1;
                        let mut k = url_start;
                        while k < chars.len() && chars[k] != ')' {
                            k += 1;
                        }
                        if k < chars.len() && chars[k] == ')' {
                            let link_text: String = chars[text_start..link_text_end].iter().collect();
                            let trimmed = link_text.trim();
                            let url: String = chars[url_start..k].iter().collect();
                            let url_trimmed = url.trim();
                            let looks_like_url = url_trimmed.contains('.')
                                || url_trimmed.contains("://")
                                || url_trimmed.starts_with('/');
                            if looks_like_url
                                && (trimmed.is_empty()
                                    || !trimmed.chars().all(|c| c.is_ascii_digit()))
                            {
                                let display = if trimmed.is_empty() {
                                    fallback_link_label(url_trimmed)
                                } else {
                                    trimmed.to_string()
                                };
                                if number_links {
                                    *counter += 1;
                                    links.push(url_trimmed.to_string());
                                    result.push_str(&format!("\x1b[33m[{}]\x1b[0m ", *counter));
                                }
                                result.push_str("\x1b[4;36m");
                                result.push_str(&display);
                                result.push_str("\x1b[0m");
                                i = k + 1;
                                continue;
                            }
                        }
                    }
                }
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}
