use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::markdown::find_ci;
use crate::{Browser, BrowserOptions, PageToMarkdown};

/// MCP tool request schema
#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub url: String,
    #[serde(default)]
    pub include_images: bool,
    #[serde(default)]
    pub keep_header: bool,
    #[serde(default)]
    pub main_content: bool,
    #[serde(default)]
    pub max_length: Option<usize>,
}

/// MCP tool response schema
#[derive(Debug, Serialize)]
pub struct McpResponse {
    pub url: String,
    pub markdown: String,
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_date: Option<String>,
}

/// MCP server wrapping the Browser
pub struct McpServer {
    browser: Browser,
}

impl McpServer {
    /// Create a new MCP server instance
    pub fn new() -> Result<Self> {
        let browser = Browser::new(BrowserOptions::default())?;
        Ok(Self { browser })
    }

    /// Handle a single MCP request: fetch URL and return Markdown
    pub async fn handle(&self, req: McpRequest) -> Result<McpResponse> {
        let html = self.browser.fetch(&req.url).await?;
        let html = self.browser.inline_iframes(&html, &req.url).await?;
        let mut markdown = PageToMarkdown::convert(&html, req.include_images, req.keep_header, req.main_content)?;

        if let Some(max) = req.max_length {
            if markdown.len() > max {
                markdown = format!("{}\n\n[truncated]", &markdown[..max]);
            }
        }

        let title = extract_title(&html);
        let description = extract_meta_content(&html, "name", "description")
            .or_else(|| extract_meta_content(&html, "property", "og:description"));
        let author = extract_meta_content(&html, "name", "author");
        let published_date = extract_published_date(&html);

        Ok(McpResponse {
            url: req.url,
            markdown,
            title,
            description,
            author,
            published_date,
        })
    }
}

/// Extract publication date from HTML.
/// Checks in order: `<meta property="article:published_time">`, `<meta name="article:published_time">`,
/// `<time datetime="...">`, and JSON-LD `"datePublished":"..."`.
fn extract_published_date(html: &str) -> Option<String> {
    extract_meta_content(html, "property", "article:published_time")
        .or_else(|| extract_meta_content(html, "name", "article:published_time"))
        .or_else(|| extract_time_datetime(html))
        .or_else(|| extract_json_ld_date(html))
}

/// Extract `datetime` attribute from the first `<time>` tag.
fn extract_time_datetime(html: &str) -> Option<String> {
    let pos = find_ci(html, "<time")?;
    let tag_end = html[pos..].find('>').map(|e| pos + e)?;
    let tag = &html[pos..=tag_end];
    extract_attr(tag, "datetime")
}

/// Extract `datePublished` from JSON-LD `<script type="application/ld+json">` blocks.
fn extract_json_ld_date(html: &str) -> Option<String> {
    let mut i = 0;
    while i < html.len() {
        let rest = &html[i..];
        let pos = find_ci(rest, "application/ld+json")?;
        let abs_pos = i + pos;
        // Find the closing </script>
        let script_close = find_ci(&html[abs_pos..], "</script>")?;
        let block = &html[abs_pos..abs_pos + script_close];
        // Find the > that opens the script tag content
        let gt = block.find('>')?;
        let json_content = &block[gt + 1..];
        // Parse as JSON and extract datePublished
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_content) {
            if let Some(date) = json.get("datePublished").and_then(|v| v.as_str()) {
                return Some(date.to_string());
            }
        }
        i = abs_pos + script_close + 9;
    }
    None
}

/// Extract `<title>` text from HTML.
fn extract_title(html: &str) -> Option<String> {
    find_ci(html, "<title>").and_then(|start| {
        let rest = &html[start + 7..];
        find_ci(rest, "</title>").map(|end| rest[..end].trim().to_string())
    })
}

/// Extract `content` attribute from a `<meta>` tag matching the given attribute key/value pair.
/// e.g. `extract_meta_content(html, "name", "description")` finds `<meta name="description" content="...">`.
fn extract_meta_content(html: &str, attr_key: &str, attr_val: &str) -> Option<String> {
    let mut i = 0;
    while i < html.len() {
        if let Some(pos) = find_ci(&html[i..], "<meta") {
            let pos = i + pos;
            let tag_end = html[pos..].find('>').map(|e| pos + e)?;
            let tag = &html[pos..=tag_end];
            if find_ci(tag, &format!("{}=\"{}\"", attr_key, attr_val)).is_some()
                || find_ci(tag, &format!("{}='{}'", attr_key, attr_val)).is_some()
            {
                return extract_attr(tag, "content");
            }
            i = tag_end + 1;
        } else {
            break;
        }
    }
    None
}

