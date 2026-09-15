//! URL watch support: content fingerprint polling and persisted watch state.

use web2md::Browser;
use web2md::ConvertOptions;
use web2md::PageToMarkdown;
use web2md::content_fingerprint;
use web2md::extract_page_metadata;

/// Fetch a URL once and return its content fingerprint plus the
/// plain-text body (used by the `watch` subcommand).
pub(crate) async fn poll_once(browser: &Browser, url: &str) -> anyhow::Result<(String, String)> {
    let html = browser.fetch(url).await?;
    let html = browser.prepare_html(&html, url).await?;
    let convert_opts = ConvertOptions::default();
    let md = PageToMarkdown::convert_with(&html, &convert_opts, &[])?;
    let meta = extract_page_metadata(&html, &md);
    let body = PageToMarkdown::to_plain_text(&md);
    let fp = meta.fingerprint.unwrap_or_else(|| content_fingerprint(&body));
    Ok((fp, body))
}

pub(crate) fn unix_secs_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

pub(crate) fn watch_state_filename(url: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(url.as_bytes());
    let digest = hasher.finalize();
    let name = digest
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    format!("watch-{}.txt", &name[..16])
}

pub(crate) fn load_watch_state(dir: Option<&std::path::Path>, url: &str) -> anyhow::Result<Option<String>> {
    let Some(dir) = dir else { return Ok(None) };
    let path = dir.join(watch_state_filename(url));
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(&path)?;
    Ok(Some(data.trim().to_string()))
}

pub(crate) fn save_watch_state(
    dir: Option<&std::path::Path>,
    url: &str,
    fingerprint: &str,
) -> anyhow::Result<()> {
    let Some(dir) = dir else { return Ok(()) };
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }
    let path = dir.join(watch_state_filename(url));
    std::fs::write(path, fingerprint.as_bytes())?;
    Ok(())
}
