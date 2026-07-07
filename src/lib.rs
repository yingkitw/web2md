use anyhow::Result;
use std::time::Duration;

mod browser;
mod crawl;
mod js;
mod markdown;
mod mcp;
mod url_blacklist;

pub use browser::{extract_feed_links, parse_sitemap_urls, Browser, BrowserOptions};
pub use crawl::{extract_page_links, is_same_origin, normalize_crawl_url, same_origin_links};
pub use url_blacklist::{filter_blacklisted_urls, is_blacklisted};
pub use js::{inject_before_body_close, run_inline_scripts, Interpreter};
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
