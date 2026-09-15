//! CLI surface: argument definitions (clap), output formats, and small output helpers.

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

use web2md::PageMetadata;

/// Output format for the fetch command
#[derive(Clone, Debug, ValueEnum)]
pub(crate) enum OutputFormat {
    /// Convert HTML to clean Markdown (default)
    Markdown,
    /// Emit raw HTML without conversion
    Html,
    /// Emit structured JSON with markdown and metadata
    Json,
    /// Emit plain text with Markdown syntax stripped (archival / NLP pipelines)
    Text,
    /// Emit CSV (url + metadata + plain text) for corpus pipelines
    Csv,
    /// Emit XML-TEI document (teiHeader + body) for corpus pipelines
    Tei,
    /// Emit plain Trafilatura-style XML (`<doc>` + `<main>`) for corpus pipelines
    Xml,
    /// Emit deterministic brand/design profile (≈ Firecrawl `branding` format)
    Branding,
    /// Emit all links as JSON (≈ Firecrawl `links` format)
    Links,
    /// Emit all images as JSON (≈ Firecrawl `images` format)
    Images,
    /// Emit structured product from JSON-LD (≈ Firecrawl `product` format, deterministic)
    Product,
    /// Emit all videos as JSON (≈ Firecrawl `video` format, deterministic)
    Video,
    /// Emit all audio clips as JSON (≈ Firecrawl `audio` format, deterministic)
    Audio,
    /// Emit HTML attribute values for selector:attribute pairs (≈ Firecrawl `attributes`)
    Attributes,
    /// Emit structured restaurant menu from JSON-LD Menu (≈ Firecrawl `menu`, deterministic)
    Menu,
}

/// Structured JSON output for `--format json` CLI flag.
#[derive(Debug, Serialize)]
pub(crate) struct CliJsonOutput {
    pub(crate) markdown: String,
    #[serde(flatten)]
    pub(crate) meta: PageMetadata,
}

