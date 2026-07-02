use anyhow::{Context, Result};
use reqwest::{Client, ClientBuilder};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use url::Url;

use crate::{DEFAULT_TIMEOUT, DEFAULT_USER_AGENT};

/// Configuration for the headless browser
#[derive(Debug, Clone)]
pub struct BrowserOptions {
    /// Request timeout
    pub timeout: Duration,
    /// User-Agent string
    pub user_agent: String,
    /// Follow redirects
    pub follow_redirects: bool,
    /// Enable JavaScript execution (reserved for future expansion)
    pub enable_javascript: bool,
    /// Initial cookies to send with every request (format: "name=value")
    pub cookies: Vec<String>,
    /// Custom HTTP headers to send with every request (format: "Name: Value")
    pub headers: Vec<String>,
    /// Minimum delay between consecutive requests to the same host
    pub request_delay: Duration,
    /// Cache TTL for fetched pages (zero = caching disabled)
    pub cache_ttl: Duration,
}

impl Default for BrowserOptions {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            user_agent: DEFAULT_USER_AGENT.to_string(),
            follow_redirects: true,
            enable_javascript: false,
            cookies: Vec::new(),
            headers: Vec::new(),
            request_delay: Duration::from_millis(0),
            cache_ttl: Duration::from_secs(0),
        }
    }
}

/// Minimal headless browser: fetches raw HTML only.
/// No rendering engine—intentionally lightweight for MCP token efficiency.
pub struct Browser {
    client: Client,
    options: BrowserOptions,
    last_request: Mutex<Option<Instant>>,
    cache: Mutex<HashMap<String, (String, Instant)>>,
}

impl Browser {
    /// Build a new Browser with the given options
    pub fn new(options: BrowserOptions) -> Result<Self> {
        let client = ClientBuilder::new()
            .timeout(options.timeout)
            .user_agent(&options.user_agent)
            .redirect(reqwest::redirect::Policy::default())
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            client,
            options,
            last_request: Mutex::new(None),
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Enforce the configured polite delay between requests.
    async fn enforce_delay(&self) {
        let delay = self.options.request_delay;
        if delay.is_zero() {
            return;
        }
        let mut guard = self.last_request.lock().unwrap();
        if let Some(last) = *guard {
            let elapsed = last.elapsed();
            if elapsed < delay {
                let remaining = delay - elapsed;
                drop(guard);
                tokio::time::sleep(remaining).await;
                let mut guard = self.last_request.lock().unwrap();
                *guard = Some(Instant::now());
                return;
            }
        }
        *guard = Some(Instant::now());
    }

    /// Fetch raw HTML from a URL
    pub async fn fetch(&self, url: &str) -> Result<String> {
        // Check cache first
        if !self.options.cache_ttl.is_zero() {
            if let Some(cached) = self.lookup_cache(url) {
                return Ok(cached);
            }
        }

        self.enforce_delay().await;

        let parsed = Url::parse(url).context("Invalid URL")?;

        let mut req = self.client.get(parsed.clone());
        if !self.options.cookies.is_empty() {
            req = req.header(
                reqwest::header::COOKIE,
                self.options.cookies.join("; "),
            );
        }
        for h in &self.options.headers {
            if let Some((name, value)) = h.split_once(':') {
                req = req.header(name.trim(), value.trim());
            }
        }

        let resp = req.send().await.context("HTTP request failed")?;

        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("HTTP error: {}", status);
        }

        let body = resp.text().await.context("Failed to read response body")?;

        // Store in cache if enabled
        if !self.options.cache_ttl.is_zero() {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(url.to_string(), (body.clone(), Instant::now()));
        }

        Ok(body)
    }

    /// Look up a URL in the cache, returning the body if not expired.
    fn lookup_cache(&self, url: &str) -> Option<String> {
        let mut cache = self.cache.lock().unwrap();
        if let Some((body, fetched_at)) = cache.get(url) {
            if fetched_at.elapsed() < self.options.cache_ttl {
                return Some(body.clone());
            }
            cache.remove(url);
        }
        None
    }

