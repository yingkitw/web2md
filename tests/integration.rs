use std::time::Duration;
use url::Url;
use web2md::{
    extract_feed_links, extract_metadata, extract_page_metadata, feed_to_markdown,
    normalize_crawl_url, parse_feed, parse_sitemap_urls, same_origin_links, Browser,
    BrowserOptions, McpRequest, McpServer, PageToMarkdown,
};

#[tokio::test]
async fn fetch_and_convert_to_markdown() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/article")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><head><title>My Article</title></head>
             <body><h1>Heading</h1><p>First paragraph.</p><p>Second paragraph.</p></body>
             </html>",
        )
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let html = browser.fetch(&format!("{}/article", server.url())).await.unwrap();
    let md = PageToMarkdown::convert(&html, false, false, false, &[]).unwrap();

    assert!(md.contains("Heading"));
    assert!(md.contains("First paragraph."));
    assert!(md.contains("Second paragraph."));
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_and_convert_to_plain_text() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/plain")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><h1>Heading</h1><p>Text with <strong>bold</strong>.</p></body></html>",
        )
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let html = browser.fetch(&format!("{}/plain", server.url())).await.unwrap();
    let md = PageToMarkdown::convert(&html, false, false, false, &[]).unwrap();
    let text = PageToMarkdown::to_plain_text(&md);

    assert!(text.contains("Heading"));
    assert!(text.contains("bold"));
    assert!(!text.contains("**"));
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_404_propagates_error() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/missing")
        .with_status(404)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let result = browser.fetch(&format!("{}/missing", server.url())).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("404"));
    mock.assert_async().await;
}

#[tokio::test]
async fn mcp_server_end_to_end() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/doc")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><head><title>Integration Test</title></head>
             <body><h1>Title</h1><p>Body text.</p></body>
             </html>",
        )
        .create_async()
        .await;

    let mcp = McpServer::new().unwrap();
    let resp = mcp
        .handle(McpRequest {
            url: format!("{}/doc", server.url()),
            include_images: false,
            keep_header: false,
            main_content: false,
            max_length: None,
        })
        .await
        .unwrap();

    assert_eq!(resp.meta.title, Some("Integration Test".to_string()));
    assert!(resp.markdown.contains("Title"));
    assert!(resp.markdown.contains("Body text."));
    mock.assert_async().await;
}

#[tokio::test]
async fn mcp_server_max_length_truncation() {
    let mut server = mockito::Server::new_async().await;
    let body = "<html><body><p>".to_string()
        + &"a ".repeat(500)
        + "</p></body></html>";
    let mock = server
        .mock("GET", "/long")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(body)
        .create_async()
        .await;

    let mcp = McpServer::new().unwrap();
    let resp = mcp
        .handle(McpRequest {
            url: format!("{}/long", server.url()),
            include_images: false,
            keep_header: false,
            main_content: false,
            max_length: Some(100),
        })
        .await
        .unwrap();

    assert!(resp.markdown.contains("[truncated]"));
    assert!(resp.markdown.len() <= 120); // rough bound: 100 chars + "\n\n[truncated]"
    mock.assert_async().await;
}

#[tokio::test]
async fn custom_timeout_is_applied() {
    let mut opts = BrowserOptions::default();
    opts.timeout = Duration::from_secs(5);

    let browser = Browser::new(opts).unwrap();
    assert_eq!(browser.options().timeout, Duration::from_secs(5));
}

#[tokio::test]
async fn strips_scripts_and_styles_in_integration() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/styled")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><head><style>.red{color:red}</style></head>
             <body>
                <script>alert('xss')</script>
                <p>Visible content</p>
             </body></html>",
        )
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let html = browser.fetch(&format!("{}/styled", server.url())).await.unwrap();
    let md = PageToMarkdown::convert(&html, false, false, false, &[]).unwrap();

    assert!(!md.contains("alert"));
    assert!(!md.contains("color:red"));
    assert!(md.contains("Visible content"));
    mock.assert_async().await;
}

#[tokio::test]
async fn strips_noise_tags_in_integration() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/noisy")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            r#"<html>
             <head><!-- tracking comment --></head>
             <body>
                <nav><a href="/">Home</a></nav>
                <p>Real content here</p>
                <aside>Related links</aside>
                <noscript>Enable JS</noscript>
                <footer>Copyright 2025</footer>
             </body></html>"#,
        )
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let html = browser.fetch(&format!("{}/noisy", server.url())).await.unwrap();
    let md = PageToMarkdown::convert(&html, false, false, false, &[]).unwrap();

    assert!(md.contains("Real content here"));
    assert!(!md.contains("Home"));
    assert!(!md.contains("Related links"));
    assert!(!md.contains("Enable JS"));
    assert!(!md.contains("Copyright"));
    assert!(!md.contains("tracking"));
    mock.assert_async().await;
}

