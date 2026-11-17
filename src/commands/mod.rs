//! Subcommand dispatch: routes parsed [`Cli`] input to per-command handlers.
//!
//! Small command arms stay inline; larger commands (`fetch`, `batch`) live in
//! their own submodules.

use anyhow::{Context, Result};
use clap::Parser;
use std::time::Duration;
use url::Url;
use web2md::{
    Browser, BrowserOptions, McpRequest, McpServer, PageToMarkdown, extract_page_metadata,
    parse_sitemap_urls,
};

pub(crate) mod batch;
pub(crate) mod fetch;

use crate::cli::{Cli, Commands, CorpusAction};
use crate::options::base_options;
use crate::tui::browse_loop;
use crate::watch::{
    load_watch_body, load_watch_state, poll_once, save_watch_body, save_watch_state,
    unix_secs_string,
};

pub(crate) async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        None => {
            if let Some(url) = cli.url {
                let options = BrowserOptions::default();
                browse_loop(url, options, false, false, false).await?;
            } else {
                Cli::parse_from(["web2md", "--help"]);
            }
        }
                Some(Commands::Browse(args)) => {
            let options = args.common.browser_options()?;
            browse_loop(args.url, options, args.common.include_images, args.common.keep_header, args.common.main_content).await?;
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
            let options = base_options(timeout, cookie, header);
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
        Some(Commands::Watch {
            url,
            every,
            timeout,
            cookie,
            header,
            cache_dir,
            diff,
            webhook,
        }) => {
            use std::time::Duration as StdDuration;
            let options = base_options(timeout, cookie, header);
            // For watch, we deliberately bypass the persistent cache during the
            // comparison fetch so each tick sees the live page, then store the
            // resulting fingerprint in a sibling state file.
            let state_path = cache_dir.as_deref().map(std::path::Path::new);
            let browser = Browser::new(options)?;
            let mut last_fp: Option<String> = load_watch_state(state_path, &url)?;
            let mut last_body: Option<String> = if diff || webhook.is_some() {
                load_watch_body(state_path, &url)?
            } else {
                None
            };
            let interval = StdDuration::from_secs(every.max(1));
            // First fetch happens immediately.
            loop {
                match poll_once(&browser, &url).await {
                    Ok((fp, body)) => {
                        if last_fp.as_deref() != Some(fp.as_str()) {
                            let ts = unix_secs_string();
                            let patch = last_body.as_ref().map(|prev| {
                                web2md::diff_markdown(
                                    &format!("previous ({url})"),
                                    prev,
                                    &format!("current ({url})"),
                                    &body,
                                )
                            });
                            if diff {
                                if let Some(ref patch) = patch {
                                    println!("# watch change {}\t{}\t{}", ts, url, fp);
                                    println!("{}", patch);
                                } else {
                                    println!(
                                        "# watch baseline {}\t{}\t{}\n{}",
                                        ts, url, fp, body
                                    );
                                }
                            } else {
                                println!(
                                    "{}\t{}\t{}\t{}",
                                    ts,
                                    url,
                                    fp,
                                    body.chars().take(80).collect::<String>()
                                );
                            }
                            if let Some(ref hook) = webhook {
                                let payload = serde_json::json!({
                                    "event": "watch.change",
                                    "url": url,
                                    "timestamp": ts,
                                    "fingerprint": fp,
                                    "snippet": body.chars().take(200).collect::<String>(),
                                    "diff": patch,
                                    "baseline": last_body.is_none(),
                                });
                                if let Err(e) =
                                    crate::options::post_webhook(hook, &payload.to_string()).await
                                {
                                    eprintln!("watch webhook POST to {} failed: {}", hook, e);
                                }
                            }
                            last_fp = Some(fp.clone());
                            last_body = Some(body.clone());
                            let _ = save_watch_state(state_path, &url, &fp);
                            if diff || webhook.is_some() {
                                let _ = save_watch_body(state_path, &url, &body);
                            }
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
            no_blacklist,
            json,
            proxy,
            auth,
        }) => {
            let mut options = base_options(timeout, cookie, header);
            if let Some(ms) = delay {
                options.request_delay = Duration::from_millis(ms);
            }
            options.filter_blacklisted_urls = !no_blacklist;
            if let Some(ref p) = proxy {
                options.proxy = Some(p.clone());
            }
            if let Some(ref a) = auth {
                options.basic_auth = Some(a.clone());
            }
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
        }) => {
            let options = base_options(timeout, cookie, header);
            let browser = Browser::new(options)?;

            let parsed = Url::parse(&url).context("Invalid URL")?;
            let sitemap_url = format!(
                "{}/sitemap.xml",
                parsed.origin().unicode_serialization()
            );

            match browser.fetch(&sitemap_url).await {
                Ok(xml) => {
                    let sitemap_urls = browser.expand_sitemap(&sitemap_url, &xml).await;
                    if !sitemap_urls.is_empty() {
                        println!("# Sitemap URLs from {}\n", sitemap_url);
                        for u in &sitemap_urls {
                            println!("{}", u);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("No sitemap.xml found: {}", e);
                }
            }
        }
        Some(Commands::Map {
            url,
            timeout,
            cookie,
            header,
            same_origin,
            search,
            limit,
            sitemap,
            json,
        }) => {
            let options = base_options(timeout, cookie, header);
            let browser = Browser::new(options)?;
            let html = browser.fetch(&url).await?;
            let html = browser.prepare_html(&html, &url).await?;
            let mut links = web2md::extract_links(&html, &url);
            if same_origin {
                let base = Url::parse(&url).ok();
                if let Some(base) = &base {
                    links.retain(|l| {
                        Url::parse(&l.url)
                            .map(|parsed| {
                                parsed.scheme() == base.scheme()
                                    && parsed.host_str() == base.host_str()
                            })
                            .unwrap_or(false)
                    });
                }
            }
            if let Some(q) = &search {
                filter_map_links(&mut links, q);
            }
            if sitemap {
                let base = Url::parse(&url).ok();
                if let Some(base) = &base {
                    let sitemap_url = format!(
                        "{}/sitemap.xml",
                        base.origin().unicode_serialization()
                    );
                    if let Ok(xml) = browser.fetch(&sitemap_url).await {
                        for u in parse_sitemap_urls(&xml) {
                            if !links.iter().any(|l| l.url == u) {
                                links.push(web2md::LinkEntry {
                                    url: u,
                                    text: String::new(),
                                });
                            }
                        }
                    }
                }
            }
            let mut urls: Vec<&str> = links.iter().map(|l| l.url.as_str()).collect();
            if let Some(max) = limit {
                urls.truncate(max);
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&urls)?);
            } else {
                for u in &urls {
                    println!("{}", u);
                }
            }
        }
        Some(Commands::Search {
            query,
            timeout,
            limit,
            json,
            fetch,
            include_domains,
            exclude_domains,
            cookie,
            header,
        }) => {
            let options = base_options(timeout, cookie, header);
            let browser = Browser::new(options)?;

            let search_url = web2md::ddg_search_url(&query, limit);
            let html = browser.fetch(&search_url).await.context("Failed to fetch search results")?;
            let mut results = web2md::parse_ddg_results(&html);
            if !include_domains.is_empty() {
                results.retain(|r| domain_matches_any(&r.url, &include_domains));
            }
            if !exclude_domains.is_empty() {
                results.retain(|r| !domain_matches_any(&r.url, &exclude_domains));
            }
            if let Some(max) = limit {
                results.truncate(max);
            }

            if fetch {
                // Fetch and convert each result URL to Markdown
                for (i, r) in results.iter().enumerate() {
                    if let Ok(page_html) = browser.fetch(&r.url).await {
                        let page_html = browser.prepare_html(&page_html, &r.url).await?;
                        let md = PageToMarkdown::convert(&page_html, false, false, false, &[])?;
                        let md = PageToMarkdown::absolutize_links(&md, &r.url);
                        if json {
                            let out = serde_json::json!({
                                "index": i + 1,
                                "title": r.title,
                                "url": r.url,
                                "snippet": r.snippet,
                                "markdown": md,
                            });
                            println!("{}", serde_json::to_string_pretty(&out)?);
                        } else {
                            println!("---\n## {}. [{}]({})\n\n{}\n", i + 1, r.title, r.url, md);
                        }
                    }
                }
            } else if json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                let md = web2md::results_to_markdown(&results);
                println!("{}", md);
            }
        }
        Some(Commands::Corpus { action }) => {
            match action {
                CorpusAction::Index { dir, output } => {
                    let dir_path = std::path::PathBuf::from(&dir);
                    let out_path = output.as_deref().map(std::path::Path::new);
                    let n = web2md::build_index(&dir_path, out_path)?;
                    let final_path = out_path
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| web2md::index_path_for(&dir_path));
                    eprintln!("Indexed {} files → {}", n, final_path.display());
                }
                CorpusAction::Query { dir, query, limit, json } => {
                    let dir_path = std::path::PathBuf::from(&dir);
                    let hits = web2md::query_index(&dir_path, &query, limit)?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&hits)?);
                    } else {
                        println!("{}", web2md::corpus_results_to_markdown(&hits));
                    }
                }
            }
        }
        Some(Commands::Docs {
            name,
            registry,
            timeout,
            json,
        }) => {
            let reg = web2md::Registry::parse(&registry)
                .with_context(|| format!("Unknown registry '{}'. Use: crates, npm, or pypi", registry))?;
            let options = base_options(timeout, Vec::new(), Vec::new());
            let browser = Browser::new(options)?;

            let api_url = web2md::registry_api_url(reg, &name);
            let body = browser.fetch_ignore_robots(&api_url)
                .await
                .with_context(|| format!("Failed to fetch {} API response for '{}'", reg.label(), name))?;
            let info = web2md::parse_registry_response(reg, &body)
                .with_context(|| format!("Failed to parse {} response for '{}'", reg.label(), name))?;

            if json {
                println!("{}", serde_json::to_string_pretty(&info)?);
            } else {
                println!("{}", web2md::package_info_to_markdown(&info));
            }
        }
        Some(Commands::Fetch(args)) => fetch::handle(args).await?,
        Some(Commands::Batch(args)) => batch::handle(args).await?,
    }

    Ok(())
}

