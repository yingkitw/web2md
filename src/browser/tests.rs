use super::*;

#[tokio::test]
async fn browser_fetch_success() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/page")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>Hello</body></html>")
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let html = browser.fetch(&format!("{}/page", server.url())).await.unwrap();

    assert!(html.contains("Hello"));
    mock.assert_async().await;
}

#[tokio::test]
async fn browser_fetch_404() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/missing")
        .with_status(404)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let result = browser.fetch(&format!("{}/missing", server.url())).await;

    assert!(result.is_err());
    mock.assert_async().await;
}

#[tokio::test]
async fn browser_sends_cookies() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/private")
        .match_header("cookie", "session=abc123; auth=xyz")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>Secret</body></html>")
        .create_async()
        .await;

    let opts = BrowserOptions {
        cookies: vec!["session=abc123".to_string(), "auth=xyz".to_string()],
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();
    let html = browser
        .fetch(&format!("{}/private", server.url()))
        .await
        .unwrap();

    assert!(html.contains("Secret"));
    mock.assert_async().await;
}

#[tokio::test]
async fn browser_sends_custom_headers() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/api")
        .match_header("x-api-key", "secret123")
        .match_header("authorization", "Bearer token")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>API</body></html>")
        .create_async()
        .await;

    let opts = BrowserOptions {
        headers: vec![
            "X-API-Key: secret123".to_string(),
            "Authorization: Bearer token".to_string(),
        ],
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();
    let html = browser
        .fetch(&format!("{}/api", server.url()))
        .await
        .unwrap();

    assert!(html.contains("API"));
    mock.assert_async().await;
}

#[tokio::test]
async fn browser_sends_basic_auth() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/protected")
        .match_header("authorization", "Basic dXNlcjpwYXNz")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>Authorized</body></html>")
        .create_async()
        .await;

    let opts = BrowserOptions {
        basic_auth: Some("user:pass".to_string()),
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();
    let html = browser
        .fetch(&format!("{}/protected", server.url()))
        .await
        .unwrap();

    assert!(html.contains("Authorized"));
    mock.assert_async().await;
}

#[tokio::test]
async fn browser_basic_auth_trims_whitespace() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/protected")
        .match_header("authorization", "Basic dXNlcjpwYXNz")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>OK</body></html>")
        .create_async()
        .await;

    let opts = BrowserOptions {
        basic_auth: Some("  user :  pass  ".to_string()),
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();
    let html = browser
        .fetch(&format!("{}/protected", server.url()))
        .await
        .unwrap();

    assert!(html.contains("OK"));
    mock.assert_async().await;
}

#[tokio::test]
async fn browser_basic_auth_missing_colon_sends_no_auth() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/page")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>No auth</body></html>")
        .create_async()
        .await;

    let opts = BrowserOptions {
        basic_auth: Some("nopassword".to_string()),
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();
    let html = browser
        .fetch(&format!("{}/page", server.url()))
        .await
        .unwrap();

    assert!(html.contains("No auth"));
    mock.assert_async().await;
}

#[test]
fn browser_proxy_option_defaults_to_none() {
    let opts = BrowserOptions::default();
    assert!(opts.proxy.is_none());
    assert!(opts.basic_auth.is_none());
}

#[test]
fn browser_proxy_option_can_be_set() {
    let opts = BrowserOptions {
        proxy: Some("http://proxy:8080".to_string()),
        ..Default::default()
    };
    assert_eq!(opts.proxy.as_deref(), Some("http://proxy:8080"));
}

#[tokio::test]
async fn browser_invalid_proxy_url_returns_error() {
    let opts = BrowserOptions {
        proxy: Some("%%not-a-valid-url%%".to_string()),
        ..Default::default()
    };
    let result = Browser::new(opts);
    assert!(result.is_err());
}

