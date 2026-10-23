//! `fetch` subcommand: single-URL conversion across all output formats,
//! structured extractors, post-processing transforms, and the recursive
//! same-origin crawler (`--depth` / `--sitemap-only`).

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::time::Duration;
use url::Url;
use web2md::{
    extract_course, extract_event, extract_faq, extract_howto, extract_job, extract_metadata,
    extract_page_metadata, extract_qapage, extract_recipe, extract_software_application,
    extract_speakable, extract_summary, extract_topic, language_matches, normalize_crawl_url,
    path_allowed, remove_base64_images, to_atom_feed, to_llms_txt_site, truncate_by_tokens,
    truncate_with_marker, AtomEntry, Browser, ConvertOptions, LlmsPageEntry, PageToMarkdown,
};

use crate::ansi::render_markdown_ansi;
use crate::cli::{format_label, CliJsonOutput, FetchArgs, OutputFormat};
use crate::cli::url_to_filename;
use crate::options::{
    build_browser_options, filter_by_include_selectors, load_cookie_file, merge_include_selectors,
    post_webhook, MOBILE_USER_AGENT,
};

pub(crate) async fn handle(args: FetchArgs) -> Result<()> {
let mut cookies = args.cookie;
if let Some(ref path) = args.cookie_file {
    cookies.extend(load_cookie_file(path)?);
}
let options = build_browser_options(
    args.timeout,
    args.delay,
    args.cache_ttl,
    cookies,
    args.header,
    args.no_blacklist,
    args.no_user_blacklist,
    args.blacklist_file,
    args.ignore_robots,
    args.respect_robots,
);
let mut options = options;
if let Some(dir) = args.cache_dir {
    options.cache_dir = Some(std::path::PathBuf::from(dir));
}
options.no_cache = args.no_cache;
if let Some(max_age) = args.cache_max_age {
    options.cache_max_age = Some(Duration::from_secs(max_age));
}
options.host_rate_limit = args.rate;
options.skip_tls_verify = args.skip_tls_verify;
if args.mobile {
    options.user_agent = MOBILE_USER_AGENT.to_string();
}
if let Some(ref p) = args.proxy {
    options.proxy = Some(p.clone());
}
if let Some(ref a) = args.auth {
    options.basic_auth = Some(a.clone());
}
let browser = Browser::new(options)?;

if args.sitemap_only && args.depth == 0 {
    anyhow::bail!("--sitemap-only requires --depth > 0");
}
if args.depth > 0 {
    if !matches!(
        args.format,
        OutputFormat::Markdown | OutputFormat::LlmsTxt | OutputFormat::Atom
    ) {
        anyhow::bail!("--depth requires markdown, llms-txt, or atom output format");
    }
    let crawl_mode = match args.format {
        OutputFormat::LlmsTxt => CrawlMode::LlmsTxt,
        OutputFormat::Atom => CrawlMode::Atom,
        _ => CrawlMode::Markdown,
    };
    crawl_fetch(
        &browser,
        &args.url,
        args.depth,
        args.sitemap_only,
        args.max_pages,
        &args.path_prefix,
        &args.path_exclude,
        args.max_length,
        args.include_images,
        args.keep_header,
        args.main_content,
        args.frontmatter,
        &args.exclude_selector,
        &args.exclude_tags,
        args.output.as_deref(),
        args.render,
        args.remove_base64_images,
        crawl_mode,
    )
    .await?;
} else {
    // Determine whether we can use the streaming path:
    // plain markdown output with no post-processing transforms.
    let can_stream = matches!(args.format, OutputFormat::Markdown)
        && args.r#type.is_none()
        && args.topic.is_none()
        && args.summary.is_none()
        && args.max_tokens.is_none()
        && args.max_length.is_none()
        && !args.frontmatter
        && !args.pii_redact
        && args.output.is_none()
        && args.webhook.is_none()
        && !args.headless
        && !args.links_summary
        && !args.images_summary
        && !args.chunk;

    let html = if can_stream {
        // Streaming path: show download progress on stderr, then
        // emit Markdown blocks to stdout as they are converted.
        eprint!("\rFetching {} ...", args.url);
        let html = browser
            .fetch_stream(&args.url, |chunk| {
                eprint!("\rFetching {} ... {} bytes", args.url, chunk.len());
            })
            .await?;
        eprintln!("\rFetching {} ... done.            ", args.url);
        browser.prepare_html(&html, &args.url).await?
    } else if args.headless {
        let opts = web2md::HeadlessOptions {
            wait_ms: 0,
            chrome_path: args.chrome_path.clone(),
            wait_for_selector: args.wait_for_selector.clone(),
            scroll: args.scroll.unwrap_or(0),
        };
        web2md::render_url(&args.url, opts).await?
    } else {
        let html = browser.fetch(&args.url).await?;
        browser.prepare_html(&html, &args.url).await?
    };

    // Streaming Markdown output: emit blocks incrementally.
    if can_stream {
        let include_sels = merge_include_selectors(&args.include_selector, &args.include_tags);
        let html = filter_by_include_selectors(&html, &include_sels);
        #[cfg(feature = "readability")]
        let html = if args.readability {
            web2md::apply_readability(&html, Some(&args.url))?
        } else {
            html
        };
        let convert_opts = ConvertOptions {
            include_images: args.include_images,
            keep_header: args.keep_header,
            main_content: args.main_content,
            favor_precision: args.precision,
            favor_recall: args.recall,
            include_comments: !args.no_comments,
            include_tables: !args.no_tables,
            include_links: !args.no_links,
            exclude_tags: args.exclude_tags.clone(),
        };
        let mut first = true;
        PageToMarkdown::convert_progressive_with(
            &html,
            &convert_opts,
            &args.exclude_selector,
            |block| {
                if args.render {
                    let rendered = render_markdown_ansi(&block, false).0;
                    if first {
                        print!("{rendered}");
                        first = false;
                    } else {
                        print!("\n{rendered}");
                    }
                } else {
                    if first {
                        print!("{block}");
                        first = false;
                    } else {
                        print!("\n{block}");
                    }
                }
                use std::io::Write;
                let _ = std::io::stdout().flush();
            },
        )?;
        println!();
        return Ok(());
    }

    let include_sels = merge_include_selectors(&args.include_selector, &args.include_tags);
    let html = filter_by_include_selectors(&html, &include_sels);
    #[cfg(feature = "readability")]
    let html = if args.readability {
        web2md::apply_readability(&html, Some(&args.url))?
    } else {
        html
    };

    let format_label_value = format_label(&args.format);
    let (mut result, frontmatter_meta) = match args.format {
        OutputFormat::Branding => {
            let profile = web2md::extract_branding(&html);
            (serde_json::to_string_pretty(&profile)?, None)
        }
        OutputFormat::Links => {
            let links = web2md::extract_links(&html, &args.url);
            (serde_json::to_string_pretty(&links)?, None)
        }
        OutputFormat::Images => {
            let images = web2md::extract_images(&html, &args.url);
            (serde_json::to_string_pretty(&images)?, None)
        }
        OutputFormat::Product => {
            match web2md::extract_product(&html) {
                Some(product) => (serde_json::to_string_pretty(&product)?, None),
                None => anyhow::bail!("no JSON-LD Product found on this page"),
            }
        }
        OutputFormat::Video => {
            let videos = web2md::extract_videos(&html, &args.url);
            (serde_json::to_string_pretty(&videos)?, None)
        }
        OutputFormat::Audio => {
            let audios = web2md::extract_audios(&html, &args.url);
            (serde_json::to_string_pretty(&audios)?, None)
        }
        OutputFormat::Attributes => {
            if args.attr.is_empty() {
                anyhow::bail!(
                    "--args.format attributes requires at least one --args.attr selector:attribute (e.g. --args.attr a:href)"
                );
            }
            let results = web2md::extract_attributes(&html, &args.attr);
            (serde_json::to_string_pretty(&results)?, None)
        }
        OutputFormat::Menu => {
            match web2md::extract_menu(&html) {
                Some(menu) => (serde_json::to_string_pretty(&menu)?, None),
                None => anyhow::bail!("no JSON-LD Menu found on this page"),
            }
        }
        OutputFormat::Breadcrumbs => {
            match web2md::extract_breadcrumbs(&html, &args.url) {
                Some(list) => (serde_json::to_string_pretty(&list)?, None),
                None => anyhow::bail!("no JSON-LD BreadcrumbList found on this page"),
            }
        }
        OutputFormat::Meta => {
            let dump = web2md::extract_meta_dump(&html);
            (serde_json::to_string_pretty(&dump)?, None)
        }
        OutputFormat::LlmsTxt => {
            let meta = web2md::extract_metadata(&html);
            let links = web2md::extract_links(&html, &args.url);
            let pairs: Vec<(String, String)> =
                links.into_iter().map(|l| (l.url, l.text)).collect();
            let out = web2md::to_llms_txt(
                &args.url,
                meta.title.as_deref(),
                meta.description.as_deref(),
                &pairs,
            );
            (out, None)
        }
        OutputFormat::Atom => {
            let meta = web2md::extract_metadata(&html);
            let entry = AtomEntry {
                url: args.url.clone(),
                title: meta
                    .title
                    .clone()
                    .unwrap_or_else(|| args.url.clone()),
                summary: meta.description.clone(),
                updated: meta.published_date.clone(),
            };
            let out = to_atom_feed(
                &args.url,
                meta.site_name.as_deref().or(meta.title.as_deref()),
                &[entry],
            );
            (out, None)
        }
        OutputFormat::Html => {
            if args.lang.is_some() {
                anyhow::bail!("--args.lang requires a converted output args.format (not html)");
            }
            if args.only_with_metadata {
                anyhow::bail!(
                    "--only-with-metadata requires a converted output args.format (not html)"
                );
            }
            if args.r#type.is_some() {
                anyhow::bail!("--type requires a converted output args.format (not html)");
            }
            (html.clone(), None)
        }
        ref fmt => {
            let struct_result: Option<String> = match args.r#type.as_deref() {
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
                Some("howto") => match extract_howto(&html) {
                    Ok(Some(md)) => Some(md),
                    Ok(None) => None,
                    Err(e) => {
                        eprintln!("--type howto: {}, falling back", e);
                        None
                    }
                },
                Some("qapage") => match extract_qapage(&html) {
                    Ok(Some(md)) => Some(md),
                    Ok(None) => None,
                    Err(e) => {
                        eprintln!("--type qapage: {}, falling back", e);
                        None
                    }
                },
                Some("speakable") => match extract_speakable(&html) {
                    Ok(Some(md)) => Some(md),
                    Ok(None) => None,
                    Err(e) => {
                        eprintln!("--type speakable: {}, falling back", e);
                        None
                    }
                },
                Some("course") => match extract_course(&html) {
                    Ok(Some(md)) => Some(md),
                    Ok(None) => None,
                    Err(e) => {
                        eprintln!("--type course: {}, falling back", e);
                        None
                    }
                },
                Some("software") | Some("softwareapplication") => {
                    match extract_software_application(&html) {
                        Ok(Some(md)) => Some(md),
                        Ok(None) => None,
                        Err(e) => {
                            eprintln!("--type software: {}, falling back", e);
                            None
                        }
                    }
                }
                Some(other) => {
                    anyhow::bail!(
                        "unsupported --type {}; expected one of: recipe, faq, job, event, howto, qapage, speakable, course, software",
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
                        include_images: args.include_images,
                        keep_header: args.keep_header,
                        main_content: args.main_content,
                        favor_precision: args.precision,
                        favor_recall: args.recall,
                        include_comments: !args.no_comments,
                        include_tables: !args.no_tables,
                        include_links: !args.no_links,
                        exclude_tags: args.exclude_tags.clone(),
                    };
                    let body = PageToMarkdown::convert_with(
                        &html,
                        &convert_opts,
                        &args.exclude_selector,
                    )?;
                    PageToMarkdown::absolutize_links(&body, &args.url)
                }
            };
            let meta = extract_page_metadata(&html, &md);
            if let Some(ref target) = args.lang
                && !language_matches(meta.language.as_deref(), target) {
                    anyhow::bail!(
                        "page language {:?} does not match --args.lang {}",
                        meta.language.as_deref().unwrap_or("(unknown)"),
                        target
                    );
                }
            if args.only_with_metadata
                && (meta.title.is_none() || meta.published_date.is_none())
            {
                anyhow::bail!(
                    "--only-with-metadata requires title and published_date; found title={:?} published_date={:?}",
                    meta.title.as_deref().unwrap_or("(missing)"),
                    meta.published_date.as_deref().unwrap_or("(missing)")
                );
            }
            if !from_struct {
                if let Some(ref t) = args.topic {
                    md = match extract_topic(&md, t, None) {
                        Some(f) => PageToMarkdown::absolutize_links(&f, &args.url),
                        None => anyhow::bail!(
                            "--args.topic: no paragraphs matched query {:?}",
                            t
                        ),
                    };
                }
                if let Some(sentences) = args.summary {
                    md = match extract_summary(&md, sentences, meta.title.as_deref()) {
                        Some(s) => s,
                        None => md,
                    };
                }
            }
            let fm_meta = matches!(fmt, OutputFormat::Markdown | OutputFormat::Text)
                .then(|| meta.clone());
            let out = match fmt {
                OutputFormat::Markdown => {
                    if args.render {
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
                    meta.to_csv(&args.url, &text)
                }
                OutputFormat::Tei => {
                    let text = PageToMarkdown::to_plain_text(&md);
                    meta.to_tei(&args.url, &text)
                }
                OutputFormat::Xml => {
                    let text = PageToMarkdown::to_plain_text(&md);
                    meta.to_xml(&args.url, &text)
                }
                OutputFormat::Html => unreachable!(),
                OutputFormat::Branding => unreachable!(),
                OutputFormat::Links => unreachable!(),
                OutputFormat::Images => unreachable!(),
                OutputFormat::Product => unreachable!(),
                OutputFormat::Video => unreachable!(),
                OutputFormat::Audio => unreachable!(),
                OutputFormat::Attributes => unreachable!(),
                OutputFormat::Menu => unreachable!(),
                OutputFormat::Breadcrumbs => unreachable!(),
                OutputFormat::Meta => unreachable!(),
                OutputFormat::LlmsTxt => unreachable!(),
                OutputFormat::Atom => unreachable!(),
            };
            (out, fm_meta)
        }
    };

    if args.frontmatter
        && let Some(meta) = frontmatter_meta
            && let Some(fm) = meta.to_frontmatter(Some(&args.url)) {
                result = format!("{}{}", fm, result);
            }

    if args.links_summary && matches!(args.format, OutputFormat::Markdown) {
        let links = web2md::extract_links(&html, &args.url);
        if !links.is_empty() {
            let mut links_section = String::from("\n\n---\n\n## Links\n\n");
            for link in &links {
                let label = if link.text.is_empty() { &link.url } else { &link.text };
                links_section.push_str(&format!("- [{}]({})\n", label, link.url));
            }
            result.push_str(&links_section);
        }
    }

    if args.images_summary && matches!(args.format, OutputFormat::Markdown) {
        let images = web2md::extract_images(&html, &args.url);
        if !images.is_empty() {
            let mut images_section = String::from("\n\n---\n\n## Images\n\n");
            for img in &images {
                let alt = img.alt.as_deref().unwrap_or("");
                images_section.push_str(&format!("- ![{}]({})\n", alt, img.src));
            }
            result.push_str(&images_section);
        }
    }

    if args.chunk && matches!(args.format, OutputFormat::Markdown) {
        result = chunk_markdown_by_headings(&result);
    }

    if let Some(max) = args.max_tokens {
        result = truncate_by_tokens(&result, max);
    } else if let Some(max) = args.max_length {
        result = truncate_with_marker(&result, max);
    }

    if args.remove_base64_images {
        result = remove_base64_images(&result);
    }

    if args.pii_redact {
        result = web2md::redact_pii(&result);
    }

    if let Some(hook) = args.webhook.as_deref() {
        let payload = serde_json::json!({
            "event": "fetch.completed",
            "args.url": args.url,
            "args.format": format_label_value,
            "result": result,
        });
        if let Err(e) = post_webhook(hook, &payload.to_string()).await {
            eprintln!("webhook POST to {} failed: {}", hook, e);
        }
    }

    if let Some(path) = args.output {
        std::fs::write(&path, &result)?;
        eprintln!("Written to {}", path);
    } else {
        println!("{}", result);
    }
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum CrawlMode {
    Markdown,
    LlmsTxt,
    Atom,
}

/// Recursively fetch and convert same-origin pages up to `depth` link hops.
/// Pages at each BFS level are fetched in parallel (up to 10 concurrent).
#[allow(clippy::too_many_arguments)]
async fn crawl_fetch(
    browser: &Browser,
    start_url: &str,
    depth: u32,
    sitemap_only: bool,
    max_pages: Option<usize>,
    path_prefix: &[String],
    path_exclude: &[String],
    max_length: Option<usize>,
    include_images: bool,
    keep_header: bool,
    main_content: bool,
    frontmatter: bool,
    exclude_selector: &[String],
    exclude_tags: &[String],
    output_dir: Option<&str>,
    render: bool,
    strip_base64_images: bool,
    mode: CrawlMode,
) -> Result<()> {
    use std::sync::Arc;
    use tokio::sync::Semaphore;
    use tokio::task::JoinSet;

    const CONCURRENCY: usize = 10;

    let root = Url::parse(start_url).context("Invalid URL")?;

    if let Some(dir) = output_dir {
        std::fs::create_dir_all(dir)?;
    }

    let sem = Arc::new(Semaphore::new(CONCURRENCY));
    let mut visited = HashSet::new();
    let mut llms_pages: Vec<LlmsPageEntry> = Vec::new();
    let mut atom_entries: Vec<AtomEntry> = Vec::new();
    let mut site_title: Option<String> = None;
    let mut site_description: Option<String> = None;
    let aggregate = matches!(mode, CrawlMode::LlmsTxt | CrawlMode::Atom);

    // For sitemap-only mode, fetch sitemap.xml and use those URLs as the initial set.
    let mut current_level: Vec<String> = if sitemap_only {
        let sitemap_url = format!(
            "{}://{}/sitemap.xml",
            root.scheme(),
            root.host_str().unwrap_or("")
        );
        eprintln!("Fetching sitemap: {}", sitemap_url);
        match browser.fetch(&sitemap_url).await {
            Ok(xml) => {
                let urls: Vec<String> = browser
                    .expand_sitemap(&sitemap_url, &xml)
                    .await
                    .into_iter()
                    .filter(|u| path_allowed(u, path_prefix, path_exclude))
                    .collect();
                eprintln!("Sitemap returned {} URL(s)", urls.len());
                if urls.is_empty() {
                    return Ok(());
                }
                urls
            }
            Err(e) => {
                anyhow::bail!("Failed to fetch sitemap.xml: {}", e);
            }
        }
    } else {
        let start = normalize_crawl_url(start_url, start_url)
            .unwrap_or_else(|| start_url.to_string());
        vec![start]
    };
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    for level in 0..=depth {
        // Filter out visited and blacklisted URLs.
        let mut to_fetch = Vec::new();
        for url in current_level.drain(..) {
            let key = normalize_crawl_url(&url, &url).unwrap_or_else(|| url.clone());
            if !visited.insert(key) {
                continue;
            }
            if browser.is_url_blocked(&url) {
                eprintln!("Skipped (blacklisted): {}", url);
                skipped += 1;
                continue;
            }
            if !path_allowed(&url, path_prefix, path_exclude) {
                eprintln!("Skipped (path filter): {}", url);
                skipped += 1;
                continue;
            }
            to_fetch.push(url);
        }

        if to_fetch.is_empty() {
            break;
        }

        if let Some(max) = max_pages {
            let remaining = max.saturating_sub(succeeded);
            if remaining == 0 {
                eprintln!("Reached --max-pages {}", max);
                break;
            }
            if to_fetch.len() > remaining {
                eprintln!(
                    "Capping crawl at {} remaining page(s) (--max-pages {})",
                    remaining, max
                );
                to_fetch.truncate(remaining);
            }
        }

        eprintln!("[depth {}] {} URL(s)", level, to_fetch.len());

        // Spawn parallel fetch tasks for this level.
        let mut tasks: JoinSet<(String, anyhow::Result<(String, String)>)> = JoinSet::new();
        for url in to_fetch {
            let browser = browser.clone();
            let sem = sem.clone();
            tasks.spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                eprintln!("  Fetching {}", url);
                let result = async {
                    let html = browser.fetch(&url).await?;
                    let html = match browser.prepare_html(&html, &url).await {
                        Ok(prepared) => prepared,
                        Err(_) => html,
                    };
                    Ok::<_, anyhow::Error>((html, url.clone()))
                }
                .await;
                (url, result)
            });
        }

        // Collect results and discover links for the next level.
        let mut next_level = Vec::new();
        while let Some(res) = tasks.join_next().await {
            let (url, outcome) = res.unwrap();
            match outcome {
                Ok((html, _)) => {
                    if level < depth && !sitemap_only {
                        for link in browser.same_origin_links(&html, &url, &root) {
                            if !path_allowed(&link, path_prefix, path_exclude) {
                                continue;
                            }
                            let link_key =
                                normalize_crawl_url(&link, &link).unwrap_or_else(|| link.clone());
                            if !visited.contains(&link_key) {
                                next_level.push(link);
                            }
                        }
                    }

                    if aggregate {
                        let meta = extract_metadata(&html);
                        if site_title.is_none() {
                            site_title = meta
                                .site_name
                                .clone()
                                .or_else(|| meta.title.clone());
                        }
                        if site_description.is_none() {
                            site_description = meta.description.clone();
                        }
                        let title = meta
                            .title
                            .clone()
                            .filter(|t| !t.trim().is_empty())
                            .unwrap_or_else(|| url.clone());
                        match mode {
                            CrawlMode::LlmsTxt => {
                                llms_pages.push(LlmsPageEntry {
                                    url: url.clone(),
                                    title,
                                    description: meta.description,
                                });
                            }
                            CrawlMode::Atom => {
                                atom_entries.push(AtomEntry {
                                    url: url.clone(),
                                    title,
                                    summary: meta.description,
                                    updated: meta.published_date,
                                });
                            }
                            CrawlMode::Markdown => unreachable!(),
                        }
                        eprintln!("  → indexed {}", url);
                        succeeded += 1;
                    } else {
                        let convert_opts = ConvertOptions {
                            include_images,
                            keep_header,
                            main_content,
                            exclude_tags: exclude_tags.to_vec(),
                            ..Default::default()
                        };
                        match PageToMarkdown::convert_with(&html, &convert_opts, exclude_selector)
                        {
                            Ok(md) => {
                                let mut md = PageToMarkdown::absolutize_links(&md, &url);
                                if strip_base64_images {
                                    md = remove_base64_images(&md);
                                }
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
                                eprintln!("  Error converting {}: {}", url, e);
                                failed += 1;
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("  Error fetching {}: {}", url, e);
                    failed += 1;
                }
            }
        }

        current_level = next_level;
    }

    match mode {
        CrawlMode::LlmsTxt => {
            let out = to_llms_txt_site(
                start_url,
                site_title.as_deref(),
                site_description.as_deref(),
                &llms_pages,
                "Pages",
            );
            if let Some(dir) = output_dir {
                let path = format!("{}/llms.txt", dir);
                std::fs::write(&path, &out)?;
                eprintln!("  → {}", path);
            } else {
                println!("{}", out);
            }
        }
        CrawlMode::Atom => {
            let out = to_atom_feed(start_url, site_title.as_deref(), &atom_entries);
            if let Some(dir) = output_dir {
                let path = format!("{}/feed.atom", dir);
                std::fs::write(&path, &out)?;
                eprintln!("  → {}", path);
            } else {
                println!("{}", out);
            }
        }
        CrawlMode::Markdown => {}
    }

    eprintln!(
        "\nCrawl done: {} succeeded, {} failed, {} skipped",
        succeeded, failed, skipped
    );
    Ok(())
}

/// Split Markdown by heading lines, inserting a separator between sections.
/// Each section starts with its heading. Sections are separated by `\n\n---\n\n`.
pub(crate) fn chunk_markdown_by_headings(md: &str) -> String {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in md.lines() {
        if line.starts_with('#') && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    chunks.join("\n---\n\n")
}