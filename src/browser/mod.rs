use anyhow::{Context, Result};
use reqwest::{Client, ClientBuilder};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use url::Url;

use crate::url_blacklist::BlacklistPatterns;
use crate::{DEFAULT_TIMEOUT, DEFAULT_USER_AGENT};
use crate::robots::{is_robots_txt_url, robots_origin, RobotsTxt};

mod sitemap;
#[cfg(test)]
mod tests;

pub use sitemap::parse_sitemap_urls;

/// Configuration for the HTTP client
#[derive(Debug, Clone)]
pub struct BrowserOptions {
    /// Request timeout
    pub timeout: Duration,
    /// User-Agent string
    pub user_agent: String,
    /// Initial cookies to send with every request (format: "name=value")
    pub cookies: Vec<String>,
    /// Custom HTTP headers to send with every request (format: "Name: Value")
    pub headers: Vec<String>,
    /// Minimum delay between consecutive requests to the same host
    pub request_delay: Duration,
    /// Per-host requests-per-second limit. `None` or `0` disables per-host throttling.
    /// Per-host pacing is tracked separately from the global `request_delay` so the
    /// heavier of the two always applies.
    pub host_rate_limit: Option<f64>,
    /// Cache TTL for fetched pages (zero = caching disabled)
    pub cache_ttl: Duration,
    /// Optional directory for persistent JSONL cache files (survives process restarts).
    /// When set, fetched pages persist under `{cache_dir}/{sha256(url)}.json` with the
    /// same `cache_ttl` and override the in-memory cache.
    pub cache_dir: Option<std::path::PathBuf>,
    /// Skip known non-content URLs (ads, tracking pixels) on secondary fetches
    pub filter_blacklisted_urls: bool,
    /// Fetch and honor robots.txt disallow rules and crawl-delay
    pub respect_robots_txt: bool,
    /// Load `~/.web2md/blacklist.txt` when present
    pub load_user_blacklist: bool,
    /// Additional blacklist pattern files (one pattern per line)
    pub extra_blacklist_files: Vec<String>,
    /// Optional HTTP/SOCKS proxy URL (e.g. "http://proxy:8080", "socks5://proxy:1080")
    pub proxy: Option<String>,
    /// Optional basic auth credentials (format: "user:password")
    pub basic_auth: Option<String>,
    /// Bypass cache entirely for this request (always hit the live URL)
    pub no_cache: bool,
    /// Only use cached entries younger than this duration (None = use cache_ttl)
    pub cache_max_age: Option<Duration>,
}

impl Default for BrowserOptions {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            user_agent: DEFAULT_USER_AGENT.to_string(),
            cookies: Vec::new(),
            headers: Vec::new(),
            request_delay: Duration::from_millis(0),
            host_rate_limit: None,
            cache_ttl: Duration::from_secs(0),
            cache_dir: None,
            filter_blacklisted_urls: true,
            respect_robots_txt: false,
            load_user_blacklist: true,
            extra_blacklist_files: Vec::new(),
            proxy: None,
            basic_auth: None,
            no_cache: false,
            cache_max_age: None,
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
    /// Last request timestamp per host (for per-host rate limiting).
    per_host_last: Mutex<HashMap<String, Instant>>,
    cache: Mutex<HashMap<String, (String, Instant)>>,
    robots_cache: Mutex<HashMap<String, RobotsTxt>>,
    /// Optional persistent file cache (when `options.cache_dir` is set).
    persistent_cache: Option<crate::PersistentCache>,
}

impl Clone for Browser {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            options: self.options.clone(),
            blacklist: self.blacklist.clone(),
            last_request: Mutex::new(*self.last_request.lock().unwrap()),
            per_host_last: Mutex::new(self.per_host_last.lock().unwrap().clone()),
            cache: Mutex::new(self.cache.lock().unwrap().clone()),
            robots_cache: Mutex::new(self.robots_cache.lock().unwrap().clone()),
            persistent_cache: self.persistent_cache.clone(),
        }
    }
}

