//! Main-content selection: Trafilatura-style candidate scoring (semantic tags,
//! block readability, paragraph clustering) and boilerplate paragraph removal.
use super::structured::extract_structured_content;
use super::*;

impl PageToMarkdown {
    /// Extract the main content area from HTML.
    /// Trafilatura-style fallback chain: collect candidates from semantic tags, block
    /// readability, and paragraph clustering; pick the highest-scoring result, then
    /// strip link-heavy boilerplate paragraphs (jusText-style).
    pub(super) fn extract_main_content(html: &str, favor_precision: bool, favor_recall: bool) -> String {
        let threshold: i64 = if favor_precision {
            200
        } else if favor_recall {
            40
        } else {
            100
        };
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
        if let Some((content, score)) = best
            && score > threshold {
                if favor_recall {
                    return content;
                }
                let min_para = if favor_precision { 50 } else { 30 };
                return Self::strip_boilerplate_paragraphs(&content, min_para);
            }

        if let Some(structured) = extract_structured_content(html) {
            return structured;
        }

        html.to_string()
    }

    /// Score HTML fragments for main-content selection (text density minus link density).
    pub(super) fn content_quality_score(html: &str) -> i64 {
        let text_len = Self::count_text_chars(html) as i64;
        let link_text_len = Self::count_link_text_chars(html) as i64;
        text_len - link_text_len
    }

    /// Collect inner HTML from semantic main-content tags.
    pub(super) fn semantic_content_candidates(html: &str) -> Vec<(String, i64)> {
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

    pub(super) fn extract_tag_inner(html: &str, tag: &str, close_tag: &str) -> Option<String> {
        let start = find_ci(html, &format!("<{tag}"))?;
        let gt = html[start..].find('>')?;
        let content_start = start + gt + 1;
        let end = find_ci(&html[content_start..], close_tag)?;
        Some(html[content_start..content_start + end].to_string())
    }

    /// Returns the highest-scoring contiguous paragraph window, if above threshold.
    pub(super) fn paragraph_cluster_html(html: &str) -> Option<String> {
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
    pub(super) fn strip_boilerplate_paragraphs(html: &str, min_text_len: i64) -> String {
        let paragraphs = Self::find_paragraph_blocks(html);
        if paragraphs.is_empty() {
            return html.to_string();
        }

        let mut remove = vec![false; paragraphs.len()];
        for (i, (inner, _, _)) in paragraphs.iter().enumerate() {
            let text_len = Self::count_text_chars(inner) as i64;
            let link_len = Self::count_link_text_chars(inner) as i64;
            if text_len < min_text_len || (text_len > 0 && link_len * 2 >= text_len) {
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
    pub(super) fn find_paragraph_blocks(html: &str) -> Vec<(String, usize, usize)> {
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
    pub(super) fn find_top_level_blocks(html: &str) -> Vec<(String, &'static str)> {
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
    pub(super) fn find_matching_close(html: &str, start: usize, open: &str, close: &str) -> Option<usize> {
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
    pub(super) fn count_text_chars(html: &str) -> usize {
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
    pub(super) fn count_link_text_chars(html: &str) -> usize {
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
    pub(super) fn deduplicate_blocks(md: &str) -> String {
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
    pub(super) fn clean(md: &str) -> String {
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
