//! Web search via DuckDuckGo HTML endpoint — no API key required.
//!
//! Mirrors Firecrawl's `/search` endpoint but is fully free and local.
//! Scrapes `https://html.duckduckgo.com/html/?q=<query>` and parses the
//! result links, titles, and snippets from the HTML.

use serde::Serialize;

use crate::html_meta::extract_attr;
use crate::html_util::{decode_html_entities, find_ci, strip_html_tags};

/// A single search result.
#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Parse DuckDuckGo HTML search results from the given HTML body.
/// DDG wraps results in `<a class="result__a" href="...">Title</a>`
/// with snippets in `<a class="result__snippet">...</a>`.
pub fn parse_ddg_results(html: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut pos = 0;

    while pos < html.len() {
        // Find each result block by looking for `result__a` class links
        let Some(rel) = find_ci(&html[pos..], "result__a") else {
            break;
        };
        let link_start = pos + rel;

        // Find the opening <a tag for this result
        let tag_start = html[..link_start].rfind('<').unwrap_or(link_start);
        let Some(tag_end) = html[tag_start..].find('>') else {
            break;
        };
        let tag = &html[tag_start..=tag_start + tag_end];
        let tag_end_abs = tag_start + tag_end + 1;

        // Extract href — DDG uses redirect links like //duckduckgo.com/l/?uddg=<encoded_url>
        let raw_url = extract_attr(tag, "href").unwrap_or_default();
        let url = decode_ddg_redirect(&raw_url);

        // Extract title text between <a ...> and </a>
        let title = extract_text_until_close(&html[tag_end_abs..]);

        // Find the snippet — look for `result__snippet` after this result link
        let snippet = if let Some(snip_rel) = find_ci(&html[tag_end_abs..], "result__snippet") {
            let snip_pos = tag_end_abs + snip_rel;
            let snip_tag_start = html[..snip_pos].rfind('<').unwrap_or(snip_pos);
            if let Some(snip_tag_end) = html[snip_tag_start..].find('>') {
                let after = snip_tag_start + snip_tag_end + 1;
                extract_text_until_close(&html[after..])
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        if !url.is_empty() {
            results.push(SearchResult {
                title: title.trim().to_string(),
                url,
                snippet: snippet.trim().to_string(),
            });
        }

        pos = tag_end_abs;
    }

    results
}

/// Extract text content from the start of HTML until the first `</a>`.
fn extract_text_until_close(rest: &str) -> String {
    let Some(close) = find_ci(rest, "</a>") else {
        return String::new();
    };
    let inner = &rest[..close];
    let text = strip_html_tags(inner);
    decode_html_entities(&text).trim().to_string()
}

/// Decode a DuckDuckGo redirect URL to extract the actual target URL.
/// DDG links look like: `//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&rut=...`
/// or sometimes: `https://duckduckgo.com/l/?uddg=...`
/// If it's already a direct URL, return it as-is.
fn decode_ddg_redirect(href: &str) -> String {
    let href = href.trim();

    // Direct HTTP(S) URL — return as-is
    if (href.starts_with("http://") || href.starts_with("https://"))
        && !href.contains("duckduckgo.com/l/") {
            return href.to_string();
        }

    // Protocol-relative DDG redirect
    let href = if href.starts_with("//") {
        format!("https:{}", href)
    } else {
        href.to_string()
    };

    // Extract the `uddg` parameter which contains the actual URL
    if let Some(uddg_pos) = href.find("uddg=") {
        let after = &href[uddg_pos + 5..];
        let end = after.find('&').unwrap_or(after.len());
        let encoded = &after[..end];
        if let Ok(decoded) = urlencoding::decode(encoded) {
            return decoded.into_owned();
        }
    }

    // Fallback: return the href as-is
    href
}

/// Build the DuckDuckGo HTML search URL for a query.
pub fn ddg_search_url(query: &str, limit: Option<usize>) -> String {
    let encoded = urlencoding::encode(query);
    let count = limit.unwrap_or(10);
    format!(
        "https://html.duckduckgo.com/html/?q={}&kp=1&kl=us-en&s=0&df=&v_q={}",
        encoded, count
    )
}

/// Render search results as Markdown.
pub fn results_to_markdown(results: &[SearchResult]) -> String {
    let mut out = String::new();
    for (i, r) in results.iter().enumerate() {
        out.push_str(&format!("{}. [{}]({})\n", i + 1, r.title, r.url));
        if !r.snippet.is_empty() {
            out.push_str(&format!("   > {}\n", r.snippet));
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ddg_results_extracts_title_url_snippet() {
        let html = r#"<html><body>
        <div class="result">
            <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&rut=abc">Example Site</a>
            <a class="result__snippet">This is an example website.</a>
        </div>
        </body></html>"#;
        let results = parse_ddg_results(html);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Example Site");
        assert_eq!(results[0].url, "https://example.com");
        assert_eq!(results[0].snippet, "This is an example website.");
    }

    #[test]
    fn parse_ddg_results_extracts_multiple() {
        let html = r#"<html><body>
        <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fa.com">Site A</a>
        <a class="result__snippet">Snippet A</a>
        <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fb.com">Site B</a>
        <a class="result__snippet">Snippet B</a>
        </body></html>"#;
        let results = parse_ddg_results(html);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].url, "https://a.com");
        assert_eq!(results[1].url, "https://b.com");
    }

    #[test]
    fn parse_ddg_results_skips_empty_urls() {
        let html = r#"<a class="result__a" href="">No URL</a>"#;
        let results = parse_ddg_results(html);
        assert!(results.is_empty());
    }

    #[test]
    fn decode_ddg_redirect_decodes_encoded_url() {
        let url = decode_ddg_redirect("//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust-lang.org&rut=xyz");
        assert_eq!(url, "https://www.rust-lang.org");
    }

    #[test]
    fn decode_ddg_redirect_passes_through_direct_url() {
        let url = decode_ddg_redirect("https://example.com/page");
        assert_eq!(url, "https://example.com/page");
    }

    #[test]
    fn ddg_search_url_encodes_query() {
        let url = ddg_search_url("rust programming", Some(10));
        assert!(url.contains("q=rust%20programming"));
        assert!(url.starts_with("https://html.duckduckgo.com/html/"));
    }

    #[test]
    fn results_to_markdown_formats_correctly() {
        let results = vec![
            SearchResult {
                title: "Rust".to_string(),
                url: "https://rust-lang.org".to_string(),
                snippet: "A language for systems".to_string(),
            },
            SearchResult {
                title: "Crates".to_string(),
                url: "https://crates.io".to_string(),
                snippet: String::new(),
            },
        ];
        let md = results_to_markdown(&results);
        assert!(md.contains("1. [Rust](https://rust-lang.org)"));
        assert!(md.contains("> A language for systems"));
        assert!(md.contains("2. [Crates](https://crates.io)"));
    }

    #[test]
    fn parse_ddg_results_handles_html_entities_in_title() {
        let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com">Tom &amp; Jerry</a>"#;
        let results = parse_ddg_results(html);
        assert_eq!(results[0].title, "Tom & Jerry");
    }
}