#[tokio::test]
async fn browser_inlines_iframe_content() {
    let mut server = mockito::Server::new_async().await;
    let iframe_mock = server
        .mock("GET", "/widget")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<p>Widget Content</p>")
        .create_async()
        .await;

    let main_html = format!(
        r#"<html><body><h1>Main</h1><iframe src="{}/widget"></iframe><p>After</p></body></html>"#,
        server.url()
    );

    let main_mock = server
        .mock("GET", "/main")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(main_html)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let html = browser.fetch(&format!("{}/main", server.url())).await.unwrap();
    let inlined = browser
        .inline_iframes(&html, &format!("{}/main", server.url()))
        .await
        .unwrap();

    assert!(inlined.contains("Widget Content"));
    assert!(inlined.contains("Main"));
    assert!(inlined.contains("After"));
    assert!(!inlined.contains("<iframe"));

    iframe_mock.assert_async().await;
    main_mock.assert_async().await;
}

#[tokio::test]
async fn browser_inlines_iframe_resolves_relative_src() {
    let mut server = mockito::Server::new_async().await;
    let iframe_mock = server
        .mock("GET", "/nested/page")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<b>Nested</b>")
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let html = r#"<div><iframe src="nested/page"></iframe></div>"#;
    let inlined = browser
        .inline_iframes(html, &server.url())
        .await
        .unwrap();

    assert!(inlined.contains("Nested"));
    assert!(!inlined.contains("<iframe"));
    iframe_mock.assert_async().await;
}

#[tokio::test]
async fn browser_skips_blacklisted_iframe_src() {
    let mut server = mockito::Server::new_async().await;
    let iframe_mock = server
        .mock("GET", "/pixel.gif")
        .with_status(200)
        .with_header("content-type", "image/gif")
        .with_body("GIF89a")
        .expect(0)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let html = r#"<html><body><p>Main</p><iframe src="/pixel.gif"></iframe></body></html>"#;
    let inlined = browser
        .inline_iframes(html, &server.url())
        .await
        .unwrap();

    assert!(inlined.contains("Main"));
    assert!(!inlined.contains("GIF89a"));
    assert!(!inlined.contains("<iframe"));
    iframe_mock.assert_async().await;
}

#[tokio::test]
async fn browser_fetches_blacklisted_iframe_when_filter_disabled() {
    let mut server = mockito::Server::new_async().await;
    let iframe_mock = server
        .mock("GET", "/pixel.gif")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<p>Pixel content</p>")
        .create_async()
        .await;

    let opts = BrowserOptions {
        filter_blacklisted_urls: false,
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();
    let html = r#"<html><body><iframe src="/pixel.gif"></iframe></body></html>"#;
    let inlined = browser
        .inline_iframes(html, &server.url())
        .await
        .unwrap();

    assert!(inlined.contains("Pixel content"));
    iframe_mock.assert_async().await;
}

#[tokio::test]
async fn browser_respects_robots_disallow() {
    let mut server = mockito::Server::new_async().await;
    let robots = server
        .mock("GET", "/robots.txt")
        .with_status(200)
        .with_header("content-type", "text/plain")
        .with_body("User-agent: *\nDisallow: /private/\n")
        .expect(1)
        .create_async()
        .await;

    let blocked = server
        .mock("GET", "/private/secret")
        .with_status(200)
        .with_body("secret")
        .expect(0)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions {
        respect_robots_txt: true,
        ..Default::default()
    })
    .unwrap();
    let target = format!("{}/private/secret", server.url());
    assert!(
        !browser.robots_allows(&target).await.unwrap(),
        "expected robots.txt to disallow {target}"
    );
    let err = browser.fetch(&target).await.unwrap_err().to_string();
    assert!(err.contains("robots.txt"));

    robots.assert_async().await;
    blocked.assert_async().await;
}