#[derive(Parser)]
#[command(name = "web2md")]
#[command(about = "Fetch web pages and convert them to Markdown")]
#[command(arg_required_else_help = false)]
pub(crate) struct Cli {
    /// URL to browse (defaults to interactive browse mode if no subcommand given)
    pub(crate) url: Option<String>,
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

/// Arguments for `web2md fetch`.
#[derive(clap::Args)]
pub(crate) struct FetchArgs {
    /// Target URL
    pub(crate) url: String,
    /// Maximum output length
    #[arg(short, long)]
    pub(crate) max_length: Option<usize>,
    /// Request timeout in seconds
    #[arg(short, long)]
    pub(crate) timeout: Option<u64>,
    /// Include image references in Markdown output
    #[arg(short, long)]
    pub(crate) include_images: bool,
    /// Cookie to send with the request (format: name=value); can be given multiple times
    #[arg(short, long)]
    pub(crate) cookie: Vec<String>,
    /// Custom HTTP header (format: "Name: Value"); can be given multiple times
    #[arg(short = 'H', long)]
    pub(crate) header: Vec<String>,
    /// Output format: markdown, html, json, text, csv, tei, or xml
    #[arg(short, long, value_enum, default_value = "markdown")]
    pub(crate) format: OutputFormat,
    /// Render Markdown with ANSI colors and formatting in the terminal
    #[arg(short, long)]
    pub(crate) render: bool,
    /// Require page language to match this ISO 639-1 or 639-3 code (e.g. en, eng)
    #[arg(long)]
    pub(crate) lang: Option<String>,
    /// Favor precision: less noise, stricter main-content selection
    #[arg(long, conflicts_with = "recall")]
    pub(crate) precision: bool,
    /// Favor recall: more text, looser main-content selection
    #[arg(long, conflicts_with = "precision")]
    pub(crate) recall: bool,
    /// Skip forum/thread comment extraction
    #[arg(long)]
    pub(crate) no_comments: bool,
    /// Strip HTML tables from output
    #[arg(long)]
    pub(crate) no_tables: bool,
    /// Emit link text only (strip Markdown `[text](url)` hrefs)
    #[arg(long)]
    pub(crate) no_links: bool,
    /// Only output when title and published_date metadata are present
    #[arg(long)]
    pub(crate) only_with_metadata: bool,
    /// Polite delay between consecutive requests in milliseconds
    #[arg(long)]
    pub(crate) delay: Option<u64>,
    /// Keep <header> tags in output (stripped by default)
    #[arg(long)]
    pub(crate) keep_header: bool,
    /// Cache TTL in seconds (0 = disabled, default: 0)
    #[arg(long)]
    pub(crate) cache_ttl: Option<u64>,
    /// Bypass cache for this request (always fetch live URL)
    #[arg(long)]
    pub(crate) no_cache: bool,
    /// Only use cache entries younger than N seconds (overrides --cache-ttl for lookup)
    #[arg(long)]
    pub(crate) cache_max_age: Option<u64>,
    /// Extract only main content from <article>, <main>, or [role=main] elements
    #[arg(long)]
    pub(crate) main_content: bool,
    /// Write output to file instead of stdout (or directory when --depth > 0)
    #[arg(short, long)]
    pub(crate) output: Option<String>,
    /// Prepend YAML frontmatter (metadata) to Markdown output
    #[arg(long)]
    pub(crate) frontmatter: bool,
    /// CSS-like selector to exclude HTML elements (e.g. `.ad`, `#sidebar`); can be given multiple times
    #[arg(long)]
    pub(crate) exclude_selector: Vec<String>,
    /// Disable URL blacklist filtering for ads/tracking pixels
    #[arg(long)]
    pub(crate) no_blacklist: bool,
    /// Recursively crawl same-origin links up to N levels deep (markdown output only)
    #[arg(long, default_value = "0")]
    pub(crate) depth: u32,
    /// Use sitemap.xml URLs only (no page link following); requires --depth > 0
    #[arg(long)]
    pub(crate) sitemap_only: bool,
    /// Ignore robots.txt disallow rules and crawl-delay
    #[arg(long)]
    pub(crate) ignore_robots: bool,
    /// Additional blacklist pattern file (one host or path pattern per line)
    #[arg(long)]
    pub(crate) blacklist_file: Vec<String>,
    /// Do not load ~/.web2md/blacklist.txt
    #[arg(long)]
    pub(crate) no_user_blacklist: bool,
    /// Keep only paragraphs relevant to this natural-language query (≈ Firecrawl `highlights`)
    #[arg(long)]
    pub(crate) topic: Option<String>,
    /// Cap output length by token budget instead of characters
    #[arg(long)]
    pub(crate) max_tokens: Option<usize>,
    /// Extract an extactive summary of this many sentences (≈ Firecrawl `summary`)
    #[arg(long)]
    pub(crate) summary: Option<usize>,
    /// Force structured extractor: `recipe` / `faq` / `job` / `event` (LLM-free)
    #[arg(long = "type")]
    pub(crate) r#type: Option<String>,
    /// Persist fetched pages as JSON files under this directory; survives restarts
    #[arg(long)]
    pub(crate) cache_dir: Option<String>,
    /// Per-host requests-per-second cap; smaller = more polite
    #[arg(long)]
    pub(crate) rate: Option<f64>,
    /// POST the result JSON to this webhook URL when fetch completes (n8n/Make/Zapier)
    #[arg(long)]
    pub(crate) webhook: Option<String>,
    /// CSS-like selector to keep only matching HTML elements (e.g. `article`, `.content`); can be given multiple times
    #[arg(long)]
    pub(crate) include_selector: Vec<String>,
    /// For `--format attributes`: `selector:attribute` pairs (e.g. `a:href`, `img:src`); repeatable
    #[arg(long)]
    pub(crate) attr: Vec<String>,
    /// Redact PII (emails, phone numbers, SSNs, credit cards) from output
    #[arg(long)]
    pub(crate) pii_redact: bool,
    /// Use a mobile User-Agent string for the request
    #[arg(long)]
    pub(crate) mobile: bool,
    /// Route requests through an HTTP/SOCKS proxy (e.g. "http://proxy:8080", "socks5://proxy:1080")
    #[arg(long)]
    pub(crate) proxy: Option<String>,
    /// Basic authentication credentials (format: "user:password")
    #[arg(long)]
    pub(crate) auth: Option<String>,
    /// Apply Mozilla Readability.js (via readabilityrs) before conversion
    /// to strip chrome and isolate the article body.
    /// Requires building with --features readability.
    #[arg(long)]
    #[cfg(feature = "readability")]
    pub(crate) readability: bool,
    /// Render the page with a real headless browser before conversion.
    /// Requires building with `--features headless` and a Chrome/Chromium
    /// binary at runtime. Use for JS-heavy SPAs the inline-script
    /// interpreter cannot drive.
    #[arg(long)]
    pub(crate) headless: bool,
    /// Path to a Chrome / Chromium binary (used with --headless;
    /// defaults to the system install).
    #[arg(long, requires = "headless")]
    pub(crate) chrome_path: Option<String>,
    /// Append a deduplicated list of all links at the end of Markdown output
    #[arg(long)]
    pub(crate) links_summary: bool,
    /// Append a deduplicated list of all images at the end of Markdown output
    #[arg(long)]
    pub(crate) images_summary: bool,
    /// Split Markdown by headings for RAG pipelines (sections separated by `\n\n---\n\n`)
    #[arg(long)]
    pub(crate) chunk: bool,
}

/// Arguments for `web2md browse`.
#[derive(clap::Args)]
pub(crate) struct BrowseArgs {
    /// Starting URL
    pub(crate) url: String,
    /// Request timeout in seconds
    #[arg(short, long)]
    pub(crate) timeout: Option<u64>,
    /// Include image references in Markdown output
    #[arg(short, long)]
    pub(crate) include_images: bool,
    /// Cookie to send with the request (format: name=value); can be given multiple times
    #[arg(short, long)]
    pub(crate) cookie: Vec<String>,
    /// Custom HTTP header (format: "Name: Value"); can be given multiple times
    #[arg(short = 'H', long)]
    pub(crate) header: Vec<String>,
    /// Polite delay between consecutive requests in milliseconds
    #[arg(long)]
    pub(crate) delay: Option<u64>,
    /// Keep <header> tags in output (stripped by default)
    #[arg(long)]
    pub(crate) keep_header: bool,
    /// Cache TTL in seconds (0 = disabled, default: 0)
    #[arg(long)]
    pub(crate) cache_ttl: Option<u64>,
    /// Bypass cache for this request (always fetch live URL)
    #[arg(long)]
    pub(crate) no_cache: bool,
    /// Only use cache entries younger than N seconds (overrides --cache-ttl for lookup)
    #[arg(long)]
    pub(crate) cache_max_age: Option<u64>,
    /// Extract only main content from <article>, <main>, or [role=main] elements
    #[arg(long)]
    pub(crate) main_content: bool,
    /// Disable URL blacklist filtering for ads/tracking pixels
    #[arg(long)]
    pub(crate) no_blacklist: bool,
    /// Ignore robots.txt disallow rules and crawl-delay
    #[arg(long)]
    pub(crate) ignore_robots: bool,
    /// Additional blacklist pattern file (one host or path pattern per line)
    #[arg(long)]
    pub(crate) blacklist_file: Vec<String>,
    /// Do not load ~/.web2md/blacklist.txt
    #[arg(long)]
    pub(crate) no_user_blacklist: bool,
}

/// Arguments for `web2md batch`.
#[derive(clap::Args)]
pub(crate) struct BatchArgs {
    /// File containing one URL per line (lines starting with # are ignored)
    pub(crate) file: String,
    /// Request timeout in seconds
    #[arg(short, long)]
    pub(crate) timeout: Option<u64>,
    /// Include image references in Markdown output
    #[arg(short, long)]
    pub(crate) include_images: bool,
    /// Cookie to send with the request (format: name=value); can be given multiple times
    #[arg(short, long)]
    pub(crate) cookie: Vec<String>,
    /// Custom HTTP header (format: "Name: Value"); can be given multiple times
    #[arg(short = 'H', long)]
    pub(crate) header: Vec<String>,
    /// Polite delay between consecutive requests in milliseconds
    #[arg(long)]
    pub(crate) delay: Option<u64>,
    /// Keep <header> tags in output (stripped by default)
    #[arg(long)]
    pub(crate) keep_header: bool,
    /// Cache TTL in seconds (0 = disabled, default: 0)
    #[arg(long)]
    pub(crate) cache_ttl: Option<u64>,
    /// Bypass cache for this request (always fetch live URL)
    #[arg(long)]
    pub(crate) no_cache: bool,
    /// Only use cache entries younger than N seconds (overrides --cache-ttl for lookup)
    #[arg(long)]
    pub(crate) cache_max_age: Option<u64>,
    /// Extract only main content from <article>, <main>, or [role=main] elements
    #[arg(long)]
    pub(crate) main_content: bool,
    /// Output directory to write Markdown files (default: stdout)
    #[arg(short, long)]
    pub(crate) output: Option<String>,
    /// Prepend YAML frontmatter (metadata) to each Markdown output
    #[arg(long)]
    pub(crate) frontmatter: bool,
    /// CSS-like selector to exclude HTML elements (e.g. `.ad`, `#sidebar`); can be given multiple times
    #[arg(long)]
    pub(crate) exclude_selector: Vec<String>,
    /// Disable URL blacklist filtering for ads/tracking pixels
    #[arg(long)]
    pub(crate) no_blacklist: bool,
    /// Ignore robots.txt disallow rules and crawl-delay
    #[arg(long)]
    pub(crate) ignore_robots: bool,
    /// Additional blacklist pattern file (one host or path pattern per line)
    #[arg(long)]
    pub(crate) blacklist_file: Vec<String>,
    /// Do not load ~/.web2md/blacklist.txt
    #[arg(long)]
    pub(crate) no_user_blacklist: bool,
    /// Route requests through an HTTP/SOCKS proxy (e.g. "http://proxy:8080", "socks5://proxy:1080")
    #[arg(long)]
    pub(crate) proxy: Option<String>,
    /// Basic authentication credentials (format: "user:password")
    #[arg(long)]
    pub(crate) auth: Option<String>,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Commands {
    /// Fetch a single URL and print Markdown to stdout
    Fetch(FetchArgs),
    /// Peek at a URL: return title + excerpt + key metadata only (cheaper than `fetch`)
    Peek {
        /// Target URL
        url: String,
        /// Request timeout in seconds
        #[arg(short, long)]
        timeout: Option<u64>,
        /// Cookie to send with the request (format: name=value); can be given multiple times
        #[arg(short, long)]
        cookie: Vec<String>,
        /// Custom HTTP header (format: "Name: Value"); can be given multiple times
        #[arg(short = 'H', long)]
        header: Vec<String>,
        /// Polite delay between consecutive requests in milliseconds
        #[arg(long)]
        delay: Option<u64>,
        /// Ignore robots.txt disallow rules and crawl-delay
        #[arg(long)]
        ignore_robots: bool,
        /// Disable URL blacklist filtering
        #[arg(long)]
        no_blacklist: bool,
        /// Output as structured JSON instead of plain text
        #[arg(long)]
        json: bool,
        /// Route requests through an HTTP/SOCKS proxy (e.g. "http://proxy:8080", "socks5://proxy:1080")
        #[arg(long)]
        proxy: Option<String>,
        /// Basic authentication credentials (format: "user:password")
        #[arg(long)]
        auth: Option<String>,
    },
    /// Interactive terminal browser (Lynx-like)
    Browse(BrowseArgs),
    /// Run as an MCP server (stdio JSON-RPC)
    Mcp,
    /// Diff two URLs (or URL vs. cached version) at the Markdown level
    Diff {
        /// First URL (or path to a cached Markdown file when using --cached-b)
        url_a: String,
        /// Second URL (or path to a cached Markdown file)
        url_b: String,
        /// Treat `url_b` as a path to a local Markdown file (skip fetch)
        #[arg(long)]
        cached_b: bool,
        /// Request timeout in seconds
        #[arg(short, long)]
        timeout: Option<u64>,
        /// Cookie to send with the request (format: name=value); can be given multiple times
        #[arg(short, long)]
        cookie: Vec<String>,
        /// Custom HTTP header (format: "Name: Value"); can be given multiple times
        #[arg(short = 'H', long)]
        header: Vec<String>,
        /// Emit machine-readable JSON summary instead of unified diff
        #[arg(long)]
        json: bool,
    },
    /// Poll a URL on an interval and emit whenever the content fingerprint (simhash) changes
    Watch {
        /// Target URL
        url: String,
        /// Poll interval in seconds (default: 300)
        #[arg(long, default_value = "300")]
        every: u64,
        /// Request timeout in seconds
        #[arg(short, long)]
        timeout: Option<u64>,
        /// Cookie to send with the request (format: name=value); can be given multiple times
        #[arg(short, long)]
        cookie: Vec<String>,
        /// Custom HTTP header (format: "Name: Value"); can be given multiple times
        #[arg(short = 'H', long)]
        header: Vec<String>,
        /// Persist seen fingerprints across restarts at this directory
        #[arg(long)]
        cache_dir: Option<String>,
        /// Ignore robots.txt for this fetch
        #[arg(long)]
        ignore_robots: bool,
    },
    /// Discover URLs from a website's sitemap.xml
    Sitemap {
        /// Target URL (sitemap.xml will be fetched from the same origin)
        url: String,
        /// Request timeout in seconds
        #[arg(short, long)]
        timeout: Option<u64>,
        /// Cookie to send with the request (format: name=value); can be given multiple times
        #[arg(short, long)]
        cookie: Vec<String>,
        /// Custom HTTP header (format: "Name: Value"); can be given multiple times
        #[arg(short = 'H', long)]
        header: Vec<String>,
    },
    /// Discover all URLs on a page by extracting <a href> links (≈ Firecrawl /map)
    Map {
        /// Target URL
        url: String,
        /// Request timeout in seconds
        #[arg(short, long)]
        timeout: Option<u64>,
        /// Cookie to send with the request (format: name=value); can be given multiple times
        #[arg(short, long)]
        cookie: Vec<String>,
        /// Custom HTTP header (format: "Name: Value"); can be given multiple times
        #[arg(short = 'H', long)]
        header: Vec<String>,
        /// Only list URLs on the same origin as the target
        #[arg(long)]
        same_origin: bool,
        /// Output as JSON array instead of one URL per line
        #[arg(long)]
        json: bool,
        /// Ignore robots.txt disallow rules and crawl-delay
        #[arg(long)]
        ignore_robots: bool,
    },
    /// Web search via DuckDuckGo (no API key required; ≈ Firecrawl /search, free)
    Search {
        /// Search query
        query: String,
        /// Request timeout in seconds
        #[arg(short, long)]
        timeout: Option<u64>,
        /// Maximum number of results to return
        #[arg(short, long)]
        limit: Option<usize>,
        /// Output as JSON instead of Markdown
        #[arg(long)]
        json: bool,
        /// Fetch and convert each result URL to Markdown (uses --limit to cap fetches)
        #[arg(long)]
        fetch: bool,
        /// Only include results from these domains (e.g. "github.com" "rust-lang.org")
        #[arg(long)]
        include_domains: Vec<String>,
        /// Exclude results from these domains
        #[arg(long)]
        exclude_domains: Vec<String>,
        /// Cookie to send with the request (format: name=value); can be given multiple times
        #[arg(short, long)]
        cookie: Vec<String>,
        /// Custom HTTP header (format: "Name: Value"); can be given multiple times
        #[arg(short = 'H', long)]
        header: Vec<String>,
    },
    /// Batch convert multiple URLs to Markdown from a file
    Batch(BatchArgs),
    /// Build a BM25 index over a directory of Markdown files, or query one
    /// (≈ poor-person's Context7 over any local corpus, no API key)
    Corpus {
        #[command(subcommand)]
        action: CorpusAction,
    },
    /// Fetch README + metadata from crates.io, npm, or PyPI (≈ Context7, no API key)
    Docs {
        /// Package name to look up
        name: String,
        /// Registry: crates, npm, or pypi (default: crates)
        #[arg(short, long, default_value = "crates")]
        registry: String,
        /// Request timeout in seconds
        #[arg(short, long)]
        timeout: Option<u64>,
        /// Output as structured JSON instead of Markdown
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum CorpusAction {
    /// Build (or rebuild) the index for a directory of Markdown files
    Index {
        /// Directory containing .md / .markdown files (recurses into subdirs)
        dir: String,
        /// Custom output path for the JSON index (default: <dir>/.web2md-index.json)
        #[arg(long)]
        output: Option<String>,
    },
    /// Query the index for a directory and return top-N matching files
    Query {
        /// Directory whose .web2md-index.json should be queried
        dir: String,
        /// Free-form query (e.g. "rust cargo")
        query: String,
        /// Maximum number of matches to return
        #[arg(short, long, default_value = "5")]
        limit: usize,
        /// Emit machine-readable JSON instead of Markdown
        #[arg(long)]
        json: bool,
    },
}

/// Map a CLI `OutputFormat` to a lowercase identifier for use in webhook payloads.
pub(crate) fn format_label(format: &OutputFormat) -> &'static str {
    match format {
        OutputFormat::Markdown => "markdown",
        OutputFormat::Html => "html",
        OutputFormat::Json => "json",
        OutputFormat::Text => "text",
        OutputFormat::Csv => "csv",
        OutputFormat::Tei => "tei",
        OutputFormat::Xml => "xml",
        OutputFormat::Branding => "branding",
        OutputFormat::Links => "links",
        OutputFormat::Images => "images",
        OutputFormat::Product => "product",
        OutputFormat::Video => "video",
        OutputFormat::Audio => "audio",
        OutputFormat::Attributes => "attributes",
        OutputFormat::Menu => "menu",
    }
}

/// Convert a URL to a safe filename for batch output.
/// e.g. "https://example.com/blog/post" → "example.com_blog_post.md"
pub(crate) fn url_to_filename(url: &str) -> String {
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return format!("{}.md", url.replace(['/', ':', '?', '=', '&'], "_")),
    };
    let host = parsed.host_str().unwrap_or("unknown");
    let path = parsed.path().trim_start_matches('/');
    let path = if path.is_empty() { "index" } else { path };
    let path = path.replace(['/', '?', '=', '&'], "_");
    format!("{}_{}.md", host, path)
}
