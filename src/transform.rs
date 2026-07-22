//! Output transformations applied **after** HTML → Markdown conversion.
//!
//! These mirror Firecrawl's `query`/`summary` formats and Context7's
//! token-aware doc shaping, but stay entirely local and LLM-free:
//!
//! - [`extract_topic`] — query-focused paragraph filtering (≈ Firecrawl `highlights`)
//! - [`extract_summary`] — extractive summarization by TF score (≈ Firecrawl `summary`)
//! - [`truncate_by_tokens`] — soft cap by token budget, not character count
//! - [`split_paragraphs`] — shared splitter used by topic and summary
//!
//! All public functions are pure and operate on `&str`, making them trivial
//! to unit test.

/// Split Markdown into paragraph-sized blocks on blank lines.
/// Tracks fenced code blocks so we never break inside ` ``` … ``` `.
pub fn split_paragraphs(md: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut fenced = 0usize;
    for line in md.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(line);
            fenced += 1;
            if fenced.is_multiple_of(2) {
                out.push(std::mem::take(&mut buf));
            }
            continue;
        }
        if fenced % 2 == 1 {
            buf.push('\n');
            buf.push_str(line);
            continue;
        }
        if line.trim().is_empty() {
            if !buf.trim().is_empty() {
                out.push(std::mem::take(&mut buf));
            }
            continue;
        }
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(line);
    }
    if !buf.trim().is_empty() {
        out.push(buf);
    }
    out
}

/// Strip Markdown formatting markers (headers, list markers, links) so we can
/// score plain prose without `**bold**` or `[text](url)` confusing the matches.
fn strip_markdown(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_code = false;
    for line in s.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            out.push(' ');
            continue;
        }
        if in_code {
            out.push(' ');
            continue;
        }
        if trimmed.starts_with("#") {
            out.push_str(trimmed.trim_start_matches('#').trim());
            out.push(' ');
            continue;
        }
        let stripped = trimmed
            .trim_start_matches(['-', '*', '+'])
            .trim_start();
        let stripped = strip_md_links(stripped);
        let stripped = stripped.replace(['*', '_'], "");
        out.push_str(&stripped);
        out.push(' ');
    }
    out
}

/// Replace `[text](url)` with `text` and `![alt](url)` with `alt`.
fn strip_md_links(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'['
            && let Some(close) = s[i + 1..].find(']') {
                let label = &s[i + 1..i + 1 + close];
                let after = &s[i + 1 + close..];
                if after.starts_with("](")
                    && let Some(end) = after[2..].find(')') {
                        out.push_str(label);
                        i += 1 + close + 2 + end + 1;
                        continue;
                    }
            }
        out.push(s.as_bytes()[i] as char);
        i += 1;
    }
    out
}

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "have", "in", "is",
    "it", "its", "of", "on", "or", "that", "the", "this", "to", "was", "were", "will", "with",
    "but", "if", "not", "you", "your", "we", "our", "they", "their", "them", "i", "me", "he",
    "she", "his", "her", "about", "into", "than", "then", "so", "do", "does", "did", "can",
    "could", "should", "would", "may", "might", "must", "shall",
];

fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            cur.push(ch.to_ascii_lowercase());
        } else if !cur.is_empty() {
            if cur.len() > 1 && !STOPWORDS.contains(&cur.as_str()) {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if cur.len() > 1 && !STOPWORDS.contains(&cur.as_str()) {
        out.push(cur);
    }
    out
}

/// Score a paragraph against a query.
/// +1 per unique query token that appears in the paragraph;
/// +0.5 per repeat; +title bonus applied separately by callers.
fn topic_score(paragraph_text: &str, query_tokens: &[String]) -> f64 {
    if query_tokens.is_empty() || paragraph_text.is_empty() {
        return 0.0;
    }
    let tokens = tokenize(paragraph_text);
    if tokens.is_empty() {
        return 0.0;
    }
    let mut hits = 0.0f64;
    let mut para_counts: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for t in &tokens {
        *para_counts.entry(t.as_str()).or_insert(0) += 1;
    }
    for qt in query_tokens {
        let c = para_counts.get(qt.as_str()).copied().unwrap_or(0) as f64;
        if c >= 1.0 {
            hits += 1.0 + 0.5 * (c - 1.0).max(0.0);
        }
    }
    hits / (query_tokens.len() as f64).sqrt()
}

/// Query-focused extraction: keep only paragraphs that score above zero,
/// preserving original order. If `max_paragraphs` is given, take the top
/// scorers. Returns Markdown-shaped output.
pub fn extract_topic(md: &str, query: &str, max_paragraphs: Option<usize>) -> Option<String> {
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return None;
    }
    let paragraphs = split_paragraphs(md);
    let mut scored: Vec<(usize, f64, String)> = Vec::new();
    for (idx, p) in paragraphs.iter().enumerate() {
        let plain = strip_markdown(p);
        let s = topic_score(&plain, &query_tokens);
        if s > 0.0 {
            scored.push((idx, s, p.clone()));
        }
    }
    if scored.is_empty() {
        return None;
    }
    if let Some(max) = max_paragraphs {
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(max);
        scored.sort_by_key(|t| t.0);
    }
    Some(scored.into_iter().map(|t| t.2).collect::<Vec<_>>().join("\n\n"))
}

