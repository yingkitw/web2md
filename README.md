# Web2MD

A tool that fetches web pages and returns them as Markdown. Designed to minimize token usage when invoked via MCP (Model Context Protocol).

## Why?

Raw HTML is noisy: scripts, styles, ads, and markup bloat consume LLM context window. Web2MD fetches a page and converts it to clean Markdown, preserving content hierarchy while stripping everything non-essential.

## Quick Start

```bash
# Interactive terminal browser (default — Lynx-like)
cargo run -- https://example.com
# [1-20] follow numbered link  [b]ack  [f]orward  [u] enter URL  [q]uit

# Fetch a page as Markdown (one-shot)
cargo run -- fetch https://example.com

# Limit output length
cargo run -- fetch https://example.com --max-length 4000

# Render with ANSI colors in the terminal (bold headings, underlined links)
cargo run -- fetch https://example.com --render

# Output structured JSON (markdown + metadata) for scripting
cargo run -- fetch https://example.com --format json

# Add a polite delay between requests (milliseconds)
cargo run -- fetch https://example.com --delay 500

# Interactive terminal browser (explicit)
cargo run -- browse https://example.com

# Run as MCP server (stdio JSON-RPC)
cargo run -- mcp
```

## Features

- **Interactive terminal browser** (`browse`): Lynx-like navigation with numbered links, back/forward history
- **ANSI rendering** (`--render`): Bold headings, underlined cyan links, colored code blocks in terminal output
- **Table rendering**: Markdown tables drawn with box-drawing characters (`┌─┬─┐`)
- **Iframe inlining**: Fetches `<iframe src="...">` content and embeds it into the parent page
- **Noise reduction**: Strips `<script>`, `<style>`, `<iframe>`, `<nav>`, `<footer>`, `<aside>`, `<noscript>`, `<form>`, `<header>`, HTML comments, and excessive whitespace (use `--keep-header` to preserve `<header>`)
- **Content deduplication**: Removes duplicate paragraph-level blocks to further reduce token output
- **Main content extraction** (`--main-content`): Extracts `<article>`, `<main>`, or `[role="main"]` content; falls back to readability scoring (text-density vs link-density) on `<div>`/`<section>` blocks, then paragraph-level sliding window scoring on `<p>` blocks for pages without semantic tags
- **Code language detection**: Preserves language annotations from `<code class="language-xxx">` as fenced block languages (` ```rust `)
- **Auth support**: Cookies (`--cookie`) and custom headers (`--header`) for authenticated pages
- **Rate limiting** (`--delay`): Polite delay between consecutive requests to avoid hammering servers
- **Caching** (`--cache-ttl`): In-memory cache with configurable TTL to avoid re-fetching the same URL
- **MCP server**: stdio JSON-RPC transport for LLM tool integration
- **Metadata extraction**: Title, description, author (meta tag or JSON-LD), publication date, image (og:image or JSON-LD), headline (JSON-LD), site name (og:site_name), and keywords/tags (article:tag, meta keywords, or JSON-LD) returned in MCP response and `--format json` output
- **JSON output** (`--format json`): Emit structured JSON (markdown + metadata) from CLI for scripting and piping
- **Comments extraction**: Detects forum/thread pages (Reddit, WordPress, vBulletin) and extracts comments with author attribution, nesting depth, and blockquote formatting
- **Link URL absolutization**: Converts relative URLs in Markdown links to absolute URLs using the page URL as base, so links are usable in LLM contexts
- **Sitemap/feed discovery** (`sitemap` subcommand): Fetches `sitemap.xml` from a website and lists all discovered URLs; optionally discovers RSS/Atom feed links from the HTML page (`--feeds` flag)
- **Batch processing** (`batch` subcommand): Reads URLs from a file (one per line, `#` comments supported) and converts each to Markdown; use `--output <dir>` to write files to a directory
- **Output to file** (`--output` flag): Write `fetch` output to a file instead of stdout
- **YAML frontmatter** (`--frontmatter` flag): Prepend metadata (title, description, author, date, image, site name, keywords) as a YAML block at the top of Markdown output — useful for static site generators and LLM context

## Architecture

- **Browser** (`browser.rs`): Minimal HTTP client with iframe inlining. No rendering engine—intentionally lightweight.
- **PageToMarkdown** (`markdown.rs`): HTML-to-Markdown conversion. Strips scripts, styles, iframes, images (optional).
- **McpServer** (`mcp.rs`): JSON-RPC server wrapper exposing a `fetch` tool.
- **CLI** (`main.rs`): `fetch` (one-shot), `browse` (interactive), `sitemap` (URL discovery), `batch` (bulk convert), `mcp` (server). Default mode is `browse`.

See [ARCHITECTURE.md](ARCHITECTURE.md) for details.

## Project Status

See [TODO.md](TODO.md) for remaining work and [SPEC.md](SPEC.md) for protocol contracts.
