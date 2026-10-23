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

fn watch_id(url: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(url.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    hex[..16].to_string()
}

pub(crate) fn watch_state_filename(url: &str) -> String {
    format!("watch-{}.txt", watch_id(url))
}

pub(crate) fn watch_body_filename(url: &str) -> String {
    format!("watch-{}.body.txt", watch_id(url))
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

pub(crate) fn load_watch_body(dir: Option<&std::path::Path>, url: &str) -> anyhow::Result<Option<String>> {
    let Some(dir) = dir else { return Ok(None) };
    let path = dir.join(watch_body_filename(url));
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(std::fs::read_to_string(&path)?))
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

pub(crate) fn save_watch_body(
    dir: Option<&std::path::Path>,
    url: &str,
    body: &str,
) -> anyhow::Result<()> {
    let Some(dir) = dir else { return Ok(()) };
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }
    let path = dir.join(watch_body_filename(url));
    std::fs::write(path, body.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_body_round_trips_alongside_fingerprint() {
        let url = "https://example.test/watch-body";
        let dir = std::env::temp_dir().join(format!(
            "web2md-watch-body-{}-{}",
            std::process::id(),
            unix_secs_string()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        save_watch_state(Some(&dir), url, "fp1").unwrap();
        save_watch_body(Some(&dir), url, "line one\nline two\n").unwrap();

        assert_eq!(
            load_watch_state(Some(&dir), url).unwrap().as_deref(),
            Some("fp1")
        );
        assert_eq!(
            load_watch_body(Some(&dir), url).unwrap().as_deref(),
            Some("line one\nline two\n")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
