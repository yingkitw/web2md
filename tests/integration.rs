use browsedown::{Browser, BrowserOptions, McpRequest, McpServer, PageToMarkdown};
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
    let md = PageToMarkdown::convert(&html, false).unwrap();

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
    let md = PageToMarkdown::convert(&html, false).unwrap();

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
    let md = PageToMarkdown::convert(&html, false).unwrap();

    assert!(md.contains("Real content here"));
    assert!(!md.contains("Home"));
    assert!(!md.contains("Related links"));
    assert!(!md.contains("Enable JS"));
    assert!(!md.contains("Copyright"));
    assert!(!md.contains("tracking"));
    mock.assert_async().await;
}

#[test]
fn cli_format_html_emits_raw_html() {
    let output = std::process::Command::new("cargo")
        .args(["run", "--", "fetch", "--format", "html", "https://example.com"])
        .current_dir("/Users/yingkitw/Desktop/myproject/browsedown")
        .output()
        .expect("cargo run failed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("<html>") || stdout.contains("<body>") || stdout.contains("<h1>"),
        "expected raw HTML tags in output, got: {}", stdout);
}

#[test]
fn cli_render_adds_ansi_codes() {
    let output = std::process::Command::new("cargo")
        .args(["run", "--", "fetch", "--render", "https://example.com"])
        .current_dir("/Users/yingkitw/Desktop/myproject/browsedown")
        .output()
        .expect("cargo run failed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // ANSI escape codes should be present when --render is used
    assert!(stdout.contains('\x1b'), "expected ANSI escape codes in rendered output");
}