/// Extractive summarization: TF-based sentence scoring.
pub fn extract_summary(md: &str, max_sentences: usize, title_hint: Option<&str>) -> Option<String> {
    if max_sentences == 0 {
        return None;
    }
    let sentences = split_sentences(md);
    if sentences.is_empty() {
        return None;
    }
    let n = sentences.len();
    if n <= max_sentences {
        return Some(md.to_string());
    }

    let mut doc_tf: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut sent_tf: Vec<std::collections::HashMap<String, usize>> = Vec::with_capacity(n);
    for s in &sentences {
        let tokens = tokenize(s);
        let mut tf = std::collections::HashMap::new();
        for t in tokens {
            *doc_tf.entry(t.clone()).or_insert(0) += 1;
            *tf.entry(t).or_insert(0) += 1;
        }
        sent_tf.push(tf);
    }

    let title_tokens: std::collections::HashSet<String> = title_hint
        .map(tokenize)
        .unwrap_or_default()
        .into_iter()
        .collect();

    let mut scored: Vec<(usize, f64)> = Vec::with_capacity(n);
    for (idx, tf) in sent_tf.iter().enumerate() {
        if tf.is_empty() || sentences[idx].split_whitespace().count() < 4 {
            scored.push((idx, 0.0));
            continue;
        }
        let mut score = 0.0f64;
        for (term, count) in tf {
            let df = *doc_tf.get(term).unwrap_or(&1) as f64;
            let idf = (1.0 + (n as f64) / df).ln_1p();
            let mut contribution = (*count as f64) * idf;
            if title_tokens.contains(term) {
                contribution *= 1.8;
            }
            score += contribution;
        }
        let pos_bonus: f64 = if idx < n / 4 { 1.4 } else if idx < n / 2 { 1.1 } else { 1.0 };
        score *= pos_bonus;
        scored.push((idx, score));
    }

    let mut indexed: Vec<(usize, f64)> = scored;
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut keep: std::collections::HashSet<usize> = indexed
        .into_iter()
        .take(max_sentences)
        .map(|(i, _)| i)
        .collect();
    let mut ordered: Vec<&str> = Vec::new();
    for (idx, sentence) in sentences.iter().enumerate() {
        if keep.remove(&idx) {
            ordered.push(*sentence);
        }
    }
    if ordered.is_empty() {
        None
    } else {
        Some(ordered.join(" "))
    }
}

