use anyhow::{Context, Result};
use url::Url;
use web2md::{extract_metadata, filter_blacklisted_urls, parse_sitemap_urls, same_origin_links, Browser, BrowserOptions, McpRequest, McpServer, PageToMarkdown};
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
}

/// Structured JSON output for `--format json` CLI flag.
#[derive(Debug, Serialize)]
struct CliJsonOutput {
    markdown: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    published_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    headline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    site_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    keywords: Option<Vec<String>>,
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
        /// Output format: markdown or html
        #[arg(short, long, value_enum, default_value = "markdown")]
        format: OutputFormat,
        /// Render Markdown with ANSI colors and formatting in the terminal
        #[arg(short, long)]
        render: bool,
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
        /// Disable URL blacklist filtering for ads/tracking pixels
        #[arg(long)]
        no_blacklist: bool,
        /// Recursively crawl same-origin links up to N levels deep (markdown output only)
        #[arg(long, default_value = "0")]
        depth: u32,
    },
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
        /// Disable URL blacklist filtering for ads/tracking pixels
        #[arg(long)]
        no_blacklist: bool,
    },
    /// Run as an MCP server (stdio JSON-RPC)
    Mcp,
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
        /// Disable URL blacklist filtering for ads/tracking pixels
        #[arg(long)]
        no_blacklist: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    web2md::init()?;
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
            delay,
            keep_header,
            cache_ttl,
            main_content,
            output: output_file,
            frontmatter,
            exclude_selector,
            javascript,
            no_blacklist,
            depth,
        }) => {
            let mut options = BrowserOptions::default();
            if let Some(secs) = timeout {
                options.timeout = Duration::from_secs(secs);
            }
            if let Some(ms) = delay {
                options.request_delay = Duration::from_millis(ms);
            }
            if let Some(secs) = cache_ttl {
                options.cache_ttl = Duration::from_secs(secs);
            }
            options.cookies = cookie;
            options.headers = header;
            options.enable_javascript = javascript;
            options.filter_blacklisted_urls = !no_blacklist;
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
                let html = browser.inline_iframes(&html, &url).await?;
                let html = browser.run_inline_scripts(&html);

                let mut result = match format {
                    OutputFormat::Markdown => {
                        let md = PageToMarkdown::convert(&html, include_images, keep_header, main_content, &exclude_selector)?;
                        let md = PageToMarkdown::absolutize_links(&md, &url);
                        if render {
                            render_markdown_ansi(&md, false).0
                        } else {
                            md
                        }
                    }
                    OutputFormat::Html => html.clone(),
                    OutputFormat::Json => {
                        let md = PageToMarkdown::convert(&html, include_images, keep_header, main_content, &exclude_selector)?;
                        let md = PageToMarkdown::absolutize_links(&md, &url);
                        let meta = extract_metadata(&html);
                        let output = CliJsonOutput {
                            markdown: md,
                            title: meta.title,
                            description: meta.description,
                            author: meta.author,
                            published_date: meta.published_date,
                            image: meta.image,
                            headline: meta.headline,
                            site_name: meta.site_name,
                            keywords: meta.keywords,
                        };
                        serde_json::to_string_pretty(&output)?
                    }
                };

                if frontmatter && matches!(format, OutputFormat::Markdown) {
                    let meta = extract_metadata(&html);
                    if let Some(fm) = meta.to_frontmatter(Some(&url)) {
                        result = format!("{}{}", fm, result);
                    }
                }

                if let Some(max) = max_length {
                    if result.len() > max {
                        result = format!("{}\n\n[truncated]", &result[..max]);
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
            no_blacklist,
        }) => {
            let mut options = BrowserOptions::default();
            if let Some(secs) = timeout {
                options.timeout = Duration::from_secs(secs);
            }
            if let Some(ms) = delay {
                options.request_delay = Duration::from_millis(ms);
            }
            if let Some(secs) = cache_ttl {
                options.cache_ttl = Duration::from_secs(secs);
            }
            options.cookies = cookie;
            options.headers = header;
            options.enable_javascript = javascript;
            options.filter_blacklisted_urls = !no_blacklist;
            browse_loop(url, options, include_images, keep_header, main_content).await?;
        }
        Some(Commands::Mcp) => {
            let server = McpServer::new()?;
            run_stdio_mcp(&server).await?;
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
                    let sitemap_urls = filter_blacklisted_urls(parse_sitemap_urls(&xml));
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
                        let feed_urls = web2md::extract_feed_links(&html);
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
            no_blacklist,
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

            let mut options = BrowserOptions::default();
            if let Some(secs) = timeout {
                options.timeout = Duration::from_secs(secs);
            }
            if let Some(ms) = delay {
                options.request_delay = Duration::from_millis(ms);
            }
            if let Some(secs) = cache_ttl {
                options.cache_ttl = Duration::from_secs(secs);
            }
            options.cookies = cookie;
            options.headers = header;
            options.enable_javascript = javascript;
            options.filter_blacklisted_urls = !no_blacklist;
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

                match browser.fetch(url).await {
                    Ok(html) => {
                        let html = match browser.inline_iframes(&html, url).await {
                            Ok(inlined) => inlined,
                            Err(_) => html,
                        };
                        let html = browser.run_inline_scripts(&html);
                        match PageToMarkdown::convert(&html, include_images, keep_header, main_content, &exclude_selector) {
                            Ok(md) => {
                                let md = PageToMarkdown::absolutize_links(&md, url);
                                let md = if frontmatter {
                                    let meta = extract_metadata(&html);
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
    let start = web2md::normalize_crawl_url(start_url, start_url)
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
        let key = web2md::normalize_crawl_url(&url, &url).unwrap_or_else(|| url.clone());
        if !visited.insert(key) {
            continue;
        }

        if browser.is_url_blocked(&url) {
            eprintln!("Skipped (blacklisted): {}", url);
            skipped += 1;
            continue;
        }

        eprintln!("[depth {}] {}", level, url);

        match browser.fetch(&url).await {
            Ok(html) => {
                let html = match browser.inline_iframes(&html, &url).await {
                    Ok(inlined) => inlined,
                    Err(_) => html,
                };
                let html = browser.run_inline_scripts(&html);

                if level < depth {
                    for link in same_origin_links(&html, &url, &root) {
                        let link_key =
                            web2md::normalize_crawl_url(&link, &link).unwrap_or(link.clone());
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
                            let meta = extract_metadata(&html);
                            if let Some(fm) = meta.to_frontmatter(Some(&url)) {
                                md = format!("{}{}", fm, md);
                            }
                        }
                        if let Some(max) = max_length {
                            if md.len() > max {
                                md = format!("{}\n\n[truncated]", &md[..max]);
                            }
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

        let html = match browser.fetch(&url).await {
            Ok(h) => match browser.inline_iframes(&h, &url).await {
                Ok(inlined) => inlined,
                Err(_) => h,
            },
            Err(e) => {
                println!("\x1b[91mError: {}\x1b[0m", e);
                println!("\nPress Enter to continue...");
                let mut _buf = String::new();
                let _ = stdin_lock.read_line(&mut _buf);
                continue;
            }
        };
        let html = browser.run_inline_scripts(&html);

        let md = PageToMarkdown::convert(&html, include_images, keep_header, main_content, &[])?;
        let md = PageToMarkdown::absolutize_links(&md, &url);
        let (rendered, links) = render_markdown_ansi(&md, true);
        println!("{}", rendered);

        println!(
            "\n\x1b[90m[q]uit [b]ack [f]orward [u]rl [1-{}] follow link\x1b[0m",
            links.len().min(20)
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
                    if n > 0 && n <= links.len().min(20) {
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

/// Render Markdown with ANSI escape codes for terminal display.
/// Markdown syntax is stripped; visual effects (bold, color, underline) replace it.
fn render_markdown_ansi(md: &str, number_links: bool) -> (String, Vec<String>) {
    use pulldown_cmark::{Alignment, Event, Options, Tag, TagEnd};

    let mut out = String::new();
    let mut link_counter: usize = 0;
    let mut links: Vec<String> = Vec::new();
    let mut pending_link: Option<(usize, String)> = None;

    // Table buffering state
    let mut in_table = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell = String::new();
    let mut col_alignments: Vec<Alignment> = Vec::new();
    let mut _in_header = false;

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
                    out.push_str(c);
                }
                Tag::Strong => out.push_str("\x1b[1m"),
                Tag::Emphasis => out.push_str("\x1b[3m"),
                Tag::Link { dest_url, .. } => {
                    if number_links && !in_table {
                        link_counter += 1;
                        pending_link = Some((link_counter, dest_url.to_string()));
                    }
                    if in_table {
                        current_cell.push_str("\x1b[4;36m");
                    } else {
                        out.push_str("\x1b[4;36m");
                    }
                }
                Tag::BlockQuote(_) => out.push_str("\x1b[90m▌ \x1b[3m"),
                Tag::CodeBlock(_) => out.push_str("\x1b[48;5;235;38;5;250m"),
                Tag::List(_) => {}
                Tag::Item => out.push_str("  • "),
                Tag::Table(aligns) => {
                    in_table = true;
                    col_alignments = aligns.to_vec();
                }
                Tag::TableHead => _in_header = true,
                Tag::TableRow => current_row = Vec::new(),
                Tag::TableCell => current_cell.clear(),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => out.push_str("\x1b[0m\n"),
                TagEnd::Paragraph => {
                    if in_table {
                        // strip trailing newline inside cells
                    } else {
                        out.push('\n');
                    }
                }
                TagEnd::Strong | TagEnd::Emphasis => {
                    if in_table {
                        current_cell.push_str("\x1b[0m");
                    } else {
                        out.push_str("\x1b[0m");
                    }
                }
                TagEnd::Link => {
                    if in_table {
                        current_cell.push_str("\x1b[0m");
                    } else {
                        out.push_str("\x1b[0m");
                        if let Some((n, _)) = pending_link.take() {
                            link_counter = n - 1;
                        }
                    }
                }
                TagEnd::BlockQuote(_) => out.push_str("\x1b[0m\n"),
                TagEnd::CodeBlock => out.push_str("\x1b[0m\n"),
                TagEnd::Item => {}
                TagEnd::TableCell => {
                    current_row.push(std::mem::take(&mut current_cell));
                }
                TagEnd::TableRow => {
                    table_rows.push(std::mem::take(&mut current_row));
                }
                TagEnd::TableHead => {
                    _in_header = false;
                    // pulldown-cmark omits TableRow/TableRowEnd inside TableHead,
                    // so flush any buffered cells as a header row
                    if !current_row.is_empty() {
                        table_rows.push(std::mem::take(&mut current_row));
                    }
                }
                TagEnd::Table => {
                    in_table = false;
                    out.push_str(&render_ansi_table(&table_rows, &col_alignments));
                    table_rows.clear();
                    col_alignments.clear();
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_table {
                    current_cell.push_str(&text);
                } else {
                    if let Some((n, url)) = pending_link.take() {
                        links.push(url);
                        out.push_str(&format!("\x1b[33m[{}]\x1b[0m ", n));
                    }
                    out.push_str(&text);
                }
            }
            Event::Code(code) => {
                let s = format!("\x1b[38;5;250m{}\x1b[0m", code);
                if in_table {
                    current_cell.push_str(&s);
                } else {
                    out.push_str(&s);
                }
            }
            Event::Html(html) => {
                if in_table {
                    current_cell.push_str(&html);
                } else {
                    out.push_str(&html);
                }
            }
            Event::SoftBreak => {
                if in_table {
                    current_cell.push(' ');
                } else {
                    out.push(' ');
                }
            }
            Event::HardBreak => {
                if in_table {
                    current_cell.push('\n');
                } else {
                    out.push('\n');
                }
            }
            Event::Rule => out.push_str("\x1b[90m────────────────────────────────────────\x1b[0m\n"),
            _ => {}
        }
    }

    out = fix_raw_links(&out, number_links, &mut link_counter, &mut links);
    (out, links)
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
/// that pulldown-cmark didn't parse as Link events (e.g., multi-line links from html2md).
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
                            // Skip empty text, pure digits (already numbered or footnotes),
                            // and URLs that don't look like URLs
                            if !trimmed.is_empty()
                                && !trimmed.chars().all(|c| c.is_ascii_digit())
                            {
                                let url: String = chars[url_start..k].iter().collect();
                                let url_trimmed = url.trim();
                                if url_trimmed.contains('.')
                                    || url_trimmed.contains("://")
                                    || url_trimmed.starts_with('/')
                                {
                                    if number_links {
                                        *counter += 1;
                                        links.push(url_trimmed.to_string());
                                        result.push_str(&format!("\x1b[33m[{}]\x1b[0m ", *counter));
                                    }
                                    result.push_str("\x1b[4;36m");
                                    result.push_str(trimmed);
                                    result.push_str("\x1b[0m");
                                    i = k + 1;
                                    continue;
                                }
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
    fn fix_raw_links_skips_empty_link_text() {
        let input = "[](https://example.com)";
        let mut counter = 0;
        let mut links = Vec::new();
        let output = fix_raw_links(input, false, &mut counter, &mut links);
        assert_eq!(output, "[](https://example.com)");
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
