use url::Url;

/// Host suffixes commonly used for ads, analytics, and tracking pixels.
const BLACKLISTED_HOST_SUFFIXES: &[&str] = &[
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
const BLACKLISTED_PATH_FRAGMENTS: &[&str] = &[
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

/// Returns true when a URL is a known non-content resource (ad, tracker, pixel).
pub fn is_blacklisted(url: &str) -> bool {
    let parsed = match Url::parse(url) {
        Ok(u) => u,
        Err(_) => return matches_non_content_path(url),
    };

    if let Some(host) = parsed.host_str() {
        let host = host.to_ascii_lowercase();
        if BLACKLISTED_HOST_SUFFIXES
            .iter()
            .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
        {
            return true;
        }
    }

    let path_query = format!(
        "{}{}",
        parsed.path().to_ascii_lowercase(),
        parsed.query().map(|q| format!("?{q}")).unwrap_or_default()
    );
    if matches_path_fragments(&path_query) {
        return true;
    }

    false
}

/// Remove blacklisted URLs from a list (e.g. sitemap or batch input).
pub fn filter_blacklisted_urls(urls: impl IntoIterator<Item = String>) -> Vec<String> {
    urls.into_iter()
        .filter(|u| !is_blacklisted(u))
        .collect()
}

fn matches_non_content_path(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    matches_path_fragments(&lower)
}

fn matches_path_fragments(lower: &str) -> bool {
    BLACKLISTED_PATH_FRAGMENTS
        .iter()
        .any(|frag| lower.contains(frag))
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
        let urls = vec![
            "https://example.com/article".to_string(),
            "https://cdn.doubleclick.net/ad".to_string(),
            "https://example.com/about".to_string(),
        ];
        let filtered = filter_blacklisted_urls(urls);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/article".to_string()));
        assert!(filtered.contains(&"https://example.com/about".to_string()));
    }
}