#[tokio::test]
async fn cli_format_html_emits_raw_html() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/raw")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body><h1>Title</h1><p>Content</p></body></html>")
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let html = browser.fetch(&format!("{}/raw", server.url())).await.unwrap();

    assert!(html.contains("<html>") || html.contains("<body>") || html.contains("<h1>"),
        "expected raw HTML tags in output, got: {}", html);
    mock.assert_async().await;
}

#[tokio::test]
async fn cli_render_adds_ansi_codes() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/render")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body><h1>Title</h1><p>Content with <a href=\"/link\">link</a>.</p></body></html>")
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let html = browser.fetch(&format!("{}/render", server.url())).await.unwrap();
    let md = PageToMarkdown::convert(&html, false, false, false, &[]).unwrap();

    // Simulate what --render does: the render_markdown_ansi function is in main.rs
    // and not exposed via the library, so we verify the markdown contains content
    // that would produce ANSI output. The actual ANSI rendering is tested in main.rs unit tests.
    assert!(md.contains("Title"), "expected title in markdown, got: {}", md);
    assert!(md.contains("Content"), "expected content in markdown, got: {}", md);
    mock.assert_async().await;
}

#[tokio::test]
async fn readability_main_content_extracts_from_div_layout() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/layout")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            r#"<html><body>
            <div><a href="/">Home</a><a href="/about">About</a><a href="/contact">Contact</a></div>
            <div><h2>Real Article</h2><p>This is the main article content with enough text to be extracted by the readability scoring algorithm. It contains substantial paragraphs that should score higher than the navigation div above.</p></div>
            </body></html>"#,
        )
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let html = browser.fetch(&format!("{}/layout", server.url())).await.unwrap();
    let md = PageToMarkdown::convert(&html, false, false, true, &[]).unwrap();

    assert!(md.contains("main article content"));
    assert!(!md.contains("Contact"));
    mock.assert_async().await;
}

#[tokio::test]
async fn json_output_format_emits_structured_json() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/json")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(r#"<html lang="en"><head>
            <title>JSON Test Page</title>
            <meta name="description" content="A test page for JSON output">
            <meta name="author" content="Test Author">
            <meta property="article:published_time" content="2025-07-04T12:00:00Z">
            <meta property="og:url" content="https://example.com/json-canonical">
            <meta property="article:section" content="Testing">
            <link rel="canonical" href="https://example.com/json">
        </head><body><h1>Heading</h1><p>Body content for JSON with enough words to populate the excerpt metadata field.</p></body></html>"#)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let html = browser.fetch(&format!("{}/json", server.url())).await.unwrap();
    let md = PageToMarkdown::convert(&html, false, false, false, &[]).unwrap();
    let meta = extract_metadata(&html);

    let json = serde_json::json!({
        "markdown": md,
        "title": meta.title,
        "description": meta.description,
        "author": meta.author,
        "published_date": meta.published_date,
        "categories": meta.categories,
        "excerpt": meta.excerpt,
        "canonical_url": meta.canonical_url,
        "language": meta.language,
    });
    let json_str = serde_json::to_string(&json).unwrap();

    assert!(json_str.contains("JSON Test Page"));
    assert!(json_str.contains("A test page for JSON output"));
    assert!(json_str.contains("Test Author"));
    assert!(json_str.contains("2025-07-04T12:00:00Z"));
    assert!(json_str.contains("json-canonical"));
    assert!(json_str.contains("\"language\":\"en\""));
    assert!(json_str.contains("Testing"));
    assert!(json_str.contains("Body content for JSON"));
    mock.assert_async().await;
}

#[tokio::test]
async fn csv_output_format_emits_header_and_row() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/csv")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            r#"<html><head><title>CSV Page</title><meta name="author" content="CSV Author">
            </head><body><article><h1>CSV Page</h1>
            <p>This English paragraph is long enough for language detection and CSV export of the extracted plain text content for corpus pipelines.</p>
            </article></body></html>"#,
        )
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let url = format!("{}/csv", server.url());
    let html = browser.fetch(&url).await.unwrap();
    let md = PageToMarkdown::convert(&html, false, false, true, &[]).unwrap();
    let text = PageToMarkdown::to_plain_text(&md);
    let meta = extract_page_metadata(&html, &md);
    let csv = meta.to_csv(&url, &text);

    assert!(csv.starts_with(
        "url,title,author,published_date,language,page_type,extraction_quality,text\n"
    ));
    assert!(csv.contains("CSV Page"));
    assert!(csv.contains("CSV Author"));
    assert!(csv.contains("article"));
    assert!(csv.contains("corpus pipelines"));
    assert_eq!(meta.language.as_deref(), Some("eng"));
    mock.assert_async().await;
}