impl Browser {
    /// Build a new Browser with the given options
    pub fn new(options: BrowserOptions) -> Result<Self> {
        let mut builder = ClientBuilder::new()
            .timeout(options.timeout)
            .user_agent(&options.user_agent)
            .redirect(reqwest::redirect::Policy::default());

        if let Some(ref proxy_url) = options.proxy {
            let proxy = reqwest::Proxy::all(proxy_url)
                .context("Failed to parse proxy URL")?;
            builder = builder.proxy(proxy);
        }

        let client = builder
            .build()
            .context("Failed to build HTTP client")?;

        let mut custom = BlacklistPatterns::default();
        if options.load_user_blacklist
            && let Some(path) = crate::url_blacklist::default_user_blacklist_path()
                && path.is_file() {
                    custom = custom.merge(BlacklistPatterns::load_file(&path)?);
                }
        for path in &options.extra_blacklist_files {
            custom = custom.merge(BlacklistPatterns::load_file(std::path::Path::new(path))?);
        }
        let blacklist = BlacklistPatterns::builtin().merge(custom);

        let persistent_cache = match options.cache_dir.as_ref() {
            Some(dir) if !options.cache_ttl.is_zero() => {
                match crate::PersistentCache::new(dir, options.cache_ttl) {
                    Ok(c) => Some(c),
                    Err(e) => {
                        eprintln!("warning: failed to initialize persistent cache: {}", e);
                        None
                    }
                }
            }
            _ => None,
        };

        Ok(Self {
            client,
            options,
            blacklist,
            last_request: Mutex::new(None),
            per_host_last: Mutex::new(HashMap::new()),
            cache: Mutex::new(HashMap::new()),
            robots_cache: Mutex::new(HashMap::new()),
            persistent_cache,
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

    /// Enforce delay from CLI `--delay` and robots.txt crawl-delay (whichever is greater),
    /// plus any per-host rate limit configured via `--rate`. The per-host pacing tracks
    /// a separate timestamp per host so two different hosts may be queried in parallel
    /// without interference.
    async fn enforce_delay(&self, robots_delay: Option<Duration>, host: &str) {
        let global_delay = self
            .options
            .request_delay
            .max(robots_delay.unwrap_or(Duration::ZERO));
        let host_delay = self
            .options
            .host_rate_limit
            .filter(|r| *r > 0.0)
            .map(|rps| Duration::from_secs_f64(1.0 / rps));

        if global_delay.is_zero() && host_delay.is_none() {
            return;
        }

        // Global pacing: block until `global_delay` has elapsed since the last request.
        if !global_delay.is_zero() {
            let sleep_for = {
                let guard = self.last_request.lock().unwrap();
                match *guard {
                    Some(last) => {
                        let elapsed = last.elapsed();
                        if elapsed < global_delay {
                            Some(global_delay - elapsed)
                        } else {
                            None
                        }
                    }
                    None => None,
                }
            };
            if let Some(d) = sleep_for {
                tokio::time::sleep(d).await;
            }
            let mut guard = self.last_request.lock().unwrap();
            *guard = Some(Instant::now());
        }

        // Per-host pacing: block until `host_delay` has elapsed since the last request to this host.
        if let Some(hd) = host_delay {
            let sleep_for = {
                let guard = self.per_host_last.lock().unwrap();
                match guard.get(host).copied() {
                    Some(last) => {
                        let elapsed = last.elapsed();
                        if elapsed < hd {
                            Some(hd - elapsed)
                        } else {
                            None
                        }
                    }
                    None => None,
                }
            };
            if let Some(d) = sleep_for {
                tokio::time::sleep(d).await;
            }
            let mut guard = self.per_host_last.lock().unwrap();
            guard.insert(host.to_string(), Instant::now());
        }
    }

    /// Returns true when URL blacklist filtering is enabled and the URL is blocked.
    pub fn is_url_blocked(&self, url: &str) -> bool {
        self.options.filter_blacklisted_urls && self.blacklist.is_blacklisted(url)
    }

    /// Fetch raw HTML from a URL
    pub async fn fetch(&self, url: &str) -> Result<String> {
        // Check cache first (unless --no-cache is set)
        if !self.options.no_cache
            && !self.options.cache_ttl.is_zero()
            && let Some(cached) = self.lookup_cache(url) {
                return Ok(cached);
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

        self.enforce_delay(robots_delay, parsed.host_str().unwrap_or("")).await;
        let body = self.fetch_raw(url).await?;
        // Persist to persistent cache, if configured and not bypassed.
        if !self.options.no_cache
            && let Some(cache) = &self.persistent_cache
            && let Err(e) = cache.put(url, &body) {
                eprintln!("warning: failed to persist cache entry for {}: {}", url, e);
            }
        Ok(body)
    }

    /// Expand a sitemap or sitemap index recursively, returning all leaf page URLs.
    /// Resolves relative `<loc>` values against the sitemap URL, follows nested
    /// `<sitemapindex>` documents up to a fixed depth, and filters blacklisted URLs.
    pub async fn expand_sitemap(&self, sitemap_url: &str, xml: &str) -> Vec<String> {
        const MAX_DEPTH: usize = 5;
        let mut page_urls = Vec::new();
        let mut seen = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back((sitemap_url.to_string(), xml.to_string(), 0usize));

        while let Some((base, xml, depth)) = queue.pop_front() {
            let base_url = match Url::parse(&base) {
                Ok(u) => u,
                Err(_) => continue,
            };
            let locs: Vec<String> = parse_sitemap_urls(&xml)
                .into_iter()
                .map(|loc| base_url.join(&loc).map(|u| u.to_string()).unwrap_or(loc))
                .filter(|u| !self.is_url_blocked(u))
                .collect();
            if xml.to_ascii_lowercase().contains("<sitemapindex") {
                if depth >= MAX_DEPTH {
                    continue;
                }
                for loc in locs {
                    if !seen.insert(loc.clone()) {
                        continue;
                    }
                    match self.fetch(&loc).await {
                        Ok(child) => queue.push_back((loc, child, depth + 1)),
                        Err(e) => eprintln!("warning: failed to fetch sitemap {}: {}", loc, e),
                    }
                }
            } else {
                page_urls.extend(locs);
            }
        }
        page_urls
    }

    /// Fetch a URL while bypassing robots.txt checks.
    /// Used for URLs derived from already-fetched page content, e.g. YouTube
    /// caption tracks, where the original fetch was already user-requested.
    pub async fn fetch_ignore_robots(&self, url: &str) -> Result<String> {
        self.fetch_raw(url).await
    }

    /// Fetch raw HTML from a URL, streaming chunks to `on_chunk` as they arrive.
    /// The full body is accumulated and returned for downstream conversion.
    /// Bypasses cache (always hits the live URL).
    pub async fn fetch_stream<F>(&self, url: &str, mut on_chunk: F) -> Result<String>
    where
        F: FnMut(&[u8]),
    {
        use futures_util::StreamExt;

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

        self.enforce_delay(robots_delay, parsed.host_str().unwrap_or("")).await;

        let mut req = self.client.get(parsed.clone());
        if !self.options.cookies.is_empty() {
            req = req.header(reqwest::header::COOKIE, self.options.cookies.join("; "));
        }
        for h in &self.options.headers {
            if let Some((name, value)) = h.split_once(':') {
                req = req.header(name.trim(), value.trim());
            }
        }
        if let Some(ref auth) = self.options.basic_auth
            && let Some((user, pass)) = auth.split_once(':') {
                req = req.basic_auth(user.trim(), Some(pass.trim()));
            }

        let resp = req.send().await.context("HTTP request failed")?;
        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("HTTP error: {}", status);
        }

        let mut body = Vec::with_capacity(64 * 1024);
        let mut stream = resp.bytes_stream();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.context("Failed to read stream chunk")?;
            on_chunk(&chunk);
            body.extend_from_slice(&chunk);
        }

        let body = String::from_utf8_lossy(&body).into_owned();

        if !self.options.no_cache && !self.options.cache_ttl.is_zero() {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(url.to_string(), (body.clone(), Instant::now()));
        }
        if !self.options.no_cache
            && let Some(cache) = &self.persistent_cache
            && let Err(e) = cache.put(url, &body) {
                eprintln!("warning: failed to persist cache entry for {}: {}", url, e);
            }

        Ok(body)
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
        if let Some(ref auth) = self.options.basic_auth
            && let Some((user, pass)) = auth.split_once(':') {
                req = req.basic_auth(user.trim(), Some(pass.trim()));
            }

        let resp = req.send().await.context("HTTP request failed")?;

        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("HTTP error: {}", status);
        }

        let body = resp.text().await.context("Failed to read response body")?;

        // Store in cache if enabled and not bypassed via --no-cache
        if !self.options.no_cache && !self.options.cache_ttl.is_zero() {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(url.to_string(), (body.clone(), Instant::now()));
        }

        Ok(body)
    }

    /// Look up a URL in the cache, returning the body if not expired.
    /// Prefers persistent cache when configured, falls back to in-memory.
    fn lookup_cache(&self, url: &str) -> Option<String> {
        if let Some(persistent) = &self.persistent_cache {
            if let Some(body) = persistent.get(url) {
                // Check cache_max_age if set
                if let Some(max_age) = self.options.cache_max_age
                    && let Some(fetched_ms) = persistent.fetched_at(url)
                {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let age = Duration::from_millis(now.saturating_sub(fetched_ms));
                    if age > max_age {
                        return None;
                    }
                }
                return Some(body);
            }
            return None;
        }
        let mut cache = self.cache.lock().unwrap();
        if let Some((body, fetched_at)) = cache.get(url) {
            let max = self.options.cache_max_age.unwrap_or(self.options.cache_ttl);
            if fetched_at.elapsed() < max {
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
                            self.fetch(&resolved).await.unwrap_or_default()
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

    /// Inline `<iframe>` content into the fetched HTML before conversion.
    pub async fn prepare_html(&self, html: &str, url: &str) -> Result<String> {
        self.inline_iframes(html, url).await
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
    if let Ok(base_url) = Url::parse(base)
        && let Ok(resolved) = base_url.join(src) {
            return resolved.to_string();
        }
    src.to_string()
}
