//! Interactive Lynx-like terminal browser: paged viewport, in-page search,
//! history, bookmarks, page export, and URL yank.

use anyhow::Result;
use std::io::{self, BufRead, Write};
use web2md::Browser;
use web2md::BrowserOptions;
use web2md::PageToMarkdown;

use crate::ansi::{render_markdown_ansi, strip_ansi};
use crate::cli::url_to_filename;

fn terminal_size() -> (usize, usize) {
    if let Ok(out) = std::process::Command::new("stty")
        .arg("size")
        .stdin(std::process::Stdio::inherit())
        .output()
        && out.status.success()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        let mut it = s.split_whitespace();
        if let (Some(h), Some(w)) = (it.next(), it.next())
            && let (Ok(h), Ok(w)) = (h.parse::<usize>(), w.parse::<usize>())
            && h >= 4 && w >= 20
        {
            return (h, w);
        }
    }
    (24, 80)
}

/// Clamp a viewport window over `total` lines starting at `top`.
/// Returns `(start, end)` with `end` exclusive.
pub(crate) fn viewport_window(total: usize, top: usize, height: usize) -> (usize, usize) {
    let height = height.max(1);
    let start = top.min(total.saturating_sub(1));
    let end = (start + height).min(total);
    (start, end)
}

/// Find the next line (forward, wrapping) whose plain text contains `pat` (case-insensitive).
/// `from` is the current viewport top; search starts at `from + 1`.
pub(crate) fn find_match_forward(lines: &[String], pat: &str, from: usize) -> Option<usize> {
    let pat = pat.to_lowercase();
    if pat.is_empty() || lines.is_empty() {
        return None;
    }
    let matches = |i: &usize| lines[*i].to_lowercase().contains(&pat);
    let from = from.min(lines.len() - 1);
    ((from + 1)..lines.len())
        .find(matches)
        .or_else(|| (0..=from).find(matches))
}

/// Find the previous line (backward, wrapping) whose plain text contains `pat`.
pub(crate) fn find_match_backward(lines: &[String], pat: &str, from: usize) -> Option<usize> {
    let pat = pat.to_lowercase();
    if pat.is_empty() || lines.is_empty() {
        return None;
    }
    let matches = |i: &usize| lines[*i].to_lowercase().contains(&pat);
    let from = from.min(lines.len() - 1);
    (0..from).rev().find(matches).or_else(|| ((from + 1)..lines.len()).rev().find(matches))
}

/// Minimal base64 encoder (standard alphabet, padded) for OSC 52 clipboard escapes.
pub(crate) fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Path of the bookmark file: `$HOME/.web2md/bookmarks.txt`.
fn bookmarks_path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".web2md").join("bookmarks.txt"))
}

/// Load bookmark URLs (one per line) from `path`.
pub(crate) fn load_bookmarks(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|s| s.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
        .unwrap_or_default()
}

/// Append a bookmark URL to `path` (creating parent dirs as needed).
pub(crate) fn add_bookmark(path: &std::path::Path, url: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{url}")
}

