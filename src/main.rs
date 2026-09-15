//! `web2md` binary entry point: parse CLI input and dispatch to subcommand handlers.

mod ansi;
mod cli;
mod commands;
mod options;
mod tui;
mod watch;

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    commands::run(cli).await
}

#[cfg(test)]
mod tests {
    use crate::ansi::{fix_raw_links, render_markdown_ansi};
    use crate::cli::url_to_filename;
    use crate::commands::domain_matches_any;
    use crate::commands::fetch::chunk_markdown_by_headings;
    use crate::tui::*;

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
    fn viewport_window_basic() {
        assert_eq!(viewport_window(100, 0, 20), (0, 20));
        assert_eq!(viewport_window(100, 40, 20), (40, 60));
        assert_eq!(viewport_window(100, 90, 20), (90, 100));
    }

    #[test]
    fn viewport_window_edge_cases() {
        assert_eq!(viewport_window(0, 0, 20), (0, 0));
        assert_eq!(viewport_window(5, 0, 20), (0, 5));
        assert_eq!(viewport_window(100, 999, 20), (99, 100));
        assert_eq!(viewport_window(10, 0, 0), (0, 1));
    }

    fn search_lines() -> Vec<String> {
        vec![
            "alpha introduction".into(),
            "beta section".into(),
            "gamma alpha again".into(),
        ]
    }

    #[test]
    fn find_match_forward_from_top() {
        let lines = search_lines();
        assert_eq!(find_match_forward(&lines, "beta", 0), Some(1));
        assert_eq!(find_match_forward(&lines, "alpha", 0), Some(2));
    }

    #[test]
    fn find_match_forward_wraps_around() {
        let lines = search_lines();
        assert_eq!(find_match_forward(&lines, "beta", 1), Some(1));
        assert_eq!(find_match_forward(&lines, "intro", 2), Some(0));
    }

    #[test]
    fn find_match_backward_search() {
        let lines = search_lines();
        assert_eq!(find_match_backward(&lines, "alpha", 2), Some(0));
        assert_eq!(find_match_backward(&lines, "gamma", 0), Some(2));
    }

    #[test]
    fn find_match_case_insensitive_and_empty() {
        let lines = vec!["Hello World".to_string()];
        assert_eq!(find_match_forward(&lines, "WORLD", 0), Some(0));
        assert_eq!(find_match_forward(&lines, "", 0), None);
        assert_eq!(find_match_backward(&lines, "nope", 0), None);
    }

    #[test]
    fn b64_known_vectors() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(b"foob"), "Zm9vYg==");
        assert_eq!(b64(b"https://example.com"), "aHR0cHM6Ly9leGFtcGxlLmNvbQ==");
    }

    #[test]
    fn bookmark_roundtrip() {
        let dir = std::env::temp_dir().join(format!("web2md_bm_test_{}", std::process::id()));
        let path = dir.join("bookmarks.txt");
        let _ = std::fs::remove_file(&path);
        assert!(load_bookmarks(&path).is_empty());
        add_bookmark(&path, "https://a.com").unwrap();
        add_bookmark(&path, "https://b.com").unwrap();
        assert_eq!(load_bookmarks(&path), vec!["https://a.com", "https://b.com"]);
        let _ = std::fs::remove_dir_all(&dir);
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
    fn chunk_markdown_splits_by_headings() {
        let md = "# Title\n\nIntro text\n\n## Section A\n\nContent A\n\n## Section B\n\nContent B";
        let chunked = chunk_markdown_by_headings(md);
        assert!(chunked.contains("---"));
        assert!(chunked.contains("# Title"));
        assert!(chunked.contains("## Section A"));
        assert!(chunked.contains("## Section B"));
    }

    #[test]
    fn chunk_markdown_single_heading_no_separator() {
        let md = "# Only Heading\n\nContent here";
        let chunked = chunk_markdown_by_headings(md);
        assert!(!chunked.contains("---"));
    }

    #[test]
    fn sup_sub_converted_to_markdown() {
        let html = "<p>H<sub>2</sub>O and E=mc<sup>2</sup></p>";
        let md = web2md::PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
        assert!(md.contains("~(2)"), "sub should be ~(2), got: {md}");
        assert!(md.contains("^(2)"), "sup should be ^(2), got: {md}");
    }
}
