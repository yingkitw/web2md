//! CLI-side option assembly: build [`BrowserOptions`] from common flag groups,
//! blacklist toggles, include-selector filtering, and webhook delivery.

use anyhow::Context;
use std::time::Duration;

use web2md::BrowserOptions;

pub(crate) const MOBILE_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1 Web2MD/",
    env!("CARGO_PKG_VERSION")
);

pub(crate) fn apply_blacklist_options(
    options: &mut BrowserOptions,
    no_blacklist: bool,
    no_user_blacklist: bool,
    blacklist_file: Vec<String>,
) {
    options.filter_blacklisted_urls = !no_blacklist;
    options.load_user_blacklist = !no_user_blacklist;
    options.extra_blacklist_files = blacklist_file;
}

/// Load cookies from a file.
///
/// Supports:
/// - simple `name=value` lines (`#` comments / blank lines ignored)
/// - Netscape cookie jar lines (`domain\tflag\tpath\tsecure\texpiry\tname\tvalue`)
pub(crate) fn load_cookie_file(path: &str) -> anyhow::Result<Vec<String>> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("reading cookie file {}", path))?;
    Ok(parse_cookie_file(&data))
}

/// Parse cookie file contents into `name=value` pairs.
pub(crate) fn parse_cookie_file(data: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Netscape HTTP Cookie File: 7+ tab-separated fields.
        if trimmed.contains('\t') {
            let parts: Vec<&str> = trimmed.split('\t').collect();
            if parts.len() >= 7 {
                let name = parts[5].trim();
                let value = parts[6].trim();
                if !name.is_empty() {
                    out.push(format!("{name}={value}"));
                }
            }
            continue;
        }
        // Simple name=value
        if let Some((name, value)) = trimmed.split_once('=') {
            let name = name.trim();
            if !name.is_empty() {
                out.push(format!("{name}={}", value.trim()));
            }
        }
    }
    out
}

/// POST `body` as JSON to `url`. Returns Ok(()) for any 2xx response. Non-2xx
/// are returned as errors so callers can log. Network errors also propagate.
pub(crate) async fn post_webhook(url: &str, body: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("building webhook client")?;
    let resp = client
        .post(url)
        .header("content-type", "application/json")
        .header("user-agent", "web2md-webhook/0.1")
        .body(body.to_string())
        .send()
        .await
        .context("POSTing webhook")?;
    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else {
        anyhow::bail!("webhook returned HTTP {}", status)
    }
}

/// Merge `--include-tags` (alphanumeric tag names) into the CSS include-selector list.
pub(crate) fn merge_include_selectors(
    selectors: &[String],
    tags: &[String],
) -> Vec<String> {
    let mut out = selectors.to_vec();
    for tag in tags {
        let name = tag.trim().to_ascii_lowercase();
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric()) {
            out.push(name);
        }
    }
    out
}

/// Keep only HTML elements matching the given CSS selectors (e.g. `article`, `.content`, `#main`).
/// Returns the concatenated inner HTML of all matching elements. If no selectors
/// are given or no matches are found, returns the original HTML unchanged.
pub(crate) fn filter_by_include_selectors(html: &str, selectors: &[String]) -> String {
    if selectors.is_empty() {
        return html.to_string();
    }
    use scraper::{Html, Selector};
    let document = Html::parse_document(html);
    let mut kept = String::new();
    for sel_str in selectors {
        let trimmed = sel_str.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(selector) = Selector::parse(trimmed) {
            for element in document.select(&selector) {
                kept.push_str(&element.html());
            }
        }
    }
    if kept.is_empty() {
        html.to_string()
    } else {
        format!("<html><body>{}</body></html>", kept)
    }
}

/// Build [`BrowserOptions`] from common CLI flags shared by fetch/browse/batch.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_browser_options(
    timeout: Option<u64>,
    delay: Option<u64>,
    cache_ttl: Option<u64>,
    cookies: Vec<String>,
    headers: Vec<String>,
    no_blacklist: bool,
    no_user_blacklist: bool,
    blacklist_file: Vec<String>,
    _ignore_robots: bool,
    respect_robots: bool,
) -> BrowserOptions {
    let mut options = BrowserOptions::default();
    if let Some(secs) = timeout {
        options.timeout = Duration::from_secs(secs);
    }
    if let Some(ms) = delay {
        options.request_delay = Duration::from_millis(ms);
    }
    if let Some(secs) = cache_ttl {
        options.cache_ttl = Duration::from_secs(secs);
    }
    options.cookies = cookies;
    options.headers = headers;
    options.respect_robots_txt = respect_robots;
    apply_blacklist_options(&mut options, no_blacklist, no_user_blacklist, blacklist_file);
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cookie_file_simple_and_netscape() {
        let data = r#"
# comment
session=abc123
auth = xyz

# Netscape HTTP Cookie File
.example.com	TRUE	/	FALSE	0	token	secret
"#;
        let cookies = parse_cookie_file(data);
        assert_eq!(
            cookies,
            vec![
                "session=abc123".to_string(),
                "auth=xyz".to_string(),
                "token=secret".to_string(),
            ]
        );
    }

    #[test]
    fn merge_include_selectors_appends_valid_tags() {
        let sels = vec![".content".to_string()];
        let tags = vec!["article".into(), "main!".into(), "MAIN".into()];
        assert_eq!(
            merge_include_selectors(&sels, &tags),
            vec![".content".to_string(), "article".into(), "main".into()]
        );
    }

    #[test]
    fn include_tags_keeps_named_elements() {
        let html = r#"<nav>Nav</nav><article><p>Keep me</p></article><footer>Foot</footer>"#;
        let sels = merge_include_selectors(&[], &["article".into()]);
        let kept = filter_by_include_selectors(html, &sels);
        assert!(kept.contains("Keep me"));
        assert!(!kept.contains("Nav"));
        assert!(!kept.contains("Foot"));
    }
}
