use std::time::Duration;

mod branding;
mod browser;
mod crawl;
mod diff_markdown;
mod docs;
mod extract;
mod feed;
mod html_meta;
mod html_to_md;
mod html_util;
mod js;
mod markdown;
mod mcp;
mod persistent_cache;
mod redact;
mod robots;
mod search;
mod structured;
mod transform;
mod url_blacklist;
mod youtube;

pub use branding::{extract_branding, BrandingProfile, ColorStat, HeadingSize};
pub use browser::{extract_feed_links, parse_sitemap_urls, Browser, BrowserOptions};
pub use diff_markdown::{diff_markdown, summarize};
pub use docs::{doc_result_to_markdown, parse_crates_io, parse_npm, parse_pypi, registry_url, DocResult, Registry};
pub use persistent_cache::PersistentCache;
pub use youtube::{
    extract_caption_track_url, extract_video_id, is_youtube_url, parse_timed_text,
    render_transcript_markdown, transcript_from_watch_html, TranscriptCue,
};
pub use crawl::{normalize_crawl_url, same_origin_links};
pub use extract::{extract_images, extract_links, extract_product, ImageEntry, LinkEntry, ProductEntry, ProductVariant};
pub use feed::{feed_to_markdown, parse_feed, Feed};
pub use markdown::{ConvertOptions, PageToMarkdown};
pub use mcp::{
    content_fingerprint, detect_content_language, extract_metadata, extract_page_metadata,
    language_matches, truncate_with_marker, McpRequest, McpServer, PageMetadata,
};
pub use structured::{
    extract_event, extract_faq, extract_job, extract_recipe, StructuredError,
};
pub use redact::redact_pii;
pub use search::{ddg_search_url, parse_ddg_results, results_to_markdown, SearchResult};
pub use transform::{extract_summary, extract_topic, truncate_by_tokens};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_USER_AGENT: &str = concat!(
    "Web2MD/",
    env!("CARGO_PKG_VERSION"),
    " (Web to Markdown Converter; +https://github.com/yingkitw/web2md)"
);