#[tokio::test]
async fn browser_ignore_robots_fetches_disallowed_path() {
    let mut server = mockito::Server::new_async().await;
    let _robots = server
        .mock("GET", "/robots.txt")
        .with_status(200)
        .with_body("User-agent: *\nDisallow: /private/\n")
        .create_async()
        .await;

    let private = server
        .mock("GET", "/private/page")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>Private</body></html>")
        .create_async()
        .await;

    let opts = BrowserOptions {
        respect_robots_txt: false,
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();
    let html = browser
        .fetch(&format!("{}/private/page", server.url()))
        .await
        .unwrap();
    assert!(html.contains("Private"));
    private.assert_async().await;
}

#[test]
fn browser_loads_custom_blacklist_file() {
    let dir = std::env::temp_dir().join(format!("web2md-bl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("extra.txt");
    std::fs::write(&file, "evil-tracker.test\n/blocked-path/\n").unwrap();

    let opts = BrowserOptions {
        load_user_blacklist: false,
        extra_blacklist_files: vec![file.to_string_lossy().into_owned()],
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();

    assert!(browser.is_url_blocked("https://cdn.evil-tracker.test/pixel"));
    assert!(browser.is_url_blocked("https://example.com/blocked-path/page"));
    assert!(!browser.is_url_blocked("https://example.com/blog/post"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn browser_enforces_request_delay() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/page")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>Hello</body></html>")
        .expect(2)
        .create_async()
        .await;

    let opts = BrowserOptions {
        request_delay: Duration::from_millis(200),
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();

    let start = Instant::now();
    let _ = browser.fetch(&format!("{}/page", server.url())).await.unwrap();
    let _ = browser.fetch(&format!("{}/page", server.url())).await.unwrap();
    let elapsed = start.elapsed();

    assert!(
        elapsed >= Duration::from_millis(200),
        "expected delay between requests, got {:?}",
        elapsed
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn browser_cache_hit_avoids_second_request() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/cached")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>Cached content</body></html>")
        .expect(1)
        .create_async()
        .await;

    let opts = BrowserOptions {
        cache_ttl: Duration::from_secs(60),
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();

    let url = format!("{}/cached", server.url());
    let html1 = browser.fetch(&url).await.unwrap();
    let html2 = browser.fetch(&url).await.unwrap();

    assert_eq!(html1, html2);
    assert!(html1.contains("Cached content"));
    mock.assert_async().await;
}

#[tokio::test]
async fn browser_cache_disabled_by_default() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/nocache")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>Content</body></html>")
        .expect(2)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();

    let url = format!("{}/nocache", server.url());
    let _ = browser.fetch(&url).await.unwrap();
    let _ = browser.fetch(&url).await.unwrap();

    mock.assert_async().await;
}

#[tokio::test]
async fn browser_cache_expires_after_ttl() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/expiry")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>Content</body></html>")
        .expect(2)
        .create_async()
        .await;

    let opts = BrowserOptions {
        cache_ttl: Duration::from_millis(50),
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();

    let url = format!("{}/expiry", server.url());
    let _ = browser.fetch(&url).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let _ = browser.fetch(&url).await.unwrap();

    mock.assert_async().await;
}

#[tokio::test]
async fn browser_no_cache_bypasses_and_skips_store() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/bypass")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>Live</body></html>")
        .expect(2)
        .create_async()
        .await;

    let opts = BrowserOptions {
        cache_ttl: Duration::from_secs(60),
        no_cache: true,
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();
    let url = format!("{}/bypass", server.url());
    let _ = browser.fetch(&url).await.unwrap();
    let _ = browser.fetch(&url).await.unwrap();
    mock.assert_async().await;
}

#[tokio::test]
async fn browser_cache_max_age_rejects_stale_entries() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/maxage")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>Fresh</body></html>")
        .expect(2)
        .create_async()
        .await;

    let opts = BrowserOptions {
        cache_ttl: Duration::from_secs(60),
        cache_max_age: Some(Duration::from_millis(50)),
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();
    let url = format!("{}/maxage", server.url());
    let _ = browser.fetch(&url).await.unwrap();
    tokio::time::sleep(Duration::from_millis(80)).await;
    let _ = browser.fetch(&url).await.unwrap();
    mock.assert_async().await;
}

#[test]
fn parse_sitemap_urls_extracts_all_locs() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://example.com/</loc><lastmod>2025-01-01</lastmod></url>
  <url><loc>https://example.com/about</loc></url>
  <url><loc>https://example.com/contact</loc></url>
</urlset>"#;
    let urls = parse_sitemap_urls(xml);
    assert_eq!(urls.len(), 3);
    assert_eq!(urls[0], "https://example.com/");
    assert_eq!(urls[1], "https://example.com/about");
    assert_eq!(urls[2], "https://example.com/contact");
}

#[test]
fn parse_sitemap_urls_handles_empty() {
    let xml = "<?xml version=\"1.0\"?><urlset></urlset>";
    let urls = parse_sitemap_urls(xml);
    assert!(urls.is_empty());
}

#[test]
fn parse_sitemap_urls_handles_sitemap_index() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap><loc>https://example.com/sitemap1.xml</loc></sitemap>
  <sitemap><loc>https://example.com/sitemap2.xml</loc></sitemap>
</sitemapindex>"#;
    let urls = parse_sitemap_urls(xml);
    assert_eq!(urls.len(), 2);
    assert_eq!(urls[0], "https://example.com/sitemap1.xml");
    assert_eq!(urls[1], "https://example.com/sitemap2.xml");
}

