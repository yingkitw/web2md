use anyhow::Result;
use std::time::Duration;

mod browser;
mod markdown;
mod mcp;

pub use browser::{Browser, BrowserOptions};
pub use markdown::PageToMarkdown;
pub use mcp::{McpRequest, McpResponse, McpServer};

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
