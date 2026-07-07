use std::collections::HashSet;

use url::Url;

use crate::is_blacklisted;
use crate::markdown::find_ci;

/// Extract absolute HTTP(S) links from `<a href="...">` tags in HTML.
pub fn extract_page_links(html: &str, base_url: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut seen = HashSet::new();
    let mut pos = 0;

    while pos < html.len() {
        if let Some(start) = find_ci(&html[pos..], "<a") {
            let start = pos + start;
            if let Some(end) = html[start..].find('>') {
                let tag = &html[start..=start + end];
                if let Some(href) = extract_href(tag) {
                    if let Some(resolved) = normalize_crawl_url(&href, base_url) {
                        if seen.insert(resolved.clone()) {
                            links.push(resolved);
                        }
                    }
                }
                pos = start + end + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    links
}

/// Returns crawl targets on the same origin as `root`, excluding blacklisted URLs.
pub fn same_origin_links(html: &str, page_url: &str, root: &Url) -> Vec<String> {
    extract_page_links(html, page_url)
        .into_iter()
        .filter(|url| is_same_origin(url, root) && !is_blacklisted(url))
        .collect()
}

/// True when `url` shares scheme and host with `root`.
pub fn is_same_origin(url: &str, root: &Url) -> bool {
    let parsed = match Url::parse(url) {
        Ok(u) => u,
        Err(_) => return false,
    };
    parsed.scheme() == root.scheme()
        && parsed.host_str().is_some()
        && parsed.host_str() == root.host_str()
}

/// Resolve and normalize a link for crawl deduplication (drops fragment).
pub fn normalize_crawl_url(href: &str, base_url: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty()
        || href.starts_with('#')
        || href.starts_with("mailto:")
        || href.starts_with("tel:")
        || href.starts_with("javascript:")
    {
        return None;
    }

    let resolved = if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else if href.starts_with("//") {
        let base = Url::parse(base_url).ok()?;
        format!("{}:{}", base.scheme(), href)
    } else {
        let base = Url::parse(base_url).ok()?;
        base.join(href).ok()?.to_string()
    };

    let mut parsed = Url::parse(&resolved).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }
    parsed.set_fragment(None);
    Some(parsed.to_string())
}

fn extract_href(tag: &str) -> Option<String> {
    let needle = "href=";
    let pos = find_ci(tag, needle)?;
    let after = &tag[pos + needle.len()..];
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
    Some(after[val_start..val_end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_page_links_resolves_relative_hrefs() {
        let html = r#"<a href="/about">About</a><a href="contact">Contact</a>"#;
        let links = extract_page_links(html, "https://example.com/blog/post");
        assert_eq!(links.len(), 2);
        assert!(links.contains(&"https://example.com/about".to_string()));
        assert!(links.contains(&"https://example.com/blog/contact".to_string()));
    }

    #[test]
    fn extract_page_links_skips_mailto_and_fragments() {
        let html = "<a href=\"#top\">Top</a><a href=\"mailto:a@b.com\">Email</a><a href=\"/ok\">OK</a>";
        let links = extract_page_links(html, "https://example.com/");
        assert_eq!(links, vec!["https://example.com/ok"]);
    }

    #[test]
    fn extract_page_links_deduplicates() {
        let html = r#"<a href="/a">A</a><a href="/a">A again</a>"#;
        let links = extract_page_links(html, "https://example.com/");
        assert_eq!(links, vec!["https://example.com/a"]);
    }

    #[test]
    fn same_origin_links_filters_external_and_blacklisted() {
        let root = Url::parse("https://example.com/").unwrap();
        let html = r#"
            <a href="/local">Local</a>
            <a href="https://other.com/page">External</a>
            <a href="/pixel.gif">Pixel</a>
        "#;
        let links = same_origin_links(html, "https://example.com/", &root);
        assert_eq!(links, vec!["https://example.com/local"]);
    }

    #[test]
    fn is_same_origin_matches_host_and_scheme() {
        let root = Url::parse("https://example.com/start").unwrap();
        assert!(is_same_origin("https://example.com/other", &root));
        assert!(!is_same_origin("http://example.com/other", &root));
        assert!(!is_same_origin("https://other.com/", &root));
    }

    #[test]
    fn normalize_crawl_url_strips_fragment() {
        let url = normalize_crawl_url("/page#section", "https://example.com/").unwrap();
        assert_eq!(url, "https://example.com/page");
    }
}
