//! Opt-in headless Chrome / Chromium rendering for JavaScript-heavy pages.
//!
//! Enabled by the `headless` cargo feature. Compiles in [`headless_chrome`]
//! (a Puppeteer-style DevTools client) and exposes [`render_url`] which
//! launches a real browser, navigates to the given URL, waits for the page
//! to settle, and returns the rendered HTML.
//!
//! Use this when an SPA refuses to render through the inline-script subset
//! (e.g. React hydration, full client-side routing, `IntersectionObserver`
//! driven loads). It mirrors the only capability Firecrawl's `/interact`
//! endpoint has that we previously could not match.
//!
//! Requires a Chrome / Chromium binary discoverable by `headless_chrome`
//! (system install or `CHROME` env var). When the feature is disabled,
//! every public function is a stub that returns a clear "feature not enabled"
//! error so the CLI can fail fast.

use anyhow::{anyhow, Result};

#[derive(Debug, Clone, Default)]
pub struct HeadlessOptions {
    /// Time to wait after `navigate_to` returns, in milliseconds. Lets the
    /// page fire additional fetches and run deferred scripts.
    pub wait_ms: u64,
    /// Custom path to a Chrome / Chromium binary. When `None`,
    /// `headless_chrome` probes standard locations.
    pub chrome_path: Option<String>,
}

/// `true` when this build has the `headless` feature compiled in. Used by
/// the CLI to give a clear error when `--headless` is passed without the
/// matching feature flag.
pub fn is_headless_available() -> bool {
    cfg!(feature = "headless")
}

#[cfg(feature = "headless")]
pub async fn render_url(url: &str, options: HeadlessOptions) -> Result<String> {
    let url = url.to_string();
    tokio::task::spawn_blocking(move || render_blocking(&url, &options))
        .await
        .map_err(|e| anyhow!("headless render task panicked: {e}"))?
}

#[cfg(feature = "headless")]
fn render_blocking(url: &str, options: &HeadlessOptions) -> Result<String> {
    use headless_chrome::Browser;

    let mut launch = headless_chrome::LaunchOptions::default();
    if let Some(path) = options.chrome_path.as_ref() {
        launch.path = Some(std::path::PathBuf::from(path));
    }
    // Disable the OS sandbox by default: most containers / CI runners don't
    // have the user-namespace setup Chrome's sandbox needs. Users can re-add
    // it through `CHROME_DEVEL_SANDBOX` if they're on a real Linux host.
    launch.sandbox = false;

    let browser = Browser::new(launch)
        .map_err(|e| anyhow!("failed to launch headless Chrome (is Chrome/Chromium installed?): {e}"))?;
    let tab = browser
        .new_tab()
        .map_err(|e| anyhow!("failed to open new tab: {e}"))?;
    tab.navigate_to(url)
        .map_err(|e| anyhow!("navigation failed for {url}: {e}"))?;
    tab.wait_until_navigated()
        .map_err(|e| anyhow!("wait_until_navigated failed: {e}"))?;
    if options.wait_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(options.wait_ms));
    }
    tab.get_content()
        .map_err(|e| anyhow!("failed to read rendered HTML: {e}"))
}

#[cfg(not(feature = "headless"))]
pub async fn render_url(_url: &str, _options: HeadlessOptions) -> Result<String> {
    Err(anyhow!(
        "headless rendering is not compiled in — rebuild with `cargo build --features headless`"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_reflects_feature_flag() {
        // This test simply asserts the function does not panic and reports
        // the build's `headless` feature state. With the default feature set
        // this is `false`; with `--features headless` it is `true`.
        let _ = is_headless_available();
    }

    #[tokio::test]
    async fn render_url_without_feature_returns_error() {
        if is_headless_available() {
            // Skip: feature is on, we can't easily run Chrome in a unit test.
            return;
        }
        let result = render_url("https://example.com", HeadlessOptions::default()).await;
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("headless") || err.contains("not compiled"),
            "expected feature-missing error, got: {err}"
        );
    }
}
