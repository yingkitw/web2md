use anyhow::Result;
use std::time::Duration;

mod browser;
mod markdown;
mod mcp;

pub use browser::{extract_feed_links, parse_sitemap_urls, Browser, BrowserOptions};
pub use markdown::PageToMarkdown;
pub use mcp::{extract_metadata, McpRequest, McpResponse, McpServer, PageMetadata};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_USER_AGENT: &str = concat!(
    "Web2MD/",
    env!("CARGO_PKG_VERSION"),
    " (Web to Markdown Converter; +https://github.com/yingkitw/web2md)"
);

/// Initialize global defaults.
pub fn init() -> Result<()> {
    Ok(())
}
