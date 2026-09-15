//! Forum/thread comment extraction heuristics.
use super::*;

impl PageToMarkdown {
    /// Detect if a page looks like a forum/thread with comments.
    /// Returns true if multiple comment-like containers are found.
    pub(super) fn looks_like_forum_page(html: &str) -> bool {
        let comment_indicators = [
            "class=\"comment",
            "class='comment",
            "class=\"post-body",
            "class='post-body",
            "class=\"comment-body",
            "class='comment-body",
            "class=\"comment-content",
            "class='comment-content",
            "class=\"message-body",
            "class='message-body",
            "id=\"comment-",
            "id='comment-",
            "data-comment-id",
            "data-testid=\"comment",
        ];
        let mut count = 0;
        for indicator in &comment_indicators {
            count += find_ci(html, indicator).map(|_| 1).unwrap_or(0);
            if count >= 2 {
                return true;
            }
        }
        false
    }

    /// Extract comments from forum/thread pages.
    /// Detects comment containers, extracts author and text, and formats as Markdown.
    /// Returns None if no comments are found or the page doesn't look like a forum.
    pub(super) fn extract_comments(html: &str) -> Option<String> {
        if !Self::looks_like_forum_page(html) {
            return None;
        }

        let comments = Self::find_comment_blocks(html);
        if comments.len() < 2 {
            return None;
        }

        let mut out = String::new();
        out.push_str("## Comments\n\n");
        for (author, text, depth) in &comments {
            let indent = "  ".repeat(*depth);
            if let Some(a) = author {
                out.push_str(&format!("{}**{}**:\n\n", indent, a));
            }
            let comment_md = crate::html_to_md::parse_html(text);
            let comment_md = Self::clean(&comment_md);
            for line in comment_md.lines() {
                out.push_str(&format!("{}> {}\n", indent, line));
            }
            out.push('\n');
        }
        Some(out.trim().to_string())
    }

    /// Find comment blocks in HTML.
    /// Returns a list of (author, inner_html, nesting_depth) tuples.
    pub(super) fn find_comment_blocks(html: &str) -> Vec<(Option<String>, String, usize)> {
        let mut blocks = Vec::new();
        let comment_patterns = [
            "class=\"comment",
            "class='comment",
            "id=\"comment-",
            "id='comment-",
            "data-testid=\"comment",
        ];

        let mut i = 0;
        while i < html.len() {
            let mut found = false;
            for pattern in &comment_patterns {
                if let Some(pos) = find_ci(&html[i..], pattern) {
                    let pos = i + pos;
                    // Find the enclosing tag start (walk back to '<')
                    let tag_start = html[..pos].rfind('<').unwrap_or(pos);
                    if let Some(gt) = html[tag_start..].find('>') {
                        let tag = &html[tag_start..=tag_start + gt];
                        let tag_name = Self::extract_tag_name(tag);
                        let close_tag = format!("</{}>", tag_name);
                        let content_start = tag_start + gt + 1;
                        if let Some(end) = Self::find_matching_close(
                            html,
                            content_start,
                            &format!("<{}", tag_name),
                            &close_tag,
                        ) {
                            let inner = &html[content_start..end];
                            // Check the container tag for data-author first, then inner HTML
                            let author = Self::extract_comment_author(tag)
                                .or_else(|| Self::extract_comment_author(inner));
                            let depth = Self::estimate_nesting_depth(html, tag_start);
                            let text = Self::extract_comment_text(inner);
                            if !text.is_empty() {
                                blocks.push((author, text, depth));
                            }
                            i = end + close_tag.len();
                            found = true;
                            break;
                        }
                    }
                }
            }
            if !found {
                break;
            }
        }
        blocks
    }

    /// Extract the tag name from an opening tag string like `<div class="...">`.
    pub(super) fn extract_tag_name(tag: &str) -> String {
        let after_lt = &tag[1..];
        let end = after_lt
            .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
            .unwrap_or(after_lt.len());
        after_lt[..end].to_string()
    }