/// Extract the value of an attribute from an HTML tag string.
fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let needle = format!("{}=", attr);
    let pos = find_ci(tag, &needle)?;
    let after = &tag[pos + needle.len()..];
    let mut i = 0;
    while i < after.len() && after.as_bytes()[i].is_ascii_whitespace() {
        i += 1;
    }
    let quote = *after.as_bytes().get(i)? as char;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let val_start = i + 1;
    let val_end = after[val_start..].find(quote)? + val_start;
    Some(after[val_start..val_end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mcp_server_fetch_and_convert() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/doc")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><head><title>Test Page</title></head><body><h1>Hello</h1><p>World</p></body></html>")
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

        assert_eq!(resp.title, Some("Test Page".to_string()));
        assert!(resp.markdown.contains("Hello"));
        assert!(resp.markdown.contains("World"));
        assert_eq!(resp.published_date, None);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mcp_server_extracts_metadata() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/meta")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<html><head>
                <title>Article Title</title>
                <meta name="description" content="A test article about Rust">
                <meta name="author" content="Jane Doe">
                <meta property="og:description" content="OG description override">
                <meta property="article:published_time" content="2025-01-15T08:30:00Z">
            </head><body><p>Content</p></body></html>"#)
            .create_async()
            .await;

        let mcp = McpServer::new().unwrap();
        let resp = mcp
            .handle(McpRequest {
                url: format!("{}/meta", server.url()),
                include_images: false,
                keep_header: false,
                main_content: false,
                max_length: None,
            })
            .await
            .unwrap();

        assert_eq!(resp.title, Some("Article Title".to_string()));
        assert_eq!(resp.description, Some("A test article about Rust".to_string()));
        assert_eq!(resp.author, Some("Jane Doe".to_string()));
        assert_eq!(resp.published_date, Some("2025-01-15T08:30:00Z".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mcp_server_og_description_fallback() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/og")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<html><head>
                <title>OG Page</title>
                <meta property="og:description" content="OG only description">
            </head><body><p>Body</p></body></html>"#)
            .create_async()
            .await;

        let mcp = McpServer::new().unwrap();
        let resp = mcp
            .handle(McpRequest {
                url: format!("{}/og", server.url()),
                include_images: false,
                keep_header: false,
                main_content: false,
                max_length: None,
            })
            .await
            .unwrap();

        assert_eq!(resp.description, Some("OG only description".to_string()));
        assert_eq!(resp.author, None);
        assert_eq!(resp.published_date, None);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mcp_server_no_metadata_returns_none() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/bare")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><body><p>No metadata here</p></body></html>")
            .create_async()
            .await;

        let mcp = McpServer::new().unwrap();
        let resp = mcp
            .handle(McpRequest {
                url: format!("{}/bare", server.url()),
                include_images: false,
                keep_header: false,
                main_content: false,
                max_length: None,
            })
            .await
            .unwrap();

        assert_eq!(resp.title, None);
        assert_eq!(resp.description, None);
        assert_eq!(resp.author, None);
        assert_eq!(resp.published_date, None);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mcp_server_extracts_date_from_time_tag() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/time")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<html><head><title>Time Article</title></head><body><article><time datetime="2024-06-01">June 1, 2024</time><p>Article body.</p></article></body></html>"#)
            .create_async()
            .await;

        let mcp = McpServer::new().unwrap();
        let resp = mcp
            .handle(McpRequest {
                url: format!("{}/time", server.url()),
                include_images: false,
                keep_header: false,
                main_content: false,
                max_length: None,
            })
            .await
            .unwrap();

        assert_eq!(resp.published_date, Some("2024-06-01".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mcp_server_extracts_date_from_json_ld() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/jsonld")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<html><head><title>JSON-LD Article</title><script type="application/ld+json">{"@type":"NewsArticle","headline":"Test","datePublished":"2023-12-25T10:00:00+00:00","author":{"@type":"Person","name":"John"}}</script></head><body><p>Content.</p></body></html>"#)
            .create_async()
            .await;

        let mcp = McpServer::new().unwrap();
        let resp = mcp
            .handle(McpRequest {
                url: format!("{}/jsonld", server.url()),
                include_images: false,
                keep_header: false,
                main_content: false,
                max_length: None,
            })
            .await
            .unwrap();

        assert_eq!(resp.published_date, Some("2023-12-25T10:00:00+00:00".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mcp_server_meta_date_takes_priority() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/priority")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<html><head><meta property="article:published_time" content="2025-03-20T12:00:00Z"></head><body><time datetime="2024-01-01">Old date</time><p>Content.</p></body></html>"#)
            .create_async()
            .await;

        let mcp = McpServer::new().unwrap();
        let resp = mcp
            .handle(McpRequest {
                url: format!("{}/priority", server.url()),
                include_images: false,
                keep_header: false,
                main_content: false,
                max_length: None,
            })
            .await
            .unwrap();

        assert_eq!(resp.published_date, Some("2025-03-20T12:00:00Z".to_string()));
        mock.assert_async().await;
    }
}
