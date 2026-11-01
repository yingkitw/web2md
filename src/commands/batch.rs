//! `batch` subcommand: sequential URL-list conversion from a file, with
//! per-URL blacklist/robots checks and optional per-page file output.

use anyhow::{Context, Result};

use web2md::{extract_page_metadata, Browser, ConvertOptions, PageToMarkdown};

use crate::cli::{url_to_filename, BatchArgs};

pub(crate) async fn handle(args: BatchArgs) -> Result<()> {
let content = std::fs::read_to_string(&args.file)
    .context("Failed to read batch file")?;
let urls: Vec<String> = content
    .lines()
    .map(|l| l.trim())
    .filter(|l| !l.is_empty() && !l.starts_with('#'))
    .map(|l| l.to_string())
    .collect();

if urls.is_empty() {
    eprintln!("No URLs found in {}", args.file);
    return Ok(());
}

let options = args.common.browser_options()?;
let browser = Browser::new(options)?;

// Create output directory if specified
if let Some(ref dir) = args.output {
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
            let convert_opts = ConvertOptions {
                include_images: args.common.include_images,
                keep_header: args.common.keep_header,
                main_content: args.common.main_content,
                exclude_tags: args.exclude_tags.clone(),
                ..Default::default()
            };
            match PageToMarkdown::convert_with(&html, &convert_opts, &args.exclude_selector) {
                Ok(md) => {
                    let md = PageToMarkdown::absolutize_links(&md, url);
                    let md = if args.frontmatter {
                        let meta = extract_page_metadata(&html, &md);
                        if let Some(fm) = meta.to_frontmatter(Some(url)) {
                            format!("{}{}", fm, md)
                        } else {
                            md
                        }
                    } else {
                        md
                    };
                    if let Some(ref dir) = args.output {
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

    eprintln!("\nDone: {}/{} succeeded, {} failed, {} skipped", succeeded, total, failed, skipped);
}
    Ok(())
}