fn split_sentences(md: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut last = 0usize;
    let mut in_code = false;
    let bytes = md.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let line_end = md[i..].find('\n').map(|e| i + e).unwrap_or(md.len());
        let line = &md[i..line_end];
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            if !in_code {
                out.push(&md[last..line_end]);
                last = line_end + 1;
            } else {
                last = line_end + 1;
            }
            i = line_end + 1;
            continue;
        }
        if in_code {
            i = line_end + 1;
            continue;
        }
        for (j, ch) in line.char_indices() {
            if matches!(ch, '.' | '!' | '?') {
                let next = line[j + ch.len_utf8()..].chars().next();
                if next.map(|c| c.is_whitespace()).unwrap_or(false) && last < i + j {
                    let candidate = &md[last..i + j + ch.len_utf8()];
                    let trimmed_c = candidate.trim();
                    if !trimmed_c.is_empty() {
                        out.push(trimmed_c);
                    }
                    last = i + j + ch.len_utf8();
                }
            }
        }
        i = line_end + 1;
    }
    let tail = md[last..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

const AVG_CHARS_PER_TOKEN: f64 = 4.0;

/// Truncate Markdown to roughly `max_tokens` tokens (1 token ≈ 4 chars).
/// Prefers cutting at paragraph boundaries; falls back to a character cut
/// with a `[truncated]` marker.
pub fn truncate_by_tokens(md: &str, max_tokens: usize) -> String {
    let target_chars = (max_tokens as f64 * AVG_CHARS_PER_TOKEN).ceil() as usize;
    if md.len() <= target_chars {
        return md.to_string();
    }
    let paragraphs = split_paragraphs(md);
    if paragraphs.is_empty() {
        return truncate_with_marker(md, target_chars);
    }
    let mut acc = String::new();
    for p in &paragraphs {
        let candidate = if acc.is_empty() {
            p.clone()
        } else {
            format!("{}\n\n{}", acc, p)
        };
        if candidate.len() > target_chars {
            break;
        }
        acc = candidate;
    }
    if acc.is_empty() {
        return truncate_with_marker(md, target_chars);
    }
    if acc.len() < md.len() {
        format!("{}\n\n[truncated]", acc)
    } else {
        acc
    }
}

/// Reuse the existing `truncate_with_marker` style but keep the last line on a word boundary.
fn truncate_with_marker(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    while cut > 0 && !s[..cut].ends_with(char::is_whitespace) {
        cut -= 1;
    }
    if cut == 0 {
        cut = max;
        while cut > 0 && !s.is_char_boundary(cut) {
            cut -= 1;
        }
    }
    format!("{}\n\n[truncated]", s[..cut].trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_paragraphs_preserves_code_blocks() {
        let md = "First para.\n\n```rust\nlet x = 1;\nlet y = 2;\n```\n\nLast para.";
        let parts = split_paragraphs(md);
        assert_eq!(parts.len(), 3);
        assert!(parts[1].contains("let x = 1;"));
    }

    #[test]
    fn topic_filter_keeps_relevant_paragraphs() {
        let md = "\
Rust is a systems programming language.\n\n\
Python is often used for scripting.\n\n\
Cargo is the Rust package manager and build system.\n\n\
JavaScript runs in the browser.\n";
        let out = extract_topic(md, "rust cargo", None).expect("topic result");
        assert!(out.contains("Rust is"));
        assert!(out.contains("Cargo is"));
        assert!(!out.contains("Python"));
        assert!(!out.contains("JavaScript"));
    }

    #[test]
    fn topic_filter_returns_none_for_empty_query() {
        let md = "Some content here.";
        assert!(extract_topic(md, "", None).is_none());
    }

    #[test]
    fn topic_filter_caps_max_paragraphs() {
        let md = "Rust 1.\n\nRust 2.\n\nRust 3.\n\nRust 4.\n\nOther text.";
        let out = extract_topic(md, "Rust", Some(2)).expect("topic result");
        assert!(out.contains("Rust 1."));
        assert!(out.contains("Rust 2."));
        assert!(!out.contains("Rust 3."));
    }

    #[test]
    fn summary_prefers_relevant_openers() {
        let md = "Bananas are yellow and rich in potassium. \
                  They grow in tropical climates. \
                  Apples are red and crunchy. \
                  Grapes grow on vines in temperate zones. \
                  Oranges provide vitamin C and folate.";
        let s = extract_summary(md, 2, Some("bananas potassium")).expect("summary");
        assert!(s.contains("Bananas"));
    }

    #[test]
    fn summary_empty_on_empty_input() {
        assert!(extract_summary("", 3, None).is_none());
        assert!(extract_summary("Some text.", 0, None).is_none());
    }

    #[test]
    fn truncate_by_tokens_cuts_at_paragraph_when_possible() {
        let md = "Short para one.\n\nShort para two is a bit longer than the first.\n\nLast paragraph.";
        let out = truncate_by_tokens(md, 8);
        assert!(out.contains("[truncated]"));
        assert!(out.contains("Short para one."));
    }

    #[test]
    fn truncate_by_tokens_returns_full_when_fits() {
        let md = "Tiny.";
        assert_eq!(truncate_by_tokens(md, 100), "Tiny.");
    }

    #[test]
    fn sentence_splitter_handles_abbreviations_naively() {
        let s = split_sentences("Sentence one. Sentence two. Sentence three.");
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn tokenizer_drops_stopwords() {
        let tokens = tokenize("The rust runtime and the cargo build tool");
        assert!(tokens.contains(&"rust".to_string()));
        assert!(tokens.contains(&"cargo".to_string()));
        assert!(!tokens.contains(&"the".to_string()));
    }

    #[test]
    fn strip_markdown_removes_links() {
        let md = "See [the docs](https://example.com) for details.";
        let plain = strip_markdown(md);
        assert!(plain.contains("the docs"));
        assert!(!plain.contains("https://"));
    }
}
