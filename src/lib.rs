use std::time::Duration;

mod browser;
mod crawl;
mod feed;
mod html_meta;
mod html_to_md;
mod html_util;
mod js;
mod markdown;
mod mcp;
mod robots;
mod url_blacklist;

pub use browser::{extract_feed_links, parse_sitemap_urls, Browser, BrowserOptions};
pub use crawl::{normalize_crawl_url, same_origin_links};
pub use feed::{feed_to_markdown, parse_feed, Feed};
pub use markdown::PageToMarkdown;
pub use mcp::{
    content_fingerprint, detect_content_language, extract_metadata, extract_page_metadata,
    language_matches, truncate_with_marker, McpRequest, McpServer, PageMetadata,
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_USER_AGENT: &str = concat!(
    "Web2MD/",
    env!("CARGO_PKG_VERSION"),
    " (Web to Markdown Converter; +https://github.com/yingkitw/web2md)"
);