#[tokio::test]
async fn tei_output_format_emits_tei_document() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/tei")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            r#"<html lang="en"><head><title>TEI Page</title><meta name="author" content="TEI Author">
            <meta property="og:site_name" content="TEI Site">
            </head><body><article><h1>TEI Page</h1>
            <p>This English paragraph is long enough for TEI export of the extracted plain text content for corpus pipelines.</p>
            </article></body></html>"#,
        )
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let url = format!("{}/tei", server.url());
    let html = browser.fetch(&url).await.unwrap();
    let md = PageToMarkdown::convert(&html, false, false, true, &[]).unwrap();
    let text = PageToMarkdown::to_plain_text(&md);
    let meta = extract_page_metadata(&html, &md);
    let tei = meta.to_tei(&url, &text);

    assert!(tei.contains("<TEI xmlns=\"http://www.tei-c.org/ns/1.0\">"));
    assert!(tei.contains("<title>TEI Page</title>"));
    assert!(tei.contains("<author>TEI Author</author>"));
    assert!(tei.contains("<publisher>TEI Site</publisher>"));
    assert!(tei.contains("<language ident=\"en\"/>"));
    assert!(tei.contains("<div type=\"entry\">"));
    assert!(tei.contains("corpus pipelines"));
    mock.assert_async().await;
}

#[tokio::test]
async fn sitemap_discovery_fetches_and_parses() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/sitemap.xml")
        .with_status(200)
        .with_header("content-type", "application/xml")
        .with_body(r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://example.com/page1</loc></url>
  <url><loc>https://example.com/page2</loc></url>
</urlset>"#)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let sitemap_url = format!("{}/sitemap.xml", server.url());
    let xml = browser.fetch(&sitemap_url).await.unwrap();
    let urls = parse_sitemap_urls(&xml);

    assert_eq!(urls.len(), 2);
    assert!(urls.contains(&"https://example.com/page1".to_string()));
    assert!(urls.contains(&"https://example.com/page2".to_string()));
    mock.assert_async().await;
}

#[tokio::test]
async fn feed_discovery_from_html_page() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/blog")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(r#"<html><head>
            <link rel="alternate" type="application/rss+xml" href="/blog/rss.xml">
            <link rel="alternate" type="application/atom+xml" href="/blog/atom.xml">
            <link rel="alternate" type="application/feed+json" href="/blog/feed.json">
        </head><body><h1>Blog</h1></body></html>"#)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let html = browser.fetch(&format!("{}/blog", server.url())).await.unwrap();
    let feeds = extract_feed_links(&html);

    assert_eq!(feeds.len(), 3);
    assert!(feeds.contains(&"/blog/rss.xml".to_string()));
    assert!(feeds.contains(&"/blog/atom.xml".to_string()));
    assert!(feeds.contains(&"/blog/feed.json".to_string()));
    mock.assert_async().await;
}

#[tokio::test]
async fn feed_command_parses_rss_to_markdown() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/rss.xml")
        .with_status(200)
        .with_header("content-type", "application/rss+xml")
        .with_body(r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <title>Test Feed</title>
  <link>https://example.com</link>
  <item>
    <title>Hello RSS</title>
    <link>https://example.com/hello</link>
    <pubDate>Sat, 11 Jul 2026 12:00:00 GMT</pubDate>
    <description>A short summary</description>
  </item>
</channel></rss>"#)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let xml = browser.fetch(&format!("{}/rss.xml", server.url())).await.unwrap();
    let feed = parse_feed(&xml).expect("should parse RSS");
    let md = feed_to_markdown(&feed);

    assert_eq!(feed.title.as_deref(), Some("Test Feed"));
    assert_eq!(feed.entries.len(), 1);
    assert!(md.contains("# Test Feed"));
    assert!(md.contains("## [Hello RSS](https://example.com/hello)"));
    assert!(md.contains("A short summary"));
    mock.assert_async().await;
}

