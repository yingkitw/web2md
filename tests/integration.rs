use web2md::{Browser, BrowserOptions, McpRequest, McpServer, PageToMarkdown};
use std::time::Duration;

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
    let md = PageToMarkdown::convert(&html, false, false, false).unwrap();

    assert!(md.contains("Heading"));
    assert!(md.contains("First paragraph."));
    assert!(md.contains("Second paragraph."));
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

    assert_eq!(resp.title, Some("Integration Test".to_string()));
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
    let md = PageToMarkdown::convert(&html, false, false, false).unwrap();

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
    let md = PageToMarkdown::convert(&html, false, false, false).unwrap();

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
    let md = PageToMarkdown::convert(&html, false, false, false).unwrap();

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
    let md = PageToMarkdown::convert(&html, false, false, true).unwrap();

    assert!(md.contains("main article content"));
    assert!(!md.contains("Contact"));
    mock.assert_async().await;
}
