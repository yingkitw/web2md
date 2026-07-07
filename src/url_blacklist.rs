use std::path::Path;

use anyhow::{Context, Result};
use url::Url;

/// Host suffixes commonly used for ads, analytics, and tracking pixels.
const BUILTIN_HOST_SUFFIXES: &[&str] = &[
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "google-analytics.com",
    "analytics.google.com",
    "adservice.google.com",
    "scorecardresearch.com",
    "quantserve.com",
    "hotjar.com",
    "mixpanel.com",
    "segment.io",
    "segment.com",
    "newrelic.com",
    "optimizely.com",
    "taboola.com",
    "outbrain.com",
    "adnxs.com",
    "moatads.com",
    "adsafeprotected.com",
];

/// Path fragments that indicate non-content resources (pixels, beacons, ads).
const BUILTIN_PATH_FRAGMENTS: &[&str] = &[
    "/pixel",
    "/beacon",
    "/track",
    "/tracking",
    "/collect",
    "/analytics",
    "/__utm",
    "/ads/",
    "/ad/",
    "1x1.gif",
    "pixel.gif",
    "spacer.gif",
    "blank.gif",
    "transparent.gif",
    "favicon.ico",
];

/// User-configurable and built-in URL blacklist patterns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlacklistPatterns {
    host_suffixes: Vec<String>,
    path_fragments: Vec<String>,
}

impl BlacklistPatterns {
    /// Built-in ad/tracker patterns shipped with Web2MD.
    pub fn builtin() -> Self {
        Self {
            host_suffixes: BUILTIN_HOST_SUFFIXES.iter().map(|s| s.to_string()).collect(),
            path_fragments: BUILTIN_PATH_FRAGMENTS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Parse blacklist file content (one pattern per line, `#` comments).
    pub fn parse_file(content: &str) -> Self {
        let mut patterns = Self::default();
        for line in content.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.contains('/') {
                patterns.path_fragments.push(line.to_ascii_lowercase());
            } else {
                patterns
                    .host_suffixes
                    .push(line.trim_start_matches('.').to_ascii_lowercase());
            }
        }
        patterns
    }

    /// Load patterns from a file on disk.
    pub fn load_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read blacklist file {}", path.display()))?;
        Ok(Self::parse_file(&content))
    }

    /// Merge another pattern set into this one (deduplicated).
    pub fn merge(mut self, other: Self) -> Self {
        for suffix in other.host_suffixes {
            if !self.host_suffixes.contains(&suffix) {
                self.host_suffixes.push(suffix);
            }
        }
        for frag in other.path_fragments {
            if !self.path_fragments.contains(&frag) {
                self.path_fragments.push(frag);
            }
        }
        self
    }

    /// Returns true when a URL matches any host suffix or path fragment.
    pub fn is_blacklisted(&self, url: &str) -> bool {
        let parsed = match Url::parse(url) {
            Ok(u) => u,
            Err(_) => return self.matches_path_fragments(&url.to_ascii_lowercase()),
        };

        if let Some(host) = parsed.host_str() {
            let host = host.to_ascii_lowercase();
            if self.host_suffixes.iter().any(|suffix| {
                host == *suffix || host.ends_with(&format!(".{suffix}"))
            }) {
                return true;
            }
        }

        let path_query = format!(
            "{}{}",
            parsed.path().to_ascii_lowercase(),
            parsed
                .query()
                .map(|q| format!("?{q}"))
                .unwrap_or_default()
        );
        self.matches_path_fragments(&path_query)
    }

    fn matches_path_fragments(&self, lower: &str) -> bool {
        self.path_fragments.iter().any(|frag| lower.contains(frag))
    }
}

/// Default user blacklist path: `~/.web2md/blacklist.txt`.
pub fn default_user_blacklist_path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|home| {
        std::path::PathBuf::from(home)
            .join(".web2md")
            .join("blacklist.txt")
    })
}

/// Returns true when a URL is blocked by the built-in blacklist.
pub fn is_blacklisted(url: &str) -> bool {
    BlacklistPatterns::builtin().is_blacklisted(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blacklists_known_ad_hosts() {
        assert!(is_blacklisted(
            "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js"
        ));
        assert!(is_blacklisted("https://www.google-analytics.com/collect?v=1"));
        assert!(is_blacklisted("https://cdn.doubleclick.net/ad/track"));
    }

    #[test]
    fn blacklists_tracking_pixel_paths() {
        assert!(is_blacklisted("https://example.com/pixel.gif"));
        assert!(is_blacklisted("https://example.com/images/1x1.gif"));
        assert!(is_blacklisted("https://example.com/beacon?event=click"));
        assert!(is_blacklisted("https://example.com/favicon.ico"));
    }

    #[test]
    fn allows_normal_content_urls() {
        assert!(!is_blacklisted("https://example.com/blog/post"));
        assert!(!is_blacklisted("https://docs.rs/web2md/latest/web2md/"));
        assert!(!is_blacklisted("https://news.ycombinator.com/item?id=1"));
    }

    #[test]
    fn filter_removes_blacklisted_entries() {
        let urls = [
            "https://example.com/article",
            "https://cdn.doubleclick.net/ad",
            "https://example.com/about",
        ];
        let filtered: Vec<_> = urls
            .into_iter()
            .filter(|u| !is_blacklisted(u))
            .collect();
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/article"));
        assert!(filtered.contains(&"https://example.com/about"));
    }

    #[test]
    fn parse_file_host_and_path_patterns() {
        let content = "# ads\nbadtracker.com\n/welcome-spam/\n";
        let patterns = BlacklistPatterns::parse_file(content);
        assert!(patterns.is_blacklisted("https://cdn.badtracker.com/pixel"));
        assert!(patterns.is_blacklisted("https://example.com/welcome-spam/page"));
        assert!(!patterns.is_blacklisted("https://example.com/blog"));
    }

    #[test]
    fn merge_combines_custom_with_builtin() {
        let custom = BlacklistPatterns::parse_file("evil.example\n");
        let merged = BlacklistPatterns::builtin().merge(custom);
        assert!(merged.is_blacklisted("https://evil.example/track"));
        assert!(merged.is_blacklisted("https://doubleclick.net/ad"));
    }
}