#[tokio::test]
async fn feed_command_parses_atom() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/atom.xml")
        .with_status(200)
        .with_header("content-type", "application/atom+xml")
        .with_body(r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Test</title>
  <link href="https://example.com/" rel="alternate"/>
  <entry>
    <title>Atom Entry</title>
    <link href="https://example.com/atom-entry"/>
    <updated>2026-07-11T10:00:00Z</updated>
    <summary>Atom summary text</summary>
  </entry>
</feed>"#)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let xml = browser.fetch(&format!("{}/atom.xml", server.url())).await.unwrap();
    let feed = parse_feed(&xml).expect("should parse Atom");

    assert_eq!(feed.title.as_deref(), Some("Atom Test"));
    assert_eq!(feed.link.as_deref(), Some("https://example.com/"));
    assert_eq!(feed.entries[0].title.as_deref(), Some("Atom Entry"));
    assert_eq!(
        feed.entries[0].link.as_deref(),
        Some("https://example.com/atom-entry")
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn feed_command_parses_json_feed() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/feed.json")
        .with_status(200)
        .with_header("content-type", "application/feed+json")
        .with_body(r#"{
            "version": "https://jsonfeed.org/version/1.1",
            "title": "JSON Feed Test",
            "home_page_url": "https://example.com/",
            "items": [
                {
                    "id": "1",
                    "url": "https://example.com/post",
                    "title": "Hello JSON Feed",
                    "content_text": "Body from JSON Feed",
                    "date_published": "2026-07-11T12:00:00Z"
                }
            ]
        }"#)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let body = browser.fetch(&format!("{}/feed.json", server.url())).await.unwrap();
    let feed = parse_feed(&body).expect("should parse JSON Feed");
    let md = feed_to_markdown(&feed);

    assert_eq!(feed.title.as_deref(), Some("JSON Feed Test"));
    assert_eq!(feed.entries.len(), 1);
    assert!(md.contains("# JSON Feed Test"));
    assert!(md.contains("## [Hello JSON Feed](https://example.com/post)"));
    assert!(md.contains("Body from JSON Feed"));
    mock.assert_async().await;
}

#[tokio::test]
async fn js_disabled_ignores_document_write() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/page")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><p>Static</p>\
             <script>document.write(\"<p>Dynamic</p>\");</script>\
             </body></html>",
        )
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let url = format!("{}/page", server.url());
    let html = browser.fetch(&url).await.unwrap();
    let html = browser.run_inline_scripts(&html);
    let md = PageToMarkdown::convert(&html, false, false, false, &[]).unwrap();

    assert!(md.contains("Static"));
    assert!(!md.contains("Dynamic"));
    mock.assert_async().await;
}

#[tokio::test]
async fn js_enabled_captures_document_write() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/page")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><p>Static</p>\
             <script>var items=[\"a\",\"b\"]; for (var i of items){document.write(\"<p>\"+i+\"</p>\");}</script>\
             <script type=\"application/ld+json\">{\"x\":1}</script>\
             <script src=\"external.js\"></script>\
             </body></html>",
        )
        .create_async()
        .await;

    let mut opts = BrowserOptions::default();
    opts.enable_javascript = true;
    let browser = Browser::new(opts).unwrap();
    let url = format!("{}/page", server.url());
    let html = browser.fetch(&url).await.unwrap();
    let html = browser.run_inline_scripts(&html);

    // Captured HTML is injected before </body>.
    assert!(html.contains("<p>a</p>"));
    assert!(html.contains("<p>b</p>"));
    assert!(html.contains("Static"));

    let md = PageToMarkdown::convert(&html, false, false, false, &[]).unwrap();
    assert!(md.contains("Static"));
    assert!(md.contains("a"));
    assert!(md.contains("b"));
    mock.assert_async().await;
}

#[tokio::test]
async fn settimeout_captures_delayed_content_with_wait() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/page")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><p>Static</p>\
             <script>setTimeout(function(){document.write(\"<p>Delayed</p>\");}, 50);</script>\
             </body></html>",
        )
        .create_async()
        .await;

    let mut opts = BrowserOptions::default();
    opts.enable_javascript = true;
    opts.post_load_wait = Duration::from_millis(100);
    let browser = Browser::new(opts).unwrap();
    let url = format!("{}/page", server.url());
    let html = browser.fetch(&url).await.unwrap();
    let html = browser.prepare_html(&html, &url).await.unwrap();

    assert!(html.contains("Delayed"));
    let md = PageToMarkdown::convert(&html, false, false, false, &[]).unwrap();
    assert!(md.contains("Delayed"));
    mock.assert_async().await;
}

