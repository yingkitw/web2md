use anyhow::{Context, Result};
use reqwest::{Client, ClientBuilder};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use url::Url;

use crate::url_blacklist::BlacklistPatterns;
use crate::{DEFAULT_TIMEOUT, DEFAULT_USER_AGENT};
use crate::robots::{is_robots_txt_url, robots_origin, RobotsTxt};

/// Parse URLs from sitemap XML content.
/// Extracts all `<loc>` tag values from sitemap.xml format.
pub fn parse_sitemap_urls(xml: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut pos = 0;
    while pos < xml.len() {
        if let Some(start) = xml[pos..].find("<loc>") {
            let start = pos + start + 5;
            if let Some(end) = xml[start..].find("</loc>") {
                let url = xml[start..start + end].trim().to_string();
                if !url.is_empty() {
                    urls.push(url);
                }
                pos = start + end + 6;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    urls
}

/// Extract feed URLs (RSS/Atom) from HTML <link> tags.
/// Looks for <link rel="alternate" type="application/rss+xml" href="...">
/// and <link rel="alternate" type="application/atom+xml" href="...">.
pub fn extract_feed_links(html: &str) -> Vec<String> {
    let mut feeds = Vec::new();
    let mut pos = 0;
    while pos < html.len() {
        if let Some(start) = html[pos..].find("<link") {
            let start = pos + start;
            if let Some(end) = html[start..].find('>') {
                let tag = &html[start..=start + end];
                if (tag.contains("application/rss+xml") || tag.contains("application/atom+xml"))
                    && tag.contains("alternate")
                {
                    if let Some(href) = extract_href(tag) {
                        feeds.push(href);
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
    feeds
}

/// Extract the href attribute value from an HTML tag string.
fn extract_href(tag: &str) -> Option<String> {
    let needle = "href=";
    let pos = tag.find(needle)?;
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

/// Configuration for the HTTP client
#[derive(Debug, Clone)]
pub struct BrowserOptions {
    /// Request timeout
    pub timeout: Duration,
    /// User-Agent string
    pub user_agent: String,
    /// Follow redirects
    pub follow_redirects: bool,
    /// Execute inline `<script>` blocks via the built-in JS interpreter,
    /// capturing `document.write` output into the page.
    pub enable_javascript: bool,
    /// Initial cookies to send with every request (format: "name=value")
    pub cookies: Vec<String>,
    /// Custom HTTP headers to send with every request (format: "Name: Value")
    pub headers: Vec<String>,
    /// Minimum delay between consecutive requests to the same host
    pub request_delay: Duration,
    /// Cache TTL for fetched pages (zero = caching disabled)
    pub cache_ttl: Duration,
    /// Skip known non-content URLs (ads, tracking pixels) on secondary fetches
    pub filter_blacklisted_urls: bool,
    /// Fetch and honor robots.txt disallow rules and crawl-delay
    pub respect_robots_txt: bool,
    /// Load `~/.web2md/blacklist.txt` when present
    pub load_user_blacklist: bool,
    /// Additional blacklist pattern files (one pattern per line)
    pub extra_blacklist_files: Vec<String>,
    /// Post-load wait after fetch (milliseconds) for JS-heavy pages; also caps setTimeout flush
    pub post_load_wait: Duration,
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
            filter_blacklisted_urls: true,
            respect_robots_txt: true,
            load_user_blacklist: true,
            extra_blacklist_files: Vec::new(),
            post_load_wait: Duration::from_millis(0),
        }
    }
}

/// Minimal HTTP client: fetches raw HTML only.
/// No rendering engine—intentionally lightweight for MCP token efficiency.
pub struct Browser {
    client: Client,
    options: BrowserOptions,
    blacklist: BlacklistPatterns,
    last_request: Mutex<Option<Instant>>,
    cache: Mutex<HashMap<String, (String, Instant)>>,
    robots_cache: Mutex<HashMap<String, RobotsTxt>>,
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

        let mut custom = BlacklistPatterns::default();
        if options.load_user_blacklist {
            if let Some(path) = crate::url_blacklist::default_user_blacklist_path() {
                if path.is_file() {
                    custom = custom.merge(BlacklistPatterns::load_file(&path)?);
                }
            }
        }
        for path in &options.extra_blacklist_files {
            custom = custom.merge(BlacklistPatterns::load_file(std::path::Path::new(path))?);
        }
        let blacklist = BlacklistPatterns::builtin().merge(custom);

        Ok(Self {
            client,
            options,
            blacklist,
            last_request: Mutex::new(None),
            cache: Mutex::new(HashMap::new()),
            robots_cache: Mutex::new(HashMap::new()),
        })
    }

    /// Same-origin links from HTML, excluding URLs blocked by the active blacklist.
    pub fn same_origin_links(&self, html: &str, page_url: &str, root: &Url) -> Vec<String> {
        crate::crawl::extract_page_links(html, page_url)
            .into_iter()
            .filter(|url| crate::crawl::is_same_origin(url, root) && !self.is_url_blocked(url))
            .collect()
    }

    /// Returns false when robots.txt disallows fetching `url`.
    pub async fn robots_allows(&self, url: &str) -> Result<bool> {
        if !self.options.respect_robots_txt {
            return Ok(true);
        }
        let parsed = Url::parse(url).context("Invalid URL")?;
        if is_robots_txt_url(&parsed) {
            return Ok(true);
        }
        let Some(origin) = robots_origin(&parsed) else {
            return Ok(true);
        };
        let rules = self.robots_for_origin(&origin).await?;
        Ok(rules.is_allowed(url))
    }

    async fn robots_for_origin(&self, origin: &str) -> Result<RobotsTxt> {
        {
            let cache = self.robots_cache.lock().unwrap();
            if let Some(rules) = cache.get(origin) {
                return Ok(rules.clone());
            }
        }

        let robots_url = Url::parse(origin)
            .context("Invalid robots origin")?
            .join("/robots.txt")
            .context("Invalid robots.txt URL")?
            .to_string();
        let rules = match self.fetch_raw(&robots_url).await {
            Ok(body) => RobotsTxt::parse(&body, &self.options.user_agent),
            Err(_) => RobotsTxt::allow_all(),
        };

        self.robots_cache
            .lock()
            .unwrap()
            .insert(origin.to_string(), rules.clone());
        Ok(rules)
    }

    /// Enforce delay from CLI `--delay` and robots.txt crawl-delay (whichever is greater).
    async fn enforce_delay(&self, robots_delay: Option<Duration>) {
        let delay = self
            .options
            .request_delay
            .max(robots_delay.unwrap_or(Duration::ZERO));
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

    /// Returns true when URL blacklist filtering is enabled and the URL is blocked.
    pub fn is_url_blocked(&self, url: &str) -> bool {
        self.options.filter_blacklisted_urls && self.blacklist.is_blacklisted(url)
    }

    /// Fetch raw HTML from a URL
    pub async fn fetch(&self, url: &str) -> Result<String> {
        // Check cache first
        if !self.options.cache_ttl.is_zero() {
            if let Some(cached) = self.lookup_cache(url) {
                return Ok(cached);
            }
        }

        let parsed = Url::parse(url).context("Invalid URL")?;
        let robots_delay = if self.options.respect_robots_txt && !is_robots_txt_url(&parsed) {
            let origin = robots_origin(&parsed).context("Invalid URL host")?;
            let rules = self.robots_for_origin(&origin).await?;
            if !rules.is_allowed(url) {
                anyhow::bail!("Blocked by robots.txt: {url}");
            }
            rules.crawl_delay()
        } else {
            None
        };

        self.enforce_delay(robots_delay).await;
        self.fetch_raw(url).await
    }

    /// HTTP GET without robots.txt checks (used for robots.txt itself).
    async fn fetch_raw(&self, url: &str) -> Result<String> {
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
            if let Some(start) = crate::html_util::find_ci(&html[i..], "<iframe") {
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

                    let close_end = crate::html_util::find_ci(&html[tag_end..], "</iframe>")
                        .map(|p| tag_end + p + "</iframe>".len());

                    let replacement = if let Some(url) = src {
                        let resolved = resolve_iframe_src(base_url, &url);
                        if self.is_url_blocked(&resolved) {
                            String::new()
                        } else {
                            match self.fetch(&resolved).await {
                                Ok(content) => content,
                                Err(_) => String::new(),
                            }
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

    /// Execute inline `<script>` blocks and inject any HTML captured via
    /// `document.write` back into the page, when `enable_javascript` is set.
    ///
    /// Scripts are evaluated with the built-in dependency-free JS subset
    /// interpreter (`crate::js`); external scripts, modules, and unsupported
    /// features are silently skipped. When JavaScript is disabled the input is
    /// returned unchanged. Call this after [`inline_iframes`](Self::inline_iframes)
    /// and before Markdown conversion.
    pub fn run_inline_scripts(&self, html: &str) -> String {
        if !self.options.enable_javascript {
            return html.to_string();
        }
        let wait_ms = self.options.post_load_wait.as_millis() as u64;
        let captured = crate::js::run_inline_scripts(html, wait_ms);
        if captured.is_empty() {
            return html.to_string();
        }
        crate::js::inject_before_body_close(html, &captured)
    }

    /// Sleep for [`BrowserOptions::post_load_wait`] after a page fetch.
    pub async fn post_load_wait(&self) {
        if !self.options.post_load_wait.is_zero() {
            tokio::time::sleep(self.options.post_load_wait).await;
        }
    }

    /// Apply post-load wait, inline iframes, and run inline scripts.
    pub async fn prepare_html(&self, html: &str, url: &str) -> Result<String> {
        self.post_load_wait().await;
        let html = self.inline_iframes(html, url).await?;
        Ok(self.run_inline_scripts(&html))
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
    let src_pos = crate::html_util::find_ci(tag, "src=")?;
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
    async fn browser_skips_blacklisted_iframe_src() {
        let mut server = mockito::Server::new_async().await;
        let iframe_mock = server
            .mock("GET", "/pixel.gif")
            .with_status(200)
            .with_header("content-type", "image/gif")
            .with_body("GIF89a")
            .expect(0)
            .create_async()
            .await;

        let browser = Browser::new(BrowserOptions::default()).unwrap();
        let html = r#"<html><body><p>Main</p><iframe src="/pixel.gif"></iframe></body></html>"#;
        let inlined = browser
            .inline_iframes(html, &server.url())
            .await
            .unwrap();

        assert!(inlined.contains("Main"));
        assert!(!inlined.contains("GIF89a"));
        assert!(!inlined.contains("<iframe"));
        iframe_mock.assert_async().await;
    }

    #[tokio::test]
    async fn browser_fetches_blacklisted_iframe_when_filter_disabled() {
        let mut server = mockito::Server::new_async().await;
        let iframe_mock = server
            .mock("GET", "/pixel.gif")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<p>Pixel content</p>")
            .create_async()
            .await;

        let mut opts = BrowserOptions::default();
        opts.filter_blacklisted_urls = false;
        let browser = Browser::new(opts).unwrap();
        let html = r#"<html><body><iframe src="/pixel.gif"></iframe></body></html>"#;
        let inlined = browser
            .inline_iframes(html, &server.url())
            .await
            .unwrap();

        assert!(inlined.contains("Pixel content"));
        iframe_mock.assert_async().await;
    }

    #[tokio::test]
    async fn browser_respects_robots_disallow() {
        let mut server = mockito::Server::new_async().await;
        let robots = server
            .mock("GET", "/robots.txt")
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body("User-agent: *\nDisallow: /private/\n")
            .expect(1)
            .create_async()
            .await;

        let blocked = server
            .mock("GET", "/private/secret")
            .with_status(200)
            .with_body("secret")
            .expect(0)
            .create_async()
            .await;

        let browser = Browser::new(BrowserOptions::default()).unwrap();
        let target = format!("{}/private/secret", server.url());
        assert!(
            !browser.robots_allows(&target).await.unwrap(),
            "expected robots.txt to disallow {target}"
        );
        let err = browser.fetch(&target).await.unwrap_err().to_string();
        assert!(err.contains("robots.txt"));

        robots.assert_async().await;
        blocked.assert_async().await;
    }

    #[tokio::test]
    async fn browser_ignore_robots_fetches_disallowed_path() {
        let mut server = mockito::Server::new_async().await;
        let _robots = server
            .mock("GET", "/robots.txt")
            .with_status(200)
            .with_body("User-agent: *\nDisallow: /private/\n")
            .create_async()
            .await;

        let private = server
            .mock("GET", "/private/page")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><body>Private</body></html>")
            .create_async()
            .await;

        let mut opts = BrowserOptions::default();
        opts.respect_robots_txt = false;
        let browser = Browser::new(opts).unwrap();
        let html = browser
            .fetch(&format!("{}/private/page", server.url()))
            .await
            .unwrap();
        assert!(html.contains("Private"));
        private.assert_async().await;
    }

    #[test]
    fn browser_loads_custom_blacklist_file() {
        let dir = std::env::temp_dir().join(format!("web2md-bl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("extra.txt");
        std::fs::write(&file, "evil-tracker.test\n/blocked-path/\n").unwrap();

        let mut opts = BrowserOptions::default();
        opts.load_user_blacklist = false;
        opts.extra_blacklist_files = vec![file.to_string_lossy().into_owned()];
        let browser = Browser::new(opts).unwrap();

        assert!(browser.is_url_blocked("https://cdn.evil-tracker.test/pixel"));
        assert!(browser.is_url_blocked("https://example.com/blocked-path/page"));
        assert!(!browser.is_url_blocked("https://example.com/blog/post"));

        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn parse_sitemap_urls_extracts_all_locs() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://example.com/</loc><lastmod>2025-01-01</lastmod></url>
  <url><loc>https://example.com/about</loc></url>
  <url><loc>https://example.com/contact</loc></url>
</urlset>"#;
        let urls = parse_sitemap_urls(xml);
        assert_eq!(urls.len(), 3);
        assert_eq!(urls[0], "https://example.com/");
        assert_eq!(urls[1], "https://example.com/about");
        assert_eq!(urls[2], "https://example.com/contact");
    }

    #[test]
    fn parse_sitemap_urls_handles_empty() {
        let xml = "<?xml version=\"1.0\"?><urlset></urlset>";
        let urls = parse_sitemap_urls(xml);
        assert!(urls.is_empty());
    }

    #[test]
    fn parse_sitemap_urls_handles_sitemap_index() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap><loc>https://example.com/sitemap1.xml</loc></sitemap>
  <sitemap><loc>https://example.com/sitemap2.xml</loc></sitemap>
</sitemapindex>"#;
        let urls = parse_sitemap_urls(xml);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com/sitemap1.xml");
        assert_eq!(urls[1], "https://example.com/sitemap2.xml");
    }

    #[test]
    fn parse_sitemap_urls_skips_empty_locs() {
        let xml = r#"<urlset><url><loc></loc></url><url><loc>https://example.com/page</loc></url></urlset>"#;
        let urls = parse_sitemap_urls(xml);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://example.com/page");
    }

    #[test]
    fn extract_feed_links_finds_rss() {
        let html = r#"<html><head>
            <link rel="alternate" type="application/rss+xml" href="/feed.xml" title="RSS Feed">
        </head><body></body></html>"#;
        let feeds = extract_feed_links(html);
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0], "/feed.xml");
    }

    #[test]
    fn extract_feed_links_finds_atom() {
        let html = r#"<html><head>
            <link rel="alternate" type="application/atom+xml" href="https://example.com/atom.xml">
        </head><body></body></html>"#;
        let feeds = extract_feed_links(html);
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0], "https://example.com/atom.xml");
    }

    #[test]
    fn extract_feed_links_finds_multiple() {
        let html = r#"<html><head>
            <link rel="alternate" type="application/rss+xml" href="/rss">
            <link rel="alternate" type="application/atom+xml" href="/atom">
            <link rel="stylesheet" href="/style.css">
        </head><body></body></html>"#;
        let feeds = extract_feed_links(html);
        assert_eq!(feeds.len(), 2);
        assert!(feeds.contains(&"/rss".to_string()));
        assert!(feeds.contains(&"/atom".to_string()));
    }

    #[test]
    fn extract_feed_links_ignores_non_feed_links() {
        let html = r#"<html><head>
            <link rel="stylesheet" href="/style.css">
            <link rel="icon" href="/favicon.ico">
        </head><body></body></html>"#;
        let feeds = extract_feed_links(html);
        assert!(feeds.is_empty());
    }

    #[test]
    fn extract_feed_links_handles_single_quotes() {
        let html = r#"<html><head>
            <link rel='alternate' type='application/rss+xml' href='/feed.rss'>
        </head><body></body></html>"#;
        let feeds = extract_feed_links(html);
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0], "/feed.rss");
    }
}