    /// Extract author name from comment inner HTML.
    /// Looks for common author patterns: `<a class="author">`, `<span class="author">`,
    /// `data-author="..."`, `<cite>`, or `<address>`.
    pub(super) fn extract_comment_author(html: &str) -> Option<String> {
        // data-author attribute
        if let Some(pos) = find_ci(html, "data-author=") {
            let after = &html[pos + 12..];
            if let Some(val) = Self::extract_quoted_value(after) {
                return Some(val);
            }
        }
        // <a class="author"> or <span class="author">
        for tag in &["<a", "<span", "<div", "<strong", "<b"] {
            let mut i = 0;
            while i < html.len() {
                if let Some(pos) = find_ci(&html[i..], tag) {
                    let pos = i + pos;
                    if let Some(gt) = html[pos..].find('>') {
                        let tag_str = &html[pos..=pos + gt];
                        if find_ci(tag_str, "author").is_some()
                            || find_ci(tag_str, "username").is_some()
                            || find_ci(tag_str, "user-name").is_some()
                        {
                            let content_start = pos + gt + 1;
                            let close = format!("</{}", &tag[1..]);
                            if let Some(end) = find_ci(&html[content_start..], &close) {
                                let author_html = &html[content_start..content_start + end];
                                let author = strip_html_tags(author_html);
                                let author = author.trim().to_string();
                                if !author.is_empty() {
                                    return Some(author);
                                }
                            }
                        }
                        i = pos + gt + 1;
                        continue;
                    }
                }
                break;
            }
        }
        // <cite> or <address>
        for tag in &["<cite", "<address"] {
            if let Some(pos) = find_ci(html, tag)
                && let Some(gt) = html[pos..].find('>') {
                    let content_start = pos + gt + 1;
                    let close = format!("</{}", &tag[1..]);
                    if let Some(end) = find_ci(&html[content_start..], &close) {
                        let author = strip_html_tags(&html[content_start..content_start + end]);
                        let author = author.trim().to_string();
                        if !author.is_empty() {
                            return Some(author);
                        }
                    }
                }
        }
        None
    }

    /// Extract the main text content from a comment block, excluding metadata.
    /// Looks for content containers first, then falls back to the full inner HTML.
    pub(super) fn extract_comment_text(html: &str) -> String {
        let content_patterns = [
            "class=\"comment-body",
            "class='comment-body",
            "class=\"comment-content",
            "class='comment-content",
            "class=\"post-body",
            "class='post-body",
            "class=\"message-body",
            "class='message-body",
            "class=\"usertext-body",
            "class='usertext-body",
            "class=\"md",
            "class='md",
        ];
        for pattern in &content_patterns {
            if let Some(pos) = find_ci(html, pattern) {
                let tag_start = html[..pos].rfind('<').unwrap_or(pos);
                if let Some(gt) = html[tag_start..].find('>') {
                    let tag = &html[tag_start..=tag_start + gt];
                    let tag_name = Self::extract_tag_name(tag);
                    let close_tag = format!("</{}>", tag_name);
                    let content_start = tag_start + gt + 1;
                    if let Some(end) =
                        Self::find_matching_close(html, content_start, &format!("<{}", tag_name), &close_tag)
                    {
                        return html[content_start..end].to_string();
                    }
                }
            }
        }
        // Fallback: return the full inner HTML
        html.to_string()
    }

    /// Estimate nesting depth by counting ancestor comment containers.
    pub(super) fn estimate_nesting_depth(html: &str, pos: usize) -> usize {
        let before = &html[..pos];
        let mut depth: usize = 0;
        let mut i = 0;
        while i < before.len() {
            let mut found = false;
            for pattern in &["class=\"comment", "class='comment", "id=\"comment-"] {
                if let Some(p) = find_ci(&before[i..], pattern) {
                    let p = i + p;
                    let tag_start = before[..p].rfind('<').unwrap_or(p);
                    if let Some(gt) = before[tag_start..].find('>') {
                        let tag = &before[tag_start..=tag_start + gt];
                        let tag_name = Self::extract_tag_name(tag);
                        let close_tag = format!("</{}>", tag_name);
                        let content_start = tag_start + gt + 1;
                        if let Some(end) = Self::find_matching_close(
                            before,
                            content_start,
                            &format!("<{}", tag_name),
                            &close_tag,
                        ) {
                            if end >= pos {
                                depth += 1;
                            }
                            i = end + close_tag.len();
                            found = true;
                            break;
                        }
                    }
                }
            }
            if !found {
                break;
            }
        }
        depth.saturating_sub(1)
    }

    /// Extract the value from a quoted attribute string like ="value" or ='value'.
    pub(super) fn extract_quoted_value(s: &str) -> Option<String> {
        let mut i = 0;
        while i < s.len() && s.as_bytes()[i].is_ascii_whitespace() {
            i += 1;
        }
        let quote = *s.as_bytes().get(i)? as char;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let val_start = i + 1;
        let val_end = s[val_start..].find(quote)? + val_start;
        Some(s[val_start..val_end].to_string())
    }
}
