//! Optional Mozilla Readability.js extraction pass.
//!
//! When the user passes `--readability` on `fetch`, the raw HTML is first
//! scored and cleaned by [`readabilityrs`] — a Rust port of Mozilla's
//! Readability library (the same algorithm behind Firefox Reader View).
//! The article HTML it returns replaces the original body before our
//! usual pipeline runs, so noise, nav, ads, and chrome are stripped
//! deterministically by a battle-tested extractor.
//!
//! Falls back to the input HTML when Readability declines to score the
//! page (e.g. it's not an article-shaped document).

use anyhow::Result;

/// Run the Mozilla Readability extractor over `html` and return the
/// cleaned article HTML. When Readability declines to score the document
/// (returns `None`) or fails to construct (bad URL), the input HTML is
/// returned unchanged so the rest of the pipeline can still try.
pub fn apply_readability(html: &str, page_url: Option<&str>) -> Result<String> {
    let opts = readabilityrs::ReadabilityOptions::builder()
        .char_threshold(300)
        .keep_classes(false)
        .build();
    let result = match page_url {
        Some(u) => readabilityrs::Readability::new(html, Some(u), Some(opts)),
        None => readabilityrs::Readability::new(html, None, Some(opts)),
    };
    let mut readability = match result {
        Ok(r) => r,
        Err(_) => return Ok(html.to_string()),
    };
    let _ = &mut readability;
    let Some(article) = readability.parse() else {
        return Ok(html.to_string());
    };
    Ok(article.content.unwrap_or_default())
}

/// Lightweight pre-flight check: returns true when Readability thinks the
/// page is likely an article worth extracting. Useful for logging in CLI
/// and for short-circuiting `--main-content` vs Readability choices.
pub fn is_readerable(html: &str) -> bool {
    readabilityrs::is_probably_readerable(html, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTICLE: &str = r#"<!doctype html>
<html>
<head><title>Real News</title><meta name="author" content="Jane Doe"></head>
<body>
  <nav><ul><li><a href="/">Home</a></li></ul></nav>
  <aside class="ad">Buy stuff!</aside>
  <article>
    <h1>The Big Story</h1>
    <p>By Jane Doe — this is a long enough paragraph about a meaningful topic
       that should pass the readability threshold. The story continues with
       more detail about the subject and provides enough text for the algorithm
       to recognize this as a genuine article rather than a navigation shell.
       We need to keep writing until we are well over the threshold so the
       extractor is comfortable scoring this block as the primary content.</p>
    <p>A second paragraph adds context, quotes sources, and develops the
       narrative further. Readability uses paragraph count and text length
       together to decide which container holds the actual article body
       versus the surrounding chrome.</p>
    <p>A third paragraph closes the piece with a summary takeaway and
       forward-looking statement. The article length is now well above the
       300-character default threshold used by Readability.js.</p>
  </article>
  <footer>Copyright 2026 — Site Name</footer>
</body>
</html>"#;

    #[test]
    fn apply_readability_strips_chrome() {
        let out = apply_readability(ARTICLE, Some("https://example.com/article")).unwrap();
        // Article content survives
        assert!(out.contains("The Big Story"));
        assert!(out.to_lowercase().contains("jane doe"));
        // Chrome is dropped
        assert!(!out.contains("Buy stuff"));
        assert!(!out.contains("Copyright 2026"));
    }

    #[test]
    fn apply_readability_passthrough_when_unsuitable() {
        let shell = "<html><body><h1>Hi</h1><p>tiny</p></body></html>";
        // Too short to be a real article; should pass through unchanged.
        let out = apply_readability(shell, Some("https://example.com/")).unwrap();
        assert!(out.contains("Hi"));
    }

    #[test]
    fn is_readerable_agrees_with_article() {
        assert!(is_readerable(ARTICLE));
        assert!(!is_readerable("<html><body><h1>Hi</h1></body></html>"));
    }

    #[test]
    fn apply_readability_handles_no_url() {
        let out = apply_readability(ARTICLE, None).unwrap();
        assert!(out.contains("The Big Story"));
    }
}