#[test]
fn parse_sitemap_urls_skips_empty_locs() {
    let xml = r#"<urlset><url><loc></loc></url><url><loc>https://example.com/page</loc></url></urlset>"#;
    let urls = parse_sitemap_urls(xml);
    assert_eq!(urls.len(), 1);
    assert_eq!(urls[0], "https://example.com/page");
}

#[tokio::test]
async fn expand_sitemap_follows_index_and_urlset() {
    let mut server = mockito::Server::new_async().await;
    let index = r#"<?xml version="1.0" encoding="UTF-8"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap><loc>sitemap-pages.xml</loc></sitemap>
</sitemapindex>"#;
    let urlset = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://example.com/page1</loc></url>
  <url><loc>https://example.com/page2</loc></url>
</urlset>"#;

    server
        .mock("GET", "/sitemap.xml")
        .with_status(200)
        .with_header("content-type", "application/xml")
        .with_body(index)
        .create_async()
        .await;
    server
        .mock("GET", "/sitemap-pages.xml")
        .with_status(200)
        .with_header("content-type", "application/xml")
        .with_body(urlset)
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions {
        load_user_blacklist: false,
        ..Default::default()
    }).unwrap();
    let sitemap_url = format!("{}/sitemap.xml", server.url());
    let xml = browser.fetch(&sitemap_url).await.unwrap();
    let urls = browser.expand_sitemap(&sitemap_url, &xml).await;

    assert_eq!(urls.len(), 2);
    assert!(urls.contains(&"https://example.com/page1".to_string()));
    assert!(urls.contains(&"https://example.com/page2".to_string()));
}

#[tokio::test]
async fn enforce_delay_throttles_per_host() {
    let opts = BrowserOptions {
        host_rate_limit: Some(20.0), // 20 rps → 50ms floor per host
        ..Default::default()
    };
    let browser = Browser::new(opts).unwrap();
    let start = std::time::Instant::now();
    browser.enforce_delay(None, "a.example").await;
    browser.enforce_delay(None, "a.example").await;
    browser.enforce_delay(None, "b.example").await; // independent clock
    let elapsed = start.elapsed();
    assert!(
        elapsed >= std::time::Duration::from_millis(45),
        "expected at least ~50ms across two calls to the same host, got {:?}",
        elapsed
    );
    assert!(
        elapsed < std::time::Duration::from_millis(180),
        "expected much less than 200ms total, got {:?}",
        elapsed
    );
}

#[tokio::test]
async fn fetch_stream_returns_full_body_and_invokes_callback() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/stream")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body><p>Hello stream</p></body></html>")
        .create_async()
        .await;

    let browser = Browser::new(BrowserOptions::default()).unwrap();
    let mut chunk_count = 0usize;
    let body = browser
        .fetch_stream(&format!("{}/stream", server.url()), |_chunk| {
            chunk_count += 1;
        })
        .await
        .unwrap();

    assert!(body.contains("Hello stream"));
    assert!(chunk_count > 0);
    mock.assert_async().await;
}