pub(crate) async fn run_stdio_mcp(server: &McpServer) -> Result<()> {
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

pub(crate) fn domain_matches_any(url: &str, domains: &[String]) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    domains.iter().any(|d| {
        let d = d.trim_start_matches("www.");
        host == d || host.ends_with(&format!(".{d}"))
    })
}

/// Filter map links by a case-insensitive substring over URL or link text
/// (≈ Firecrawl /map?search=).
fn filter_map_links(links: &mut Vec<web2md::LinkEntry>, query: &str) {
    let q = query.to_lowercase();
    links.retain(|l| l.url.to_lowercase().contains(&q) || l.text.to_lowercase().contains(&q));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_matches_exact_and_subdomain() {
        assert!(domain_matches_any("https://github.com/repo", &["github.com".to_string()]));
        assert!(domain_matches_any("https://docs.github.com/page", &["github.com".to_string()]));
        assert!(!domain_matches_any("https://example.com", &["github.com".to_string()]));
    }

    #[test]
    fn domain_matches_strips_www_prefix() {
        assert!(domain_matches_any("https://www.rust-lang.org", &["rust-lang.org".to_string()]));
        assert!(domain_matches_any("https://rust-lang.org", &["www.rust-lang.org".to_string()]));
    }

    #[test]
    fn map_search_filters_url_or_text_case_insensitively() {
        use web2md::LinkEntry;
        let mut links = vec![
            LinkEntry {
                url: "https://example.com/docs/rust-guide".into(),
                text: "Guide".into(),
            },
            LinkEntry {
                url: "https://example.com/blog".into(),
                text: "Rust 101".into(),
            },
            LinkEntry {
                url: "https://example.com/about".into(),
                text: "About".into(),
            },
        ];
        filter_map_links(&mut links, "RUST");
        assert_eq!(links.len(), 2);
        filter_map_links(&mut links, "about");
        assert!(links.is_empty());
    }
}

