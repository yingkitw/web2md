use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{Browser, BrowserOptions, PageToMarkdown};

/// MCP tool request schema
#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub url: String,
    #[serde(default)]
    pub include_images: bool,
    #[serde(default)]
    pub max_length: Option<usize>,
}

/// MCP tool response schema
#[derive(Debug, Serialize)]
pub struct McpResponse {
    pub url: String,
    pub markdown: String,
    pub title: Option<String>,
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
        let mut markdown = PageToMarkdown::convert(&html, req.include_images)?;

        if let Some(max) = req.max_length {
            if markdown.len() > max {
                markdown = format!("{}\n\n[truncated]", &markdown[..max]);
            }
        }

        // Extract <title> as a cheap heuristic
        let title = html
            .find("<title>")
            .and_then(|start| {
                let rest = &html[start + 7..];
                rest.find("</title>").map(|end| rest[..end].trim().to_string())
            });

        Ok(McpResponse {
            url: req.url,
            markdown,
            title,
        })
    }
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
                max_length: None,
            })
            .await
            .unwrap();

        assert_eq!(resp.title, Some("Test Page".to_string()));
        assert!(resp.markdown.contains("Hello"));
        assert!(resp.markdown.contains("World"));
        mock.assert_async().await;
    }
}
