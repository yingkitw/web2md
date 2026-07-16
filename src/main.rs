use anyhow::{Context, Result};
use url::Url;
use web2md::{
    content_fingerprint, extract_event, extract_faq, extract_feed_links, extract_job,
    extract_page_metadata, extract_recipe, extract_summary, extract_topic, feed_to_markdown,
    language_matches, normalize_crawl_url, parse_feed, parse_sitemap_urls,
    truncate_by_tokens, truncate_with_marker, Browser, BrowserOptions, ConvertOptions,
    McpRequest, McpServer, PageMetadata, PageToMarkdown,
};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::io::{self, BufRead, Write};
use std::time::Duration;

/// Output format for the fetch command
#[derive(Clone, Debug, ValueEnum)]
enum OutputFormat {
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
}

/// Structured JSON output for `--format json` CLI flag.
#[derive(Debug, Serialize)]
struct CliJsonOutput {
    markdown: String,
    #[serde(flatten)]
    meta: PageMetadata,
}

#[derive(Parser)]
#[command(name = "web2md")]
#[command(about = "Fetch web pages and convert them to Markdown")]
#[command(arg_required_else_help = false)]
struct Cli {
    /// URL to browse (defaults to interactive browse mode if no subcommand given)
    url: Option<String>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch a single URL and print Markdown to stdout
    Fetch {
        /// Target URL
        url: String,
        /// Maximum output length
        #[arg(short, long)]
        max_length: Option<usize>,
        /// Request timeout in seconds
        #[arg(short, long)]
        timeout: Option<u64>,
        /// Include image references in Markdown output
        #[arg(short, long)]
        include_images: bool,
        /// Cookie to send with the request (format: name=value); can be given multiple times
        #[arg(short, long)]
        cookie: Vec<String>,
        /// Custom HTTP header (format: "Name: Value"); can be given multiple times
        #[arg(short = 'H', long)]
        header: Vec<String>,
        /// Output format: markdown, html, json, text, csv, tei, or xml
        #[arg(short, long, value_enum, default_value = "markdown")]
        format: OutputFormat,
        /// Render Markdown with ANSI colors and formatting in the terminal
        #[arg(short, long)]
        render: bool,
        /// Require page language to match this ISO 639-1 or 639-3 code (e.g. en, eng)
        #[arg(long)]
        lang: Option<String>,
        /// Favor precision: less noise, stricter main-content selection
        #[arg(long, conflicts_with = "recall")]
        precision: bool,
        /// Favor recall: more text, looser main-content selection
        #[arg(long, conflicts_with = "precision")]
        recall: bool,
        /// Skip forum/thread comment extraction
        #[arg(long)]
        no_comments: bool,
        /// Strip HTML tables from output
        #[arg(long)]
        no_tables: bool,
        /// Emit link text only (strip Markdown `[text](url)` hrefs)
        #[arg(long)]
        no_links: bool,
        /// Only output when title and published_date metadata are present
        #[arg(long)]
        only_with_metadata: bool,
        /// Polite delay between consecutive requests in milliseconds
        #[arg(long)]
        delay: Option<u64>,
        /// Keep <header> tags in output (stripped by default)
        #[arg(long)]
        keep_header: bool,
        /// Cache TTL in seconds (0 = disabled, default: 0)
        #[arg(long)]
        cache_ttl: Option<u64>,
        /// Extract only main content from <article>, <main>, or [role=main] elements
        #[arg(long)]
        main_content: bool,
        /// Write output to file instead of stdout (or directory when --depth > 0)
        #[arg(short, long)]
        output: Option<String>,
        /// Prepend YAML frontmatter (metadata) to Markdown output
        #[arg(long)]
        frontmatter: bool,
        /// CSS-like selector to exclude HTML elements (e.g. `.ad`, `#sidebar`); can be given multiple times
        #[arg(long)]
        exclude_selector: Vec<String>,
        /// Execute inline <script> blocks via the built-in JS interpreter and fold document.write output into the page
        #[arg(long)]
        javascript: bool,
        /// Post-load wait in milliseconds before processing (also caps timer callback delay)
        #[arg(long)]
        wait: Option<u64>,
        /// Disable URL blacklist filtering for ads/tracking pixels
        #[arg(long)]
        no_blacklist: bool,
        /// Recursively crawl same-origin links up to N levels deep (markdown output only)
        #[arg(long, default_value = "0")]
        depth: u32,
        /// Ignore robots.txt disallow rules and crawl-delay
        #[arg(long)]
        ignore_robots: bool,
        /// Additional blacklist pattern file (one host or path pattern per line)
        #[arg(long)]
        blacklist_file: Vec<String>,
        /// Do not load ~/.web2md/blacklist.txt
        #[arg(long)]
        no_user_blacklist: bool,
        /// Keep only paragraphs relevant to this natural-language query (≈ Firecrawl `highlights`)
        #[arg(long)]
        topic: Option<String>,
        /// Cap output length by token budget instead of characters
        #[arg(long)]
        max_tokens: Option<usize>,
        /// Extract an extactive summary of this many sentences (≈ Firecrawl `summary`)
        #[arg(long)]
        summary: Option<usize>,
        /// Force structured extractor: `recipe` / `faq` / `job` / `event` (LLM-free)
        #[arg(long = "type")]
        r#type: Option<String>,
        /// Persist fetched pages as JSON files under this directory; survives restarts
        #[arg(long)]
        cache_dir: Option<String>,
        /// Per-host requests-per-second cap; smaller = more polite
        #[arg(long)]
        rate: Option<f64>,
        /// POST the result JSON to this webhook URL when fetch completes (n8n/Make/Zapier)
        #[arg(long)]
        webhook: Option<String>,
    },
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
    },
    /// Interactive terminal browser (Lynx-like)
    Browse {
        /// Starting URL
        url: String,
        /// Request timeout in seconds
        #[arg(short, long)]
        timeout: Option<u64>,
        /// Include image references in Markdown output
        #[arg(short, long)]
        include_images: bool,
        /// Cookie to send with the request (format: name=value); can be given multiple times
        #[arg(short, long)]
        cookie: Vec<String>,
        /// Custom HTTP header (format: "Name: Value"); can be given multiple times
        #[arg(short = 'H', long)]
        header: Vec<String>,
        /// Polite delay between consecutive requests in milliseconds
        #[arg(long)]
        delay: Option<u64>,
        /// Keep <header> tags in output (stripped by default)
        #[arg(long)]
        keep_header: bool,
        /// Cache TTL in seconds (0 = disabled, default: 0)
        #[arg(long)]
        cache_ttl: Option<u64>,
        /// Extract only main content from <article>, <main>, or [role=main] elements
        #[arg(long)]
        main_content: bool,
        /// Execute inline <script> blocks via the built-in JS interpreter and fold document.write output into the page
        #[arg(long)]
        javascript: bool,
        /// Post-load wait in milliseconds before processing (also caps timer callback delay)
        #[arg(long)]
        wait: Option<u64>,
        /// Disable URL blacklist filtering for ads/tracking pixels
        #[arg(long)]
        no_blacklist: bool,
        /// Ignore robots.txt disallow rules and crawl-delay
        #[arg(long)]
        ignore_robots: bool,
        /// Additional blacklist pattern file (one host or path pattern per line)
        #[arg(long)]
        blacklist_file: Vec<String>,
        /// Do not load ~/.web2md/blacklist.txt
        #[arg(long)]
        no_user_blacklist: bool,
    },
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
    /// Extract a YouTube video transcript as Markdown (no video download)
    Transcript {
        /// YouTube watch or share URL
        url: String,
        /// Request timeout in seconds
        #[arg(short, long)]
        timeout: Option<u64>,
        /// Preferred caption language code (e.g. en, fr, de)
        #[arg(long)]
        lang: Option<String>,
        /// Cookie to send with the request (format: name=value); can be given multiple times
        #[arg(short, long)]
        cookie: Vec<String>,
        /// Custom HTTP header (format: "Name: Value"); can be given multiple times
        #[arg(short = 'H', long)]
        header: Vec<String>,
        /// Ignore robots.txt for this fetch
        #[arg(long)]
        ignore_robots: bool,
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
    /// Discover URLs from a website's sitemap.xml and RSS/Atom feeds
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
        /// Also check the HTML page for RSS/Atom feed links
        #[arg(long)]
        feeds: bool,
    },
    /// Fetch an RSS, Atom, or JSON Feed and convert entries to Markdown
    Feed {
        /// Feed URL (RSS 2.0, Atom, or JSON Feed)
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
        /// Maximum number of entries to include (default: all)
        #[arg(long)]
        max_entries: Option<usize>,
        /// Emit structured JSON instead of Markdown
        #[arg(long)]
        json: bool,
        /// Output file path (default: stdout)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Batch convert multiple URLs to Markdown from a file
    Batch {
        /// File containing one URL per line (lines starting with # are ignored)
        file: String,
        /// Request timeout in seconds
        #[arg(short, long)]
        timeout: Option<u64>,
        /// Include image references in Markdown output
        #[arg(short, long)]
        include_images: bool,
        /// Cookie to send with the request (format: name=value); can be given multiple times
        #[arg(short, long)]
        cookie: Vec<String>,
        /// Custom HTTP header (format: "Name: Value"); can be given multiple times
        #[arg(short = 'H', long)]
        header: Vec<String>,
        /// Polite delay between consecutive requests in milliseconds
        #[arg(long)]
        delay: Option<u64>,
        /// Keep <header> tags in output (stripped by default)
        #[arg(long)]
        keep_header: bool,
        /// Cache TTL in seconds (0 = disabled, default: 0)
        #[arg(long)]
        cache_ttl: Option<u64>,
        /// Extract only main content from <article>, <main>, or [role=main] elements
        #[arg(long)]
        main_content: bool,
        /// Output directory to write Markdown files (default: stdout)
        #[arg(short, long)]
        output: Option<String>,
        /// Prepend YAML frontmatter (metadata) to each Markdown output
        #[arg(long)]
        frontmatter: bool,
        /// CSS-like selector to exclude HTML elements (e.g. `.ad`, `#sidebar`); can be given multiple times
        #[arg(long)]
        exclude_selector: Vec<String>,
        /// Execute inline <script> blocks via the built-in JS interpreter and fold document.write output into the page
        #[arg(long)]
        javascript: bool,
        /// Post-load wait in milliseconds before processing (also caps timer callback delay)
        #[arg(long)]
        wait: Option<u64>,
        /// Disable URL blacklist filtering for ads/tracking pixels
        #[arg(long)]
        no_blacklist: bool,
        /// Ignore robots.txt disallow rules and crawl-delay
        #[arg(long)]
        ignore_robots: bool,
        /// Additional blacklist pattern file (one host or path pattern per line)
        #[arg(long)]
        blacklist_file: Vec<String>,
        /// Do not load ~/.web2md/blacklist.txt
        #[arg(long)]
        no_user_blacklist: bool,
    },
}