/// Interactive Lynx-like browser: paged viewport, in-page search, history,
/// bookmarks, page export, and URL yank.
pub(crate) async fn browse_loop(start_url: String, options: BrowserOptions, include_images: bool, keep_header: bool, main_content: bool) -> Result<()> {
    let mut history = vec![start_url];
    let mut current = 0usize;
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let browser = Browser::new(options)?;

    let mut raw_md = String::new();
    let mut rendered_lines: Vec<String> = Vec::new();
    let mut links: Vec<String> = Vec::new();
    let mut rendered = true;
    let mut top = 0usize;
    let mut need_fetch = true;
    let mut last_search: Option<String> = None;

    loop {
        let url = history[current].clone();

        if need_fetch {
            print!("\x1b[2J\x1b[H\x1b[7m WEB2MD \x1b[0m \x1b[90m{url}\x1b[0m\n\x1b[90mFetching...\x1b[0m");
            io::stdout().flush()?;

            match browser.fetch(&url).await {
                Ok(h) => {
                    print!("\r\x1b[2K\x1b[90mConverting...\x1b[0m");
                    io::stdout().flush()?;
                    let prepared = browser.prepare_html(&h, &url).await.unwrap_or(h);
                    let mut md = String::new();
                    PageToMarkdown::convert_progressive(
                        &prepared,
                        include_images,
                        keep_header,
                        main_content,
                        &[],
                        |block| md.push_str(&PageToMarkdown::absolutize_links(&block, &url)),
                    )?;
                    let (ansi, ls) = render_markdown_ansi(&md, true);
                    raw_md = md;
                    rendered_lines = ansi.lines().map(String::from).collect();
                    links = ls;
                    rendered = true;
                    top = 0;
                }
                Err(e) => {
                    println!("\r\x1b[2K\x1b[91mError: {e}\x1b[0m");
                    println!("\x1b[90mPress Enter to continue...\x1b[0m");
                    let mut buf = String::new();
                    let _ = stdin_lock.read_line(&mut buf);
                }
            }
            need_fetch = false;
        }

        let page: Vec<String> = if rendered {
            rendered_lines.clone()
        } else {
            raw_md.lines().map(String::from).collect()
        };

        let (rows, _) = terminal_size();
        let body_height = rows.saturating_sub(3).max(1);
        let (start, end) = viewport_window(page.len(), top, body_height);
        let view = if rendered { "ansi" } else { "raw" };

        print!("\x1b[2J\x1b[H");
        println!("\x1b[7m WEB2MD \x1b[0m \x1b[90m{url}\x1b[0m");
        for line in &page[start..end] {
            println!("{line}");
        }
        println!(
            "\x1b[7m [{}/{}] {} links | {} \x1b[0m",
            if page.is_empty() { 0 } else { start + 1 },
            end,
            links.len(),
            view
        );
        println!(
            "\x1b[90mSpace/+/- scroll  g/G top/end  /pat search  n/N next/prev  [1-{}] link  u url  b/f back/fwd  r reload  s save  a bookmark  v bookmarks  L links  y copy URL  m raw/ansi  ? help  q quit\x1b[0m",
            links.len()
        );
        print!("\x1b[1m> \x1b[0m");
        io::stdout().flush()?;

        let mut input = String::new();
        stdin_lock.read_line(&mut input)?;
        let input = input.trim();

        // Navigation that replaces the current page resets scroll + marks a refetch.
        let mut navigate: Option<String> = None;
        match input {
            "" | "+" | " " | "j" => top = (start + body_height).min(page.len().saturating_sub(1)),
            "-" | "k" => top = start.saturating_sub(body_height),
            "g" => top = 0,
            "G" => top = page.len().saturating_sub(1),
            "q" | "Q" => break,
            "b" | "B" => {
                if current > 0 {
                    current -= 1;
                    navigate = Some(history[current].clone());
                }
            }
            "f" | "F" => {
                if current + 1 < history.len() {
                    current += 1;
                    navigate = Some(history[current].clone());
                }
            }
            "r" | "R" => navigate = Some(url.clone()),
            "u" | "U" => {
                print!("URL: ");
                io::stdout().flush()?;
                let mut new_url = String::new();
                stdin_lock.read_line(&mut new_url)?;
                let new_url = new_url.trim().to_string();
                if !new_url.is_empty() {
                    navigate = Some(new_url);
                }
            }
            "m" | "M" => {
                rendered = !rendered;
                top = 0;
            }
            "s" | "S" => {
                let path = url_to_filename(&url);
                match std::fs::write(&path, &raw_md) {
                    Ok(_) => println!("\x1b[92mSaved {}\x1b[0m", path),
                    Err(e) => println!("\x1b[91mSave failed: {e}\x1b[0m"),
                }
                wait_for_enter(&mut stdin_lock);
            }
            "a" | "A" => {
                if let Some(path) = bookmarks_path() {
                    match add_bookmark(&path, &url) {
                        Ok(_) => println!("\x1b[92mBookmarked {url}\x1b[0m"),
                        Err(e) => println!("\x1b[91mBookmark failed: {e}\x1b[0m"),
                    }
                } else {
                    println!("\x1b[91m$HOME not set; cannot save bookmarks\x1b[0m");
                }
                wait_for_enter(&mut stdin_lock);
            }
            "v" | "V" => {
                let list = bookmarks_path().map(|p| load_bookmarks(&p)).unwrap_or_default();
                if list.is_empty() {
                    println!("\x1b[90mNo bookmarks\x1b[0m");
                } else {
                    println!("\x1b[1mBookmarks:\x1b[0m");
                    for (i, b) in list.iter().enumerate() {
                        println!("  \x1b[33m[{}]\x1b[0m {}", i + 1, b);
                    }
                    print!("\x1b[90mNumber to open, Enter to go back\x1b[0m \x1b[1m> \x1b[0m");
                    io::stdout().flush()?;
                    let mut pick = String::new();
                    stdin_lock.read_line(&mut pick)?;
                    if let Ok(n) = pick.trim().parse::<usize>()
                        && n > 0 && n <= list.len()
                    {
                        navigate = Some(list[n - 1].clone());
                    }
                }
            }
            "L" | "l" => {
                println!("\x1b[1mLinks on page:\x1b[0m");
                for (i, l) in links.iter().enumerate() {
                    println!("  \x1b[33m[{}]\x1b[0m {}", i + 1, resolve_url(&url, l));
                }
                wait_for_enter(&mut stdin_lock);
            }
            "y" | "Y" => {
                // OSC 52: copy to the terminal clipboard without any dependency.
                print!("\x1b]52;c;{}\x07", b64(url.as_bytes()));
                io::stdout().flush()?;
                println!("\x1b[92mCopied to clipboard (OSC 52): {url}\x1b[0m");
                wait_for_enter(&mut stdin_lock);
            }
            "?" => {
                println!("\x1b[1mKeys:\x1b[0m");
                println!("  Space/+-/j/k  scroll pages      g/G      top / end");
                println!("  /pat          search forward    n / N    next / previous match");
                println!("  1..N          follow link       u        open a URL");
                println!("  b / f         back / forward    r        reload");
                println!("  s             save page as .md  a        bookmark page");
                println!("  v             list bookmarks    L        list links");
                println!("  y             copy URL (OSC 52) m        toggle raw/ansi view");
                println!("  q             quit");
                wait_for_enter(&mut stdin_lock);
            }
            pat if pat.starts_with('/') => {
                let p = pat.trim_start_matches('/').trim().to_string();
                if !p.is_empty() {
                    last_search = Some(p.clone());
                }
                if let Some(p) = &last_search {
                    let plain: Vec<String> = page.iter().map(|l| strip_ansi(l)).collect();
                    match find_match_forward(&plain, p, top) {
                        Some(i) => top = i,
                        None => println!("\x1b[91mPattern not found: {p}\x1b[0m"),
                    }
                }
            }
            "n" => {
                if let Some(p) = &last_search {
                    let plain: Vec<String> = page.iter().map(|l| strip_ansi(l)).collect();
                    match find_match_forward(&plain, p, top) {
                        Some(i) => top = i,
                        None => println!("\x1b[91mNo more matches: {p}\x1b[0m"),
                    }
                }
            }
            "N" => {
                if let Some(p) = &last_search {
                    let plain: Vec<String> = page.iter().map(|l| strip_ansi(l)).collect();
                    match find_match_backward(&plain, p, top) {
                        Some(i) => top = i,
                        None => println!("\x1b[91mNo previous match: {p}\x1b[0m"),
                    }
                }
            }
            num => {
                if let Ok(n) = num.parse::<usize>()
                    && n > 0 && n <= links.len()
                {
                    navigate = Some(resolve_url(&url, &links[n - 1]));
                }
            }
        }

        if let Some(target) = navigate {
            history.truncate(current + 1);
            history.push(target);
            current += 1;
            top = 0;
            need_fetch = true;
        }
    }

    Ok(())
}

/// Print a "press Enter" prompt and consume one line of input.
fn wait_for_enter(stdin_lock: &mut io::StdinLock<'_>) {
    println!("\x1b[90mPress Enter to continue...\x1b[0m");
    let mut buf = String::new();
    let _ = stdin_lock.read_line(&mut buf);
}

/// Resolve a relative URL against a base URL.
pub(crate) fn resolve_url(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }
    if relative.starts_with("//") {
        if let Some(prefix) = base.split("://").next() {
            return format!("{}:{}", prefix, relative);
        }
        return relative.to_string();
    }
    if let Ok(base_url) = url::Url::parse(base)
        && let Ok(resolved) = base_url.join(relative) {
            return resolved.to_string();
        }
    relative.to_string()
}