    /// Replace `<iframe>` tags with the content fetched from their `src` attribute.
    /// Relative URLs are resolved against `base_url`.
    /// Iframes with `javascript:`, `about:`, or `#` src are stripped.
    pub async fn inline_iframes(&self, html: &str, base_url: &str) -> Result<String> {
        let mut result = String::with_capacity(html.len());
        let mut i = 0;

        while i < html.len() {
            if let Some(start) = crate::markdown::find_ci(&html[i..], "<iframe") {
                let start = i + start;
                result.push_str(&html[i..start]);

                if let Some(tag_end) = find_tag_end(html, start) {
                    let tag = &html[start..=tag_end];
                    let src = extract_src(tag).filter(|s| {
                        !s.is_empty()
                            && !s.starts_with("javascript:")
                            && !s.starts_with("about:")
                            && !s.starts_with("#")
                    });

                    let close_end = crate::markdown::find_ci(&html[tag_end..], "</iframe>")
                        .map(|p| tag_end + p + "</iframe>".len());

                    let replacement = if let Some(url) = src {
                        let resolved = resolve_iframe_src(base_url, &url);
                        match self.fetch(&resolved).await {
                            Ok(content) => content,
                            Err(_) => String::new(),
                        }
                    } else {
                        String::new()
                    };

                    result.push_str(&replacement);

                    if let Some(end) = close_end {
                        i = end;
                    } else {
                        i = tag_end + 1;
                    }
                } else {
                    i = start + 1;
                }
            } else {
                result.push_str(&html[i..]);
                break;
            }
        }

        Ok(result)
    }

    /// Returns a reference to the underlying HTTP client
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Returns a reference to the browser options
    pub fn options(&self) -> &BrowserOptions {
        &self.options
    }
}

/// Find the `>` that closes an HTML tag, respecting quotes.
fn find_tag_end(html: &str, start: usize) -> Option<usize> {
    let mut in_quote = None;
    for (offset, c) in html[start..].char_indices() {
        match c {
            '"' | '\'' => {
                if in_quote == Some(c) {
                    in_quote = None;
                } else if in_quote.is_none() {
                    in_quote = Some(c);
                }
            }
            '>' if in_quote.is_none() => return Some(start + offset),
            _ => {}
        }
    }
    None
}