fn apply_blacklist_options(
    options: &mut BrowserOptions,
    no_blacklist: bool,
    no_user_blacklist: bool,
    blacklist_file: Vec<String>,
) {
    options.filter_blacklisted_urls = !no_blacklist;
    options.load_user_blacklist = !no_user_blacklist;
    options.extra_blacklist_files = blacklist_file;
}

/// Fetch a URL once and return its content fingerprint plus the
/// plain-text body (used by the `watch` subcommand).
async fn poll_once(browser: &Browser, url: &str) -> anyhow::Result<(String, String)> {
    let html = browser.fetch(url).await?;
    let html = browser.prepare_html(&html, url).await?;
    let convert_opts = ConvertOptions::default();
    let md = PageToMarkdown::convert_with(&html, &convert_opts, &[])?;
    let meta = extract_page_metadata(&html, &md);
    let body = PageToMarkdown::to_plain_text(&md);
    let fp = meta.fingerprint.unwrap_or_else(|| content_fingerprint(&body));
    Ok((fp, body))
}

fn unix_secs_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn watch_state_filename(url: &str) -> String {
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

fn load_watch_state(dir: Option<&std::path::Path>, url: &str) -> anyhow::Result<Option<String>> {
    let Some(dir) = dir else { return Ok(None) };
    let path = dir.join(watch_state_filename(url));
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(&path)?;
    Ok(Some(data.trim().to_string()))
}

fn save_watch_state(
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

/// Map a CLI `OutputFormat` to a lowercase identifier for use in webhook payloads.
fn format_label(format: &OutputFormat) -> &'static str {
    match format {
        OutputFormat::Markdown => "markdown",
        OutputFormat::Html => "html",
        OutputFormat::Json => "json",
        OutputFormat::Text => "text",
        OutputFormat::Csv => "csv",
        OutputFormat::Tei => "tei",
        OutputFormat::Xml => "xml",
        OutputFormat::Branding => "branding",
    }
}

/// POST `body` as JSON to `url`. Returns Ok(()) for any 2xx response. Non-2xx
/// are returned as errors so callers can log. Network errors also propagate.
async fn post_webhook(url: &str, body: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("building webhook client")?;
    let resp = client
        .post(url)
        .header("content-type", "application/json")
        .header("user-agent", "web2md-webhook/0.1")
        .body(body.to_string())
        .send()
        .await
        .context("POSTing webhook")?;
    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else {
        anyhow::bail!("webhook returned HTTP {}", status)
    }
}

/// Build [`BrowserOptions`] from common CLI flags shared by fetch/browse/batch.
fn build_browser_options(
    timeout: Option<u64>,
    delay: Option<u64>,
    wait: Option<u64>,
    cache_ttl: Option<u64>,
    cookies: Vec<String>,
    headers: Vec<String>,
    javascript: bool,
    no_blacklist: bool,
    no_user_blacklist: bool,
    blacklist_file: Vec<String>,
    ignore_robots: bool,
) -> BrowserOptions {
    let mut options = BrowserOptions::default();
    if let Some(secs) = timeout {
        options.timeout = Duration::from_secs(secs);
    }
    if let Some(ms) = delay {
        options.request_delay = Duration::from_millis(ms);
    }
    if let Some(ms) = wait {
        options.post_load_wait = Duration::from_millis(ms);
    }
    if let Some(secs) = cache_ttl {
        options.cache_ttl = Duration::from_secs(secs);
    }
    options.cookies = cookies;
    options.headers = headers;
    options.enable_javascript = javascript;
    apply_blacklist_options(&mut options, no_blacklist, no_user_blacklist, blacklist_file);
    options.respect_robots_txt = !ignore_robots;
    options
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            if let Some(url) = cli.url {
                let options = BrowserOptions::default();
                browse_loop(url, options, false, false, false).await?;
            } else {
                Cli::parse_from(["web2md", "--help"]);
            }
        }
        Some(Commands::Fetch {
            url,
            max_length,
            timeout,
            include_images,
            cookie,
            header,
            format,
            render,
            lang,
            precision,
            recall,
            no_comments,
            no_tables,
            no_links,
            only_with_metadata,
            delay,
            keep_header,
            cache_ttl,
            main_content,
            output: output_file,
            frontmatter,
            exclude_selector,
            javascript,
            wait,
            no_blacklist,
            depth,
            ignore_robots,
            blacklist_file,
            no_user_blacklist,
            topic,
            max_tokens,
            summary,
            r#type,
            cache_dir,
            rate,
            webhook,
        }) => {
            let options = build_browser_options(
                timeout,
                delay,
                wait,
                cache_ttl,
                cookie,
                header,
                javascript,
                no_blacklist,
                no_user_blacklist,
                blacklist_file,
                ignore_robots,
            );
            let mut options = options;
            if let Some(dir) = cache_dir {
                options.cache_dir = Some(std::path::PathBuf::from(dir));
            }
            options.host_rate_limit = rate;
            let browser = Browser::new(options)?;

            if depth > 0 {
                if !matches!(format, OutputFormat::Markdown) {
                    anyhow::bail!("--depth requires markdown output format");
                }
                crawl_fetch(
                    &browser,
                    &url,
                    depth,
                    max_length,
                    include_images,
                    keep_header,
                    main_content,
                    frontmatter,
                    &exclude_selector,
                    output_file.as_deref(),
                    render,
                )
                .await?;
            } else {
                let html = browser.fetch(&url).await?;
                let html = browser.prepare_html(&html, &url).await?;

                let format_label_value = format_label(&format);
                let (mut result, frontmatter_meta) = match format {
                    OutputFormat::Branding => {
                        let profile = web2md::extract_branding(&html);
                        (serde_json::to_string_pretty(&profile)?, None)
                    }
                    OutputFormat::Html => {
                        if lang.is_some() {
                            anyhow::bail!("--lang requires a converted output format (not html)");
                        }
                        if only_with_metadata {
                            anyhow::bail!(
                                "--only-with-metadata requires a converted output format (not html)"
                            );
                        }
                        if r#type.is_some() {
                            anyhow::bail!("--type requires a converted output format (not html)");
                        }
                        (html.clone(), None)
                    }
                    format => {
                        let struct_result: Option<String> = match r#type.as_deref() {
                            Some("recipe") => match extract_recipe(&html) {
                                Ok(Some(md)) => Some(md),
                                Ok(None) => None,
                                Err(e) => {
                                    eprintln!("--type recipe: {}, falling back", e);
                                    None
                                }
                            },
                            Some("faq") => match extract_faq(&html) {
                                Ok(Some(md)) => Some(md),
                                Ok(None) => None,
                                Err(e) => {
                                    eprintln!("--type faq: {}, falling back", e);
                                    None
                                }
                            },
                            Some("job") => match extract_job(&html) {
                                Ok(Some(md)) => Some(md),
                                Ok(None) => None,
                                Err(e) => {
                                    eprintln!("--type job: {}, falling back", e);
                                    None
                                }
                            },
                            Some("event") => match extract_event(&html) {
                                Ok(Some(md)) => Some(md),
                                Ok(None) => None,
                                Err(e) => {
                                    eprintln!("--type event: {}, falling back", e);
                                    None
                                }
                            },
                            Some(other) => {
                                anyhow::bail!(
                                    "unsupported --type {}; expected one of: recipe, faq, job, event",
                                    other
                                );
                            }
                            None => None,
                        };
                        let from_struct = struct_result.is_some();
                        let mut md = match struct_result {
                            Some(md) => md,
                            None => {
                                let convert_opts = ConvertOptions {
                                    include_images,
                                    keep_header,
                                    main_content,
                                    favor_precision: precision,
                                    favor_recall: recall,
                                    include_comments: !no_comments,
                                    include_tables: !no_tables,
                                    include_links: !no_links,
                                };
                                let body = PageToMarkdown::convert_with(
                                    &html,
                                    &convert_opts,
                                    &exclude_selector,
                                )?;
                                PageToMarkdown::absolutize_links(&body, &url)
                            }
                        };
                        let meta = extract_page_metadata(&html, &md);
                        if let Some(ref target) = lang {
                            if !language_matches(meta.language.as_deref(), target) {
                                anyhow::bail!(
                                    "page language {:?} does not match --lang {}",
                                    meta.language.as_deref().unwrap_or("(unknown)"),
                                    target
                                );
                            }
                        }
                        if only_with_metadata
                            && (meta.title.is_none() || meta.published_date.is_none())
                        {
                            anyhow::bail!(
                                "--only-with-metadata requires title and published_date; found title={:?} published_date={:?}",
                                meta.title.as_deref().unwrap_or("(missing)"),
                                meta.published_date.as_deref().unwrap_or("(missing)")
                            );
                        }
                        if !from_struct {
                            if let Some(ref t) = topic {
                                md = match extract_topic(&md, t, None) {
                                    Some(f) => PageToMarkdown::absolutize_links(&f, &url),
                                    None => anyhow::bail!(
                                        "--topic: no paragraphs matched query {:?}",
                                        t
                                    ),
                                };
                            }
                            if let Some(sentences) = summary {
                                md = match extract_summary(&md, sentences, meta.title.as_deref()) {
                                    Some(s) => s,
                                    None => md,
                                };
                            }
                        }
                        let fm_meta = matches!(format, OutputFormat::Markdown | OutputFormat::Text)
                            .then(|| meta.clone());
                        let out = match format {
                            OutputFormat::Markdown => {
                                if render {
                                    render_markdown_ansi(&md, false).0
                                } else {
                                    md
                                }
                            }
                            OutputFormat::Json => {
                                let output = CliJsonOutput {
                                    markdown: md,
                                    meta,
                                };
                                serde_json::to_string_pretty(&output)?
                            }
                            OutputFormat::Text => PageToMarkdown::to_plain_text(&md),
                            OutputFormat::Csv => {
                                let text = PageToMarkdown::to_plain_text(&md);
                                meta.to_csv(&url, &text)
                            }
                            OutputFormat::Tei => {
                                let text = PageToMarkdown::to_plain_text(&md);
                                meta.to_tei(&url, &text)
                            }
                            OutputFormat::Xml => {
                                let text = PageToMarkdown::to_plain_text(&md);
                                meta.to_xml(&url, &text)
                            }
                            OutputFormat::Html => unreachable!(),
                            OutputFormat::Branding => unreachable!(),
                        };
                        (out, fm_meta)
                    }
                };

                if frontmatter {
                    if let Some(meta) = frontmatter_meta {
                        if let Some(fm) = meta.to_frontmatter(Some(&url)) {
                            result = format!("{}{}", fm, result);
                        }
                    }
                }

                if let Some(max) = max_tokens {
                    result = truncate_by_tokens(&result, max);
                } else if let Some(max) = max_length {
                    result = truncate_with_marker(&result, max);
                }

                if let Some(hook) = webhook.as_deref() {
                    let payload = serde_json::json!({
                        "event": "fetch.completed",
                        "url": url,
                        "format": format_label_value,
                        "result": result,
                    });
                    if let Err(e) = post_webhook(hook, &payload.to_string()).await {
                        eprintln!("webhook POST to {} failed: {}", hook, e);
                    }
                }

                if let Some(path) = output_file {
                    std::fs::write(&path, &result)?;
                    eprintln!("Written to {}", path);
                } else {
                    println!("{}", result);
                }
            }
        }
        Some(Commands::Browse {
            url,
            timeout,
            include_images,
            cookie,
            header,
            delay,
            keep_header,
            cache_ttl,
            main_content,
            javascript,
            wait,
            no_blacklist,
            ignore_robots,
            blacklist_file,
            no_user_blacklist,
        }) => {
            let options = build_browser_options(
                timeout,
                delay,
                wait,
                cache_ttl,
                cookie,
                header,
                javascript,
                no_blacklist,
                no_user_blacklist,
                blacklist_file,
                ignore_robots,
            );
            browse_loop(url, options, include_images, keep_header, main_content).await?;
        }
        Some(Commands::Mcp) => {
            let server = McpServer::new()?;
            run_stdio_mcp(&server).await?;
        }
        Some(Commands::Diff {
            url_a,
            url_b,
            cached_b,
            timeout,
            cookie,
            header,
            json,
        }) => {
            use web2md::diff_markdown;
            let mut options = BrowserOptions::default();
            if let Some(secs) = timeout {
                options.timeout = Duration::from_secs(secs);
            }
            options.cookies = cookie;
            options.headers = header;
            let browser = Browser::new(options)?;
            let html_a = browser.fetch(&url_a).await?;
            let html_a = browser.prepare_html(&html_a, &url_a).await?;
            let md_a = PageToMarkdown::convert(&html_a, false, false, false, &[])?;
            let md_a = PageToMarkdown::absolutize_links(&md_a, &url_a);
            let md_b = if cached_b {
                std::fs::read_to_string(&url_b)
                    .with_context(|| format!("reading cached Markdown from {}", url_b))?
            } else {
                let html_b = browser.fetch(&url_b).await?;
                let html_b = browser.prepare_html(&html_b, &url_b).await?;
                let body = PageToMarkdown::convert(&html_b, false, false, false, &[])?;
                PageToMarkdown::absolutize_links(&body, &url_b)
            };
            let diff = diff_markdown(&url_a, &md_a, &url_b, &md_b);
            if json {
                let (added, removed) = web2md::summarize(&diff);
                let out = serde_json::json!({
                    "url_a": url_a,
                    "url_b": url_b,
                    "lines_added": added,
                    "lines_removed": removed,
                    "diff": diff,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                let (added, removed) = web2md::summarize(&diff);
                eprintln!("+{} -{}\n", added, removed);
                println!("{}", diff);
            }
        }
        Some(Commands::Transcript {
            url,
            timeout,
            lang,
            cookie,
            header,
            ignore_robots,
        }) => {
            use web2md::{
                extract_caption_track_url, is_youtube_url, parse_timed_text,
                render_transcript_markdown,
            };
            if !is_youtube_url(&url) {
                anyhow::bail!(
                    "{} is not a recognized YouTube URL (expected youtube.com/watch or youtu.be)",
                    url
                );
            }
            let mut options = BrowserOptions::default();
            if let Some(secs) = timeout {
                options.timeout = Duration::from_secs(secs);
            }
            options.cookies = cookie;
            options.headers = header;
            options.respect_robots_txt = !ignore_robots;
            let browser = Browser::new(options)?;
            let watch_html = browser.fetch(&url).await?;
            let track_url = match extract_caption_track_url(&watch_html, lang.as_deref()) {
                Some(u) => u,
                None => anyhow::bail!(
                    "no caption track found in watch HTML for {} (page may not have captions)",
                    url
                ),
            };
            let caption_xml = browser.fetch(&track_url).await?;
            let cues = parse_timed_text(&caption_xml)?;
            let md = render_transcript_markdown(&cues);
            print!("{}", md);
        }
        Some(Commands::Watch {
            url,
            every,
            timeout,
            cookie,
            header,
            cache_dir,
            ignore_robots,
        }) => {
            use std::time::Duration as StdDuration;
            let mut options = BrowserOptions::default();
            if let Some(secs) = timeout {
                options.timeout = Duration::from_secs(secs);
            }
            options.cookies = cookie;
            options.headers = header;
            options.respect_robots_txt = !ignore_robots;
            // For watch, we deliberately bypass the persistent cache during the
            // comparison fetch so each tick sees the live page, then store the
            // resulting fingerprint in a sibling state file.
            let state_path = cache_dir.as_deref().map(std::path::Path::new);
            let browser = Browser::new(options)?;
            let mut last_fp: Option<String> = load_watch_state(state_path, &url)?;
            let interval = StdDuration::from_secs(every.max(1));
            // First fetch happens immediately.
            loop {
                match poll_once(&browser, &url).await {
                    Ok((fp, body)) => {
                        if last_fp.as_deref() != Some(fp.as_str()) {
                            let ts = unix_secs_string();
                            println!(
                                "{}\t{}\t{}\t{}",
                                ts,
                                url,
                                fp,
                                body.chars().take(80).collect::<String>()
                            );
                            last_fp = Some(fp.clone());
                            let _ = save_watch_state(state_path, &url, &fp);
                        }
                    }
                    Err(e) => eprintln!("watch error: {}", e),
                }
                tokio::time::sleep(interval).await;
            }
        }
        Some(Commands::Peek {
            url,
            timeout,
            cookie,
            header,
            delay,
            ignore_robots,
            no_blacklist,
            json,
        }) => {
            let mut options = BrowserOptions::default();
            if let Some(secs) = timeout {
                options.timeout = Duration::from_secs(secs);
            }
            if let Some(ms) = delay {
                options.request_delay = Duration::from_millis(ms);
            }
            options.cookies = cookie;
            options.headers = header;
            options.respect_robots_txt = !ignore_robots;
            options.filter_blacklisted_urls = !no_blacklist;
            let browser = Browser::new(options)?;
            let html = browser.fetch(&url).await?;
            let html = browser.prepare_html(&html, &url).await?;
            let meta = extract_page_metadata(&html, "");
            let excerpt = meta.excerpt.clone().unwrap_or_default();
            if json {
                let output = serde_json::json!({
                    "url": url,
                    "title": meta.title,
                    "description": meta.description,
                    "author": meta.author,
                    "published_date": meta.published_date,
                    "site_name": meta.site_name,
                    "language": meta.language,
                    "excerpt": excerpt,
                    "fingerprint": meta.fingerprint,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("URL:        {}", url);
                if let Some(t) = meta.title.as_deref() {
                    println!("Title:      {}", t);
                }
                if let Some(d) = meta.description.as_deref() {
                    println!("Description: {}", d);
                }
                if let Some(a) = meta.author.as_deref() {
                    println!("Author:     {}", a);
                }
                if let Some(d) = meta.published_date.as_deref() {
                    println!("Date:       {}", d);
                }
                if let Some(s) = meta.site_name.as_deref() {
                    println!("Site:       {}", s);
                }
                if let Some(l) = meta.language.as_deref() {
                    println!("Language:   {}", l);
                }
                if !excerpt.is_empty() {
                    println!("\nExcerpt:\n  {}", excerpt.replace('\n', " "));
                }
            }
        }
        Some(Commands::Sitemap {
            url,
            timeout,
            cookie,
            header,
            feeds,
        }) => {
            let mut options = BrowserOptions::default();
            if let Some(secs) = timeout {
                options.timeout = Duration::from_secs(secs);
            }
            options.cookies = cookie;
            options.headers = header;
            let browser = Browser::new(options)?;

            let parsed = Url::parse(&url).context("Invalid URL")?;
            let sitemap_url = format!("{}://{}/sitemap.xml", parsed.scheme(), parsed.host_str().unwrap_or(""));

            let mut found_urls: Vec<String> = Vec::new();

            // Try fetching sitemap.xml
            match browser.fetch(&sitemap_url).await {
                Ok(xml) => {
                    let sitemap_urls: Vec<String> = parse_sitemap_urls(&xml)
                        .into_iter()
                        .filter(|u| !browser.is_url_blocked(u))
                        .collect();
                    if !sitemap_urls.is_empty() {
                        println!("# Sitemap URLs from {}\n", sitemap_url);
                        for u in &sitemap_urls {
                            println!("{}", u);
                        }
                        found_urls.extend(sitemap_urls);
                    }
                }
                Err(e) => {
                    eprintln!("No sitemap.xml found: {}", e);
                }
            }

            // Optionally check the HTML page for feed links
            if feeds {
                match browser.fetch(&url).await {
                    Ok(html) => {
                        let feed_urls = extract_feed_links(&html);
                        if !feed_urls.is_empty() {
                            println!("\n# Feed links from {}\n", url);
                            for f in &feed_urls {
                                println!("{}", f);
                            }
                            found_urls.extend(feed_urls);
                        }
                    }
                    Err(e) => {
                        eprintln!("Could not fetch page for feed discovery: {}", e);
                    }
                }
            }

            if found_urls.is_empty() {
                eprintln!("No sitemap or feed URLs found.");
            }
        }
        Some(Commands::Feed {
            url,
            timeout,
            cookie,
            header,
            max_entries,
            json,
            output: output_file,
        }) => {
            let mut options = BrowserOptions::default();
            if let Some(secs) = timeout {
                options.timeout = Duration::from_secs(secs);
            }
            options.cookies = cookie;
            options.headers = header;
            let browser = Browser::new(options)?;

            let xml = browser.fetch(&url).await.context("Failed to fetch feed")?;
            let mut feed = parse_feed(&xml).context("URL did not contain a valid RSS, Atom, or JSON Feed")?;
            if let Some(max) = max_entries {
                feed.entries.truncate(max);
            }

            let result = if json {
                #[derive(Serialize)]
                struct FeedJsonEntry {
                    #[serde(skip_serializing_if = "Option::is_none")]
                    title: Option<String>,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    link: Option<String>,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    published: Option<String>,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    summary: Option<String>,
                }
                #[derive(Serialize)]
                struct FeedJson {
                    #[serde(skip_serializing_if = "Option::is_none")]
                    title: Option<String>,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    link: Option<String>,
                    entries: Vec<FeedJsonEntry>,
                }
                let output = FeedJson {
                    title: feed.title,
                    link: feed.link,
                    entries: feed
                        .entries
                        .into_iter()
                        .map(|e| FeedJsonEntry {
                            title: e.title,
                            link: e.link,
                            published: e.published,
                            summary: e.summary,
                        })
                        .collect(),
                };
                serde_json::to_string_pretty(&output)?
            } else {
                feed_to_markdown(&feed)
            };

            if let Some(path) = output_file {
                std::fs::write(&path, &result)
                    .with_context(|| format!("Failed to write output to {}", path))?;
            } else {
                println!("{}", result);
            }
        }
        Some(Commands::Batch {
            file,
            timeout,
            include_images,
            cookie,
            header,
            delay,
            keep_header,
            cache_ttl,
            main_content,
            output: output_dir,
            frontmatter,
            exclude_selector,
            javascript,
            wait,
            no_blacklist,
            ignore_robots,
            blacklist_file,
            no_user_blacklist,
        }) => {
            let content = std::fs::read_to_string(&file)
                .context("Failed to read batch file")?;
            let urls: Vec<String> = content
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| l.to_string())
                .collect();

            if urls.is_empty() {
                eprintln!("No URLs found in {}", file);
                return Ok(());
            }

            let options = build_browser_options(
                timeout,
                delay,
                wait,
                cache_ttl,
                cookie,
                header,
                javascript,
                no_blacklist,
                no_user_blacklist,
                blacklist_file,
                ignore_robots,
            );
            let browser = Browser::new(options)?;

            // Create output directory if specified
            if let Some(ref dir) = output_dir {
                std::fs::create_dir_all(dir)?;
            }

            let total = urls.len();
            let mut succeeded = 0;
            let mut failed = 0;
            let mut skipped = 0;

            for (i, url) in urls.iter().enumerate() {
                eprintln!("[{}/{}] {}", i + 1, total, url);

                if browser.is_url_blocked(url) {
                    eprintln!("  Skipped (blacklisted URL)");
                    skipped += 1;
                    continue;
                }

                if !browser.robots_allows(url).await? {
                    eprintln!("  Skipped (robots.txt)");
                    skipped += 1;
                    continue;
                }

                match browser.fetch(url).await {
                    Ok(html) => {
                        let html = match browser.prepare_html(&html, url).await {
                            Ok(prepared) => prepared,
                            Err(_) => html,
                        };
                        match PageToMarkdown::convert(&html, include_images, keep_header, main_content, &exclude_selector) {
                            Ok(md) => {
                                let md = PageToMarkdown::absolutize_links(&md, url);
                                let md = if frontmatter {
                                    let meta = extract_page_metadata(&html, &md);
                                    if let Some(fm) = meta.to_frontmatter(Some(url)) {
                                        format!("{}{}", fm, md)
                                    } else {
                                        md
                                    }
                                } else {
                                    md
                                };
                                if let Some(ref dir) = output_dir {
                                    let filename = url_to_filename(url);
                                    let path = format!("{}/{}", dir, filename);
                                    std::fs::write(&path, &md)?;
                                    eprintln!("  → {}", path);
                                } else {
                                    println!("---\n# {}\n\n{}", url, md);
                                }
                                succeeded += 1;
                            }
                            Err(e) => {
                                eprintln!("  Error converting: {}", e);
                                failed += 1;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  Error fetching: {}", e);
                        failed += 1;
                    }
                }
            }

            eprintln!("\nDone: {}/{} succeeded, {} failed, {} skipped", succeeded, total, failed, skipped);
        }
    }

    Ok(())
}

/// Recursively fetch and convert same-origin pages up to `depth` link hops.
async fn crawl_fetch(
    browser: &Browser,
    start_url: &str,
    depth: u32,
    max_length: Option<usize>,
    include_images: bool,
    keep_header: bool,
    main_content: bool,
    frontmatter: bool,
    exclude_selector: &[String],
    output_dir: Option<&str>,
    render: bool,
) -> Result<()> {
    let root = Url::parse(start_url).context("Invalid URL")?;
    let start = normalize_crawl_url(start_url, start_url)
        .unwrap_or_else(|| start_url.to_string());

    if let Some(dir) = output_dir {
        std::fs::create_dir_all(dir)?;
    }

    let mut visited = HashSet::new();
    let mut queue = VecDeque::from([(start.clone(), 0u32)]);
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    while let Some((url, level)) = queue.pop_front() {
        let key = normalize_crawl_url(&url, &url).unwrap_or_else(|| url.clone());
        if !visited.insert(key) {
            continue;
        }

        if browser.is_url_blocked(&url) {
            eprintln!("Skipped (blacklisted): {}", url);
            skipped += 1;
            continue;
        }

        if !browser.robots_allows(&url).await? {
            eprintln!("Skipped (robots.txt): {}", url);
            skipped += 1;
            continue;
        }

        eprintln!("[depth {}] {}", level, url);

        match browser.fetch(&url).await {
            Ok(html) => {
                let html = match browser.prepare_html(&html, &url).await {
                    Ok(prepared) => prepared,
                    Err(_) => html,
                };

                if level < depth {
                    for link in browser.same_origin_links(&html, &url, &root) {
                        let link_key =
                            normalize_crawl_url(&link, &link).unwrap_or(link.clone());
                        if !visited.contains(&link_key) {
                            queue.push_back((link, level + 1));
                        }
                    }
                }

                match PageToMarkdown::convert(
                    &html,
                    include_images,
                    keep_header,
                    main_content,
                    exclude_selector,
                ) {
                    Ok(md) => {
                        let mut md = PageToMarkdown::absolutize_links(&md, &url);
                        if frontmatter {
                            let meta = extract_page_metadata(&html, &md);
                            if let Some(fm) = meta.to_frontmatter(Some(&url)) {
                                md = format!("{}{}", fm, md);
                            }
                        }
                        if let Some(max) = max_length {
                            md = truncate_with_marker(&md, max);
                        }
                        if render {
                            md = render_markdown_ansi(&md, false).0;
                        }

                        if let Some(dir) = output_dir {
                            let filename = url_to_filename(&url);
                            let path = format!("{}/{}", dir, filename);
                            std::fs::write(&path, &md)?;
                            eprintln!("  → {}", path);
                        } else {
                            println!("---\n# {}\n\n{}", url, md);
                        }
                        succeeded += 1;
                    }
                    Err(e) => {
                        eprintln!("  Error converting: {}", e);
                        failed += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("  Error fetching: {}", e);
                failed += 1;
            }
        }
    }

    eprintln!(
        "\nCrawl done: {} succeeded, {} failed, {} skipped",
        succeeded, failed, skipped
    );
    Ok(())
}

/// Convert a URL to a safe filename for batch output.
/// e.g. "https://example.com/blog/post" → "example.com_blog_post.md"
fn url_to_filename(url: &str) -> String {
    let parsed = match Url::parse(url) {
        Ok(u) => u,
        Err(_) => return format!("{}.md", url.replace(['/', ':', '?', '=', '&'], "_")),
    };
    let host = parsed.host_str().unwrap_or("unknown");
    let path = parsed.path().trim_start_matches('/');
    let path = if path.is_empty() { "index" } else { path };
    let path = path.replace(['/', '?', '=', '&'], "_");
    format!("{}_{}.md", host, path)
}

/// Interactive Lynx-like browser loop.
async fn browse_loop(start_url: String, options: BrowserOptions, include_images: bool, keep_header: bool, main_content: bool) -> Result<()> {
    let mut history = vec![start_url];
    let mut current = 0;
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let browser = Browser::new(options)?;

    loop {
        let url = history[current].clone();

        // Clear screen + header bar
        print!("\x1b[2J\x1b[H");
        println!("\x1b[7m WEB2MD \x1b[0m \x1b[90m{}\x1b[0m\n", url);
        io::stdout().flush()?;

        print!("\x1b[90mFetching...\x1b[0m");
        io::stdout().flush()?;

        let html = match browser.fetch(&url).await {
            Ok(h) => match browser.prepare_html(&h, &url).await {
                Ok(prepared) => prepared,
                Err(_) => h,
            },
            Err(e) => {
                println!("\r\x1b[2K\x1b[91mError: {}\x1b[0m", e);
                println!("\nPress Enter to continue...");
                let mut _buf = String::new();
                let _ = stdin_lock.read_line(&mut _buf);
                continue;
            }
        };

        print!("\r\x1b[2K\x1b[90mConverting...\x1b[0m");
        io::stdout().flush()?;

        let mut renderer = AnsiRenderer::new(true);
        let page_url = url.clone();
        let mut first_block = true;
        PageToMarkdown::convert_progressive(
            &html,
            include_images,
            keep_header,
            main_content,
            &[],
            |block| {
                if first_block {
                    print!("\r\x1b[2K");
                    first_block = false;
                }
                let block = PageToMarkdown::absolutize_links(&block, &page_url);
                let rendered = renderer.render_chunk(&block);
                let trimmed = rendered.trim_end();
                if !trimmed.is_empty() {
                    print!("{trimmed}\n");
                }
                let _ = io::stdout().flush();
            },
        )?;

        let links = renderer.into_links();

        println!(
            "\n\x1b[90m[q]uit [b]ack [f]orward [u]rl [1-{}] follow link\x1b[0m",
            links.len()
        );
        print!("\x1b[1m> \x1b[0m");
        io::stdout().flush()?;

        let mut input = String::new();
        stdin_lock.read_line(&mut input)?;
        let input = input.trim();

        match input {
            "q" | "Q" => break,
            "b" | "B" => {
                if current > 0 {
                    current -= 1;
                }
            }
            "f" | "F" => {
                if current < history.len() - 1 {
                    current += 1;
                }
            }
            "u" | "U" => {
                print!("URL: ");
                io::stdout().flush()?;
                let mut new_url = String::new();
                stdin_lock.read_line(&mut new_url)?;
                let new_url = new_url.trim().to_string();
                if !new_url.is_empty() {
                    history.truncate(current + 1);
                    history.push(new_url);
                    current += 1;
                }
            }
            num => {
                if let Ok(n) = num.parse::<usize>() {
                    if n > 0 && n <= links.len() {
                        let target = resolve_url(&url, &links[n - 1]);
                        history.truncate(current + 1);
                        history.push(target);
                        current += 1;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Resolve a relative URL against a base URL.
fn resolve_url(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }
    if relative.starts_with("//") {
        if let Some(prefix) = base.split("://").next() {
            return format!("{}:{}", prefix, relative);
        }
        return relative.to_string();
    }
    if let Ok(base_url) = url::Url::parse(base) {
        if let Ok(resolved) = base_url.join(relative) {
            return resolved.to_string();
        }
    }
    relative.to_string()
}

/// Incremental ANSI Markdown renderer for progressive browse output.
struct AnsiRenderer {
    link_counter: usize,
    links: Vec<String>,
    number_links: bool,
    out: String,
    pending_link: Option<(usize, String)>,
    in_table: bool,
    table_rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    col_alignments: Vec<pulldown_cmark::Alignment>,
    _in_header: bool,
}

impl AnsiRenderer {
    fn new(number_links: bool) -> Self {
        Self {
            link_counter: 0,
            links: Vec::new(),
            number_links,
            out: String::new(),
            pending_link: None,
            in_table: false,
            table_rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: String::new(),
            col_alignments: Vec::new(),
            _in_header: false,
        }
    }

    fn render_chunk(&mut self, md: &str) -> String {
        use pulldown_cmark::{Event, Options, Tag, TagEnd};

        self.out.clear();
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_TABLES);
        let parser = pulldown_cmark::Parser::new_ext(md, opts);

        for event in parser {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Heading { level, .. } => {
                        let c = match level {
                            pulldown_cmark::HeadingLevel::H1 => "\x1b[1;91m",
                            pulldown_cmark::HeadingLevel::H2 => "\x1b[1;93m",
                            pulldown_cmark::HeadingLevel::H3 => "\x1b[1;92m",
                            pulldown_cmark::HeadingLevel::H4 => "\x1b[1;94m",
                            pulldown_cmark::HeadingLevel::H5 => "\x1b[1;95m",
                            pulldown_cmark::HeadingLevel::H6 => "\x1b[1;96m",
                        };
                        self.out.push_str(c);
                    }
                    Tag::Strong => self.out.push_str("\x1b[1m"),
                    Tag::Emphasis => self.out.push_str("\x1b[3m"),
                    Tag::Link { dest_url, .. } => {
                        if self.number_links && !self.in_table {
                            self.link_counter += 1;
                            self.pending_link = Some((self.link_counter, dest_url.to_string()));
                        }
                        if self.in_table {
                            self.current_cell.push_str("\x1b[4;36m");
                        } else {
                            self.out.push_str("\x1b[4;36m");
                        }
                    }
                    Tag::BlockQuote(_) => self.out.push_str("\x1b[90m▌ \x1b[3m"),
                    Tag::CodeBlock(_) => self.out.push_str("\x1b[48;5;235;38;5;250m"),
                    Tag::List(_) => {}
                    Tag::Item => self.out.push_str("  • "),
                    Tag::Table(aligns) => {
                        self.in_table = true;
                        self.col_alignments = aligns.to_vec();
                    }
                    Tag::TableHead => self._in_header = true,
                    Tag::TableRow => self.current_row = Vec::new(),
                    Tag::TableCell => self.current_cell.clear(),
                    _ => {}
                },
                Event::End(tag) => match tag {
                    TagEnd::Heading(_) => self.out.push_str("\x1b[0m\n"),
                    TagEnd::Paragraph => {
                        if !self.in_table {
                            self.out.push('\n');
                        }
                    }
                    TagEnd::Strong | TagEnd::Emphasis => {
                        if self.in_table {
                            self.current_cell.push_str("\x1b[0m");
                        } else {
                            self.out.push_str("\x1b[0m");
                        }
                    }
                    TagEnd::Link => {
                        if self.in_table {
                            self.current_cell.push_str("\x1b[0m");
                        } else {
                            if let Some((n, url)) = self.pending_link.take() {
                                self.links.push(url.clone());
                                self.out
                                    .push_str(&format!("\x1b[33m[{}]\x1b[0m ", n));
                                self.out.push_str(&fallback_link_label(&url));
                            }
                            self.out.push_str("\x1b[0m");
                        }
                    }
                    TagEnd::BlockQuote(_) => self.out.push_str("\x1b[0m\n"),
                    TagEnd::CodeBlock => self.out.push_str("\x1b[0m\n"),
                    TagEnd::Item => {}
                    TagEnd::TableCell => {
                        self.current_row.push(std::mem::take(&mut self.current_cell));
                    }
                    TagEnd::TableRow => {
                        self.table_rows.push(std::mem::take(&mut self.current_row));
                    }
                    TagEnd::TableHead => {
                        self._in_header = false;
                        if !self.current_row.is_empty() {
                            self.table_rows.push(std::mem::take(&mut self.current_row));
                        }
                    }
                    TagEnd::Table => {
                        self.in_table = false;
                        self.out.push_str(&render_ansi_table(
                            &self.table_rows,
                            &self.col_alignments,
                        ));
                        self.table_rows.clear();
                        self.col_alignments.clear();
                    }
                    _ => {}
                },
                Event::Text(text) => {
                    if self.in_table {
                        self.current_cell.push_str(&text);
                    } else if let Some((n, url)) = self.pending_link.take() {
                        self.links.push(url);
                        self.out
                            .push_str(&format!("\x1b[33m[{}]\x1b[0m ", n));
                        self.out.push_str(&text);
                    } else {
                        self.out.push_str(&text);
                    }
                }
                Event::Code(code) => {
                    let s = format!("\x1b[38;5;250m{}\x1b[0m", code);
                    if self.in_table {
                        self.current_cell.push_str(&s);
                    } else {
                        self.out.push_str(&s);
                    }
                }
                Event::Html(html) => {
                    if self.in_table {
                        self.current_cell.push_str(&html);
                    } else {
                        self.out.push_str(&html);
                    }
                }
                Event::SoftBreak => {
                    if self.in_table {
                        self.current_cell.push(' ');
                    } else {
                        self.out.push(' ');
                    }
                }
                Event::HardBreak => {
                    if self.in_table {
                        self.current_cell.push('\n');
                    } else {
                        self.out.push('\n');
                    }
                }
                Event::Rule => {
                    self.out.push_str(
                        "\x1b[90m────────────────────────────────────────\x1b[0m\n",
                    );
                }
                _ => {}
            }
        }

        self.out = fix_raw_links(
            &self.out,
            self.number_links,
            &mut self.link_counter,
            &mut self.links,
        );
        std::mem::take(&mut self.out)
    }

    fn into_links(self) -> Vec<String> {
        self.links
    }
}

/// Render Markdown with ANSI escape codes for terminal display.
/// Markdown syntax is stripped; visual effects (bold, color, underline) replace it.
fn render_markdown_ansi(md: &str, number_links: bool) -> (String, Vec<String>) {
    let mut renderer = AnsiRenderer::new(number_links);
    let rendered = renderer.render_chunk(md);
    (rendered, renderer.into_links())
}

/// Display label for links with no visible anchor text.
fn fallback_link_label(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(segments) = parsed.path_segments() {
            if let Some(seg) = segments.filter(|s| !s.is_empty()).last() {
                return seg.replace('-', " ");
            }
        }
        if let Some(host) = parsed.host_str() {
            return host.to_string();
        }
    }
    url.to_string()
}

/// Strip ANSI escape sequences from a string to get the visual width.
fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
            continue;
        }
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Compute visual width of a string (ANSI codes excluded).
fn visual_width(s: &str) -> usize {
    strip_ansi(s).chars().count()
}

/// Pad a string to target visual width, preserving any ANSI prefixes/suffixes.
fn pad_visual(s: &str, width: usize, align: &pulldown_cmark::Alignment) -> String {
    let stripped = strip_ansi(s);
    let stripped_len = stripped.chars().count();
    if stripped_len >= width {
        return s.to_string();
    }
    let pad = width - stripped_len;
    match align {
        pulldown_cmark::Alignment::Right => format!("{}{}", " ".repeat(pad), s),
        pulldown_cmark::Alignment::Center => {
            let left = pad / 2;
            let right = pad - left;
            format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
        }
        _ => format!("{}{}", s, " ".repeat(pad)),
    }
}

/// Render buffered table rows as an ANSI-styled box-drawing table.
fn render_ansi_table(rows: &[Vec<String>], aligns: &[pulldown_cmark::Alignment]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return String::new();
    }

    let mut widths = vec![0usize; cols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(visual_width(cell));
        }
    }
    for w in &mut widths {
        *w = (*w).max(1);
    }

    let mut out = String::new();

    // Top border
    out.push_str("\x1b[90m┌");
    for (i, w) in widths.iter().enumerate() {
        out.push_str(&"─".repeat(w + 2));
        if i < widths.len() - 1 {
            out.push('┬');
        }
    }
    out.push_str("┐\x1b[0m\n");

    for (ri, row) in rows.iter().enumerate() {
        let is_header = ri == 0 && row.len() == cols;
        out.push_str("\x1b[90m│\x1b[0m ");
        for ci in 0..cols {
            let cell = row.get(ci).map(|s| s.as_str()).unwrap_or("");
            let align = aligns.get(ci).unwrap_or(&pulldown_cmark::Alignment::None);
            let padded = pad_visual(cell, widths[ci], align);
            if is_header {
                out.push_str("\x1b[1m");
                out.push_str(&padded);
                out.push_str("\x1b[0m");
            } else {
                out.push_str(&padded);
            }
            out.push_str(" \x1b[90m│\x1b[0m ");
        }
        out.push('\n');

        // Separator after header
        if is_header && rows.len() > 1 {
            out.push_str("\x1b[90m├");
            for (i, w) in widths.iter().enumerate() {
                out.push_str(&"─".repeat(w + 2));
                if i < widths.len() - 1 {
                    out.push('┼');
                }
            }
            out.push_str("┤\x1b[0m\n");
        }
    }

    // Bottom border
    out.push_str("\x1b[90m└");
    for (i, w) in widths.iter().enumerate() {
        out.push_str(&"─".repeat(w + 2));
        if i < widths.len() - 1 {
            out.push('┴');
        }
    }
    out.push_str("┘\x1b[0m\n");

    out
}

/// Post-process rendered output to catch raw `[text](url)` Markdown link patterns
/// that pulldown-cmark didn't parse as Link events (e.g., multi-line links from HTML conversion).
fn fix_raw_links(
    text: &str,
    number_links: bool,
    counter: &mut usize,
    links: &mut Vec<String>,
) -> String {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '[' {
            // Skip images: ![alt](url)
            let is_image = i > 0 && chars[i - 1] == '!';
            if !is_image {
                let text_start = i + 1;
                let mut j = text_start;
                let mut bracket_depth = 1;

                while j < chars.len() && bracket_depth > 0 {
                    match chars[j] {
                        '[' => bracket_depth += 1,
                        ']' => bracket_depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }

                if bracket_depth == 0 {
                    let link_text_end = j - 1;
                    if j < chars.len() && chars[j] == '(' {
                        let url_start = j + 1;
                        let mut k = url_start;
                        while k < chars.len() && chars[k] != ')' {
                            k += 1;
                        }
                        if k < chars.len() && chars[k] == ')' {
                            let link_text: String = chars[text_start..link_text_end].iter().collect();
                            let trimmed = link_text.trim();
                            let url: String = chars[url_start..k].iter().collect();
                            let url_trimmed = url.trim();
                            let looks_like_url = url_trimmed.contains('.')
                                || url_trimmed.contains("://")
                                || url_trimmed.starts_with('/');
                            if looks_like_url
                                && (trimmed.is_empty()
                                    || !trimmed.chars().all(|c| c.is_ascii_digit()))
                            {
                                let display = if trimmed.is_empty() {
                                    fallback_link_label(url_trimmed)
                                } else {
                                    trimmed.to_string()
                                };
                                if number_links {
                                    *counter += 1;
                                    links.push(url_trimmed.to_string());
                                    result.push_str(&format!("\x1b[33m[{}]\x1b[0m ", *counter));
                                }
                                result.push_str("\x1b[4;36m");
                                result.push_str(&display);
                                result.push_str("\x1b[0m");
                                i = k + 1;
                                continue;
                            }
                        }
                    }
                }
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

/// Minimal stdio JSON-RPC loop for MCP
async fn run_stdio_mcp(server: &McpServer) -> Result<()> {
    use std::io::{self, BufRead};

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        let req: McpRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{{\"error\":\"{}\"}}", e);
                continue;
            }
        };

        match server.handle(req).await {
            Ok(resp) => {
                println!("{}", serde_json::to_string(&resp)?);
            }
            Err(e) => {
                eprintln!("{{\"error\":\"{}\"}}", e);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_url_absolute_unchanged() {
        assert_eq!(resolve_url("https://ibm.com", "https://example.com"), "https://example.com");
    }

    #[test]
    fn resolve_url_relative_joined() {
        assert_eq!(resolve_url("https://ibm.com/page", "/about"), "https://ibm.com/about");
        assert_eq!(resolve_url("https://ibm.com/page/", "about"), "https://ibm.com/page/about");
    }

    #[test]
    fn resolve_url_protocol_relative() {
        assert_eq!(resolve_url("https://ibm.com", "//cdn.com/file.js"), "https://cdn.com/file.js");
    }

    #[test]
    fn fix_raw_links_renders_multiline_links() {
        let input = "[\nExplore IBM\n~90%\nfaster\n](https://ibm.com)";
        let mut counter = 0;
        let mut links = Vec::new();
        let output = fix_raw_links(input, false, &mut counter, &mut links);
        assert!(output.contains("Explore IBM"));
        assert!(output.contains("~90%"));
        assert!(output.contains("faster"));
        assert!(!output.contains("]("));
        assert!(!output.contains("https://ibm.com"));
        assert!(output.contains("\x1b[4;36m"));
    }

    #[test]
    fn fix_raw_links_numbers_raw_links() {
        let input = "[\nExplore IBM\n](https://ibm.com)";
        let mut counter = 0;
        let mut links = Vec::new();
        let output = fix_raw_links(input, true, &mut counter, &mut links);
        assert_eq!(counter, 1);
        assert_eq!(links, vec!["https://ibm.com"]);
        assert!(output.contains("\x1b[33m[1]\x1b[0m"));
        assert!(output.contains("\x1b[4;36m"));
    }

    #[test]
    fn fix_raw_links_skips_plain_brackets() {
        let input = "Some [text] without a link";
        let mut counter = 0;
        let mut links = Vec::new();
        let output = fix_raw_links(input, false, &mut counter, &mut links);
        assert_eq!(output, "Some [text] without a link");
    }

    #[test]
    fn fix_raw_links_labels_empty_url_links() {
        let input = "[](https://example.com/case-studies/wimbledon)";
        let mut counter = 0;
        let mut links = Vec::new();
        let output = fix_raw_links(input, true, &mut counter, &mut links);
        assert_eq!(counter, 1);
        assert_eq!(links, vec!["https://example.com/case-studies/wimbledon"]);
        assert!(output.contains("wimbledon"));
    }

    #[test]
    fn fix_raw_links_skips_empty_href() {
        let input = "[not a url]()";
        let mut counter = 0;
        let mut links = Vec::new();
        let output = fix_raw_links(input, false, &mut counter, &mut links);
        assert_eq!(output, "[not a url]()");
    }

    #[test]
    fn fix_raw_links_skips_images() {
        let input = "![alt text](https://example.com/img.png)";
        let mut counter = 0;
        let mut links = Vec::new();
        let output = fix_raw_links(input, true, &mut counter, &mut links);
        assert_eq!(counter, 0);
        assert_eq!(output, "![alt text](https://example.com/img.png)");
    }

    #[test]
    fn fix_raw_links_skips_digit_only_text() {
        let input = "[1](https://example.com) [42](https://ibm.com)";
        let mut counter = 0;
        let mut links = Vec::new();
        let output = fix_raw_links(input, true, &mut counter, &mut links);
        assert_eq!(counter, 0);
        assert_eq!(output, "[1](https://example.com) [42](https://ibm.com)");
    }

    #[test]
    fn fix_raw_links_skips_non_url() {
        let input = "[note](see below)";
        let mut counter = 0;
        let mut links = Vec::new();
        let output = fix_raw_links(input, true, &mut counter, &mut links);
        assert_eq!(counter, 0);
        assert_eq!(output, "[note](see below)");
    }

    #[test]
    fn render_markdown_ansi_table() {
        let md = "| Name | Age |\n|------|-----|\n| Alice | 30 |\n| Bob | 25 |";
        let (output, _) = render_markdown_ansi(md, false);
        assert!(output.contains("┌"), "missing top-left corner");
        assert!(output.contains("┐"), "missing top-right corner");
        assert!(output.contains("└"), "missing bottom-left corner");
        assert!(output.contains("┘"), "missing bottom-right corner");
        assert!(output.contains("│"), "missing vertical bar");
        assert!(output.contains("Name"), "missing Name header");
        assert!(output.contains("Age"), "missing Age header");
        assert!(output.contains("Alice"), "missing Alice");
        assert!(output.contains("Bob"), "missing Bob");
    }

    #[test]
    fn url_to_filename_basic() {
        let name = url_to_filename("https://example.com/blog/post");
        assert_eq!(name, "example.com_blog_post.md");
    }

    #[test]
    fn url_to_filename_root() {
        let name = url_to_filename("https://example.com/");
        assert_eq!(name, "example.com_index.md");
    }

    #[test]
    fn url_to_filename_with_query() {
        let name = url_to_filename("https://example.com/search?q=rust&page=2");
        assert!(name.starts_with("example.com_search"));
        assert!(name.ends_with(".md"));
    }
}