#[tokio::test]
async fn setinterval_captures_repeated_content_with_wait() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/page")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body>\
             <script>setInterval(function(){document.write(\"x\");}, 40);</script>\
             </body></html>",
        )
        .create_async()
        .await;

    let mut opts = BrowserOptions::default();
    opts.enable_javascript = true;
    opts.post_load_wait = Duration::from_millis(120);
    let browser = Browser::new(opts).unwrap();
    let url = format!("{}/page", server.url());
    let html = browser.fetch(&url).await.unwrap();
    let html = browser.prepare_html(&html, &url).await.unwrap();

    assert!(html.contains("xxx"));
    mock.assert_async().await;
}

#[tokio::test]
async fn blacklisted_iframe_not_inlined_in_pipeline() {
    let mut server = mockito::Server::new_async().await;
    let iframe_mock = server
        .mock("GET", "/beacon")
        .with_status(200)
        .with_body("TRACKED")
        .expect(0)
        .create_async()
        .await;

    let main_mock = server
        .mock("GET", "/page")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(r#"<html><body><h1>Article</h1><iframe src="/beacon"></iframe></body></html>"#)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let url = format!("{}/page", server.url());
    let html = browser.fetch(&url).await.unwrap();
    let html = browser.inline_iframes(&html, &url).await.unwrap();
    let md = PageToMarkdown::convert(&html, false, false, false, &[]).unwrap();

    assert!(md.contains("Article"));
    assert!(!md.contains("TRACKED"));
    iframe_mock.assert_async().await;
    main_mock.assert_async().await;
}

#[tokio::test]
async fn recursive_crawl_depth_one_discovers_same_origin_links() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let root = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            r#"<html><body>
                <p>Root page</p>
                <a href="{}/a">A</a>
                <a href="{}/b">B</a>
                <a href="https://other.example.com/x">External</a>
            </body></html>"#,
            base, base
        ))
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let root_url = format!("{}/", base);
    let origin = Url::parse(&root_url).unwrap();

    let html = browser.fetch(&root_url).await.unwrap();
    let links = same_origin_links(&html, &root_url, &origin);
    assert_eq!(links.len(), 2);
    assert!(links.iter().any(|u| u.ends_with("/a")));
    assert!(links.iter().any(|u| u.ends_with("/b")));

    root.assert_async().await;
}

#[tokio::test]
async fn recursive_crawl_depth_two_reaches_nested_page() {
    use std::collections::{HashSet, VecDeque};

    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let page_c = server
        .mock("GET", "/c")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body><p>Page C nested</p></body></html>")
        .create_async()
        .await;

    let page_a = server
        .mock("GET", "/a")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            r#"<html><body><p>Page A</p><a href="{}/c">C</a></body></html>"#,
            base
        ))
        .create_async()
        .await;

    let root = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            r#"<html><body><p>Root</p><a href="{}/a">A</a></body></html>"#,
            base
        ))
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let root_url = format!("{}/", base);
    let origin = Url::parse(&root_url).unwrap();

    let mut visited = HashSet::new();
    let mut queue = VecDeque::from([(root_url.clone(), 0u32)]);
    let depth = 2u32;
    let mut fetched = Vec::new();

    while let Some((url, level)) = queue.pop_front() {
        let key = normalize_crawl_url(&url, &url).unwrap_or(url.clone());
        if !visited.insert(key) {
            continue;
        }
        let html = browser.fetch(&url).await.unwrap();
        fetched.push(url.clone());
        if level < depth {
            for link in same_origin_links(&html, &url, &origin) {
                let link_key = normalize_crawl_url(&link, &link).unwrap_or(link.clone());
                if !visited.contains(&link_key) {
                    queue.push_back((link, level + 1));
                }
            }
        }
    }

    assert_eq!(fetched.len(), 3);
    assert!(fetched.iter().any(|u| u.ends_with("/c")));

    root.assert_async().await;
    page_a.assert_async().await;
    page_c.assert_async().await;
}

#[tokio::test]
async fn robots_txt_blocks_disallowed_paths() {
    let mut server = mockito::Server::new_async().await;
    let _robots = server
        .mock("GET", "/robots.txt")
        .with_status(200)
        .with_header("content-type", "text/plain")
        .with_body("User-agent: *\nDisallow: /hidden/\n")
        .create_async()
        .await;

    let allowed = server
        .mock("GET", "/visible")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body><p>Visible</p></body></html>")
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let visible = browser
        .fetch(&format!("{}/visible", server.url()))
        .await
        .unwrap();
    let md = PageToMarkdown::convert(&visible, false, false, false, &[]).unwrap();
    assert!(md.contains("Visible"));

    assert!(!browser
        .robots_allows(&format!("{}/hidden/page", server.url()))
        .await
        .unwrap());

    allowed.assert_async().await;
}
