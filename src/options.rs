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
    apply_blacklist_options(&mut options, no_blacklist, no_user_blacklist, blacklist_file);
    options
}