/// Extract `src="..."` or `src='...'` from an HTML tag string.
fn extract_src(tag: &str) -> Option<String> {
    let src_pos = crate::markdown::find_ci(tag, "src=")?;
    let after = &tag[src_pos + 4..];

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

/// Resolve a relative iframe src against a base URL.
fn resolve_iframe_src(base: &str, src: &str) -> String {
    if src.starts_with("http://") || src.starts_with("https://") {
        return src.to_string();
    }
    if src.starts_with("//") {
        if let Some(prefix) = base.split("://").next() {
            return format!("{}:{}", prefix, src);
        }
        return src.to_string();
    }
    if let Ok(base_url) = Url::parse(base) {
        if let Ok(resolved) = base_url.join(src) {
            return resolved.to_string();
        }
    }
    src.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn browser_fetch_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/page")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><body>Hello</body></html>")
            .create_async()
            .await;

        let browser = Browser::new(BrowserOptions::default()).unwrap();
        let html = browser.fetch(&format!("{}/page", server.url())).await.unwrap();

        assert!(html.contains("Hello"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn browser_fetch_404() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/missing")
            .with_status(404)
            .create_async()
            .await;

        let browser = Browser::new(BrowserOptions::default()).unwrap();
        let result = browser.fetch(&format!("{}/missing", server.url())).await;

        assert!(result.is_err());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn browser_sends_cookies() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/private")
            .match_header("cookie", "session=abc123; auth=xyz")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><body>Secret</body></html>")
            .create_async()
            .await;

        let mut opts = BrowserOptions::default();
        opts.cookies = vec!["session=abc123".to_string(), "auth=xyz".to_string()];
        let browser = Browser::new(opts).unwrap();
        let html = browser
            .fetch(&format!("{}/private", server.url()))
            .await
            .unwrap();

        assert!(html.contains("Secret"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn browser_sends_custom_headers() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api")
            .match_header("x-api-key", "secret123")
            .match_header("authorization", "Bearer token")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><body>API</body></html>")
            .create_async()
            .await;

        let mut opts = BrowserOptions::default();
        opts.headers = vec![
            "X-API-Key: secret123".to_string(),
            "Authorization: Bearer token".to_string(),
        ];
        let browser = Browser::new(opts).unwrap();
        let html = browser
            .fetch(&format!("{}/api", server.url()))
            .await
            .unwrap();

        assert!(html.contains("API"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn browser_inlines_iframe_content() {
        let mut server = mockito::Server::new_async().await;
        let iframe_mock = server
            .mock("GET", "/widget")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<p>Widget Content</p>")
            .create_async()
            .await;

        let main_html = format!(
            r#"<html><body><h1>Main</h1><iframe src="{}/widget"></iframe><p>After</p></body></html>"#,
            server.url()
        );

        let main_mock = server
            .mock("GET", "/main")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(main_html)
            .create_async()
            .await;

        let browser = Browser::new(BrowserOptions::default()).unwrap();
        let html = browser.fetch(&format!("{}/main", server.url())).await.unwrap();
        let inlined = browser
            .inline_iframes(&html, &format!("{}/main", server.url()))
            .await
            .unwrap();

        assert!(inlined.contains("Widget Content"));
        assert!(inlined.contains("Main"));
        assert!(inlined.contains("After"));
        assert!(!inlined.contains("<iframe"));

        iframe_mock.assert_async().await;
        main_mock.assert_async().await;
    }

    #[tokio::test]
    async fn browser_inlines_iframe_resolves_relative_src() {
        let mut server = mockito::Server::new_async().await;
        let iframe_mock = server
            .mock("GET", "/nested/page")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<b>Nested</b>")
            .create_async()
            .await;

        let browser = Browser::new(BrowserOptions::default()).unwrap();
        let html = r#"<div><iframe src="nested/page"></iframe></div>"#;
        let inlined = browser
            .inline_iframes(html, &server.url())
            .await
            .unwrap();

        assert!(inlined.contains("Nested"));
        assert!(!inlined.contains("<iframe"));
        iframe_mock.assert_async().await;
    }

    #[tokio::test]
    async fn browser_enforces_request_delay() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/page")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><body>Hello</body></html>")
            .expect(2)
            .create_async()
            .await;

        let mut opts = BrowserOptions::default();
        opts.request_delay = Duration::from_millis(200);
        let browser = Browser::new(opts).unwrap();

        let start = Instant::now();
        let _ = browser.fetch(&format!("{}/page", server.url())).await.unwrap();
        let _ = browser.fetch(&format!("{}/page", server.url())).await.unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed >= Duration::from_millis(200),
            "expected delay between requests, got {:?}",
            elapsed
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn browser_cache_hit_avoids_second_request() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cached")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><body>Cached content</body></html>")
            .expect(1)
            .create_async()
            .await;

        let mut opts = BrowserOptions::default();
        opts.cache_ttl = Duration::from_secs(60);
        let browser = Browser::new(opts).unwrap();

        let url = format!("{}/cached", server.url());
        let html1 = browser.fetch(&url).await.unwrap();
        let html2 = browser.fetch(&url).await.unwrap();

        assert_eq!(html1, html2);
        assert!(html1.contains("Cached content"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn browser_cache_disabled_by_default() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/nocache")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><body>Content</body></html>")
            .expect(2)
            .create_async()
            .await;

        let browser = Browser::new(BrowserOptions::default()).unwrap();

        let url = format!("{}/nocache", server.url());
        let _ = browser.fetch(&url).await.unwrap();
        let _ = browser.fetch(&url).await.unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn browser_cache_expires_after_ttl() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/expiry")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><body>Content</body></html>")
            .expect(2)
            .create_async()
            .await;

        let mut opts = BrowserOptions::default();
        opts.cache_ttl = Duration::from_millis(50);
        let browser = Browser::new(opts).unwrap();

        let url = format!("{}/expiry", server.url());
        let _ = browser.fetch(&url).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = browser.fetch(&url).await.unwrap();

        mock.assert_async().await;
    }
}
