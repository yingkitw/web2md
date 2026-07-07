# Web2MD

[![Crates.io](https://img.shields.io/crates/v/web2md)](https://crates.io/crates/web2md)
[![docs.rs](https://docs.rs/web2md/badge.svg)](https://docs.rs/web2md)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org)
[![MCP](https://img.shields.io/badge/MCP-compatible-purple.svg)](#mcp-server)
[![GitHub](https://img.shields.io/badge/github-yingkitw%2Fweb2md-lightgrey.svg)](https://github.com/yingkitw/web2md)

A tool that fetches web pages and returns them as Markdown. Designed to minimize token usage when invoked via MCP (Model Context Protocol).

## Why Markdown instead of HTML?

HTML is built for browsers, not for reasoning. A typical article page carries far more markup than meaning:

| What you get with raw HTML | What Web2MD gives you |
|---|---|
| `<div class="sidebar-ad">`, inline styles, `<script>` blocks | Content only — nav, ads, footers, and scripts stripped |
| Nested tags around every word (`<span><strong>…</strong></span>`) | Flat, readable text with `#` headings and `-` lists |
| Relative links (`/docs/guide`) that agents cannot follow | Absolute URLs ready for the next fetch |
| 5–10× more tokens for the same article | Compact Markdown that fits more pages in context |

**Markdown preserves what matters for reading and reasoning** — headings, paragraphs, lists, tables, code blocks, and links — without the DOM ceremony. LLMs are trained on vast amounts of Markdown (GitHub, docs, forums), so they parse structure and intent from it more reliably than from tag soup.

**Browsing in Markdown is a deliberate trade:** you lose pixel-perfect layout and client-side rendering, but you gain a representation optimized for *understanding* and *acting on* web content. For research, summarization, citation, and multi-step agent workflows, that trade is almost always worth it.

## Value for AI agents

Agents consume the web through tools. Every page fetch costs context window, latency, and money. Web2MD is built around that constraint:

- **Token efficiency** — `--main-content` extraction, noise stripping, and deduplication shrink pages before they hit the model. An agent can read several articles in the space one raw HTML dump would occupy.
- **MCP-native** — Run `web2md mcp` as a stdio JSON-RPC server. Agents call a single `fetch` tool and receive Markdown plus structured metadata (title, author, date, description, keywords) in one response.
- **Actionable links** — Relative URLs are absolutized so an agent can follow numbered links in terminal browse mode or chain fetches across a site without guessing base paths.
- **Structured output** — `--format json` and YAML frontmatter (`--frontmatter`) give agents machine-readable metadata alongside prose, useful for citations, filtering, and downstream pipelines.
- **Polite crawling** — `--delay`, caching (`--cache-ttl`), and batch mode let research agents process URL lists without hammering servers or re-fetching the same page.
- **Auth for gated content** — Cookies and custom headers (`--cookie`, `--header`) let agents reach documentation, dashboards, or member-only pages when credentials are provided.

In short: **Web2MD turns the web into a format agents can read, reason over, and act on** — without burning context on markup nobody needs.

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

# Crawl same-origin links up to 2 hops and write Markdown files
cargo run -- fetch https://example.com --depth 2 --output ./pages

# Interactive terminal browser (explicit)
cargo run -- browse https://example.com

# Run as MCP server (stdio JSON-RPC)
cargo run -- mcp
```

## MCP Server

Web2MD exposes a stdio JSON-RPC `fetch` tool for MCP clients (Cursor, Claude Desktop, custom agents). Send a URL; receive Markdown plus metadata in one round trip:

```json
{ "url": "https://example.com/article", "main_content": true, "max_length": 4000 }
```

Response includes `markdown`, `title`, `description`, `author`, `published_date`, and other fields when present on the page. See [SPEC.md](SPEC.md) for the full request/response schema.

Example Cursor MCP config:

```json
{
  "mcpServers": {
    "web2md": {
      "command": "web2md",
      "args": ["mcp"]
    }
  }
}
```

## Features

- **Interactive terminal browser** (`browse`): Lynx-like navigation with numbered links, back/forward history
- **ANSI rendering** (`--render`): Bold headings, underlined cyan links, colored code blocks in terminal output
- **Table rendering**: Markdown tables drawn with box-drawing characters (`┌─┬─┐`)
- **Iframe inlining**: Fetches `<iframe src="...">` content and embeds it into the parent page
- **Noise reduction**: Strips `<script>`, `<style>`, `<iframe>`, `<nav>`, `<footer>`, `<aside>`, `<noscript>`, `<form>`, `<header>`, HTML comments, and excessive whitespace (use `--keep-header` to preserve `<header>`)
- **URL blacklist**: Skips known ad/tracking/analytics URLs on iframe inlining, batch jobs, and sitemap output (use `--no-blacklist` to disable)
- **Recursive crawl** (`--depth N`): BFS crawl of same-origin links from a start URL; writes one Markdown file per page to `--output` directory or prints separated sections to stdout
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
- **CSS selector targeting** (`--exclude-selector` flag): Strip HTML elements matching `.class` or `#id` selectors before conversion — remove ads, sidebars, and other noise elements
- **Optional JavaScript execution** (`--javascript` flag): Inline `<script>` blocks run through the project's own dependency-free interpreter (`src/js/`) and `document.write` output is folded into the page. No `boa`/`v8` dependency; unsupported scripts are skipped silently.

## Architecture

- **Browser** (`browser.rs`): Minimal HTTP client with iframe inlining. Optional inline-JS execution via a built-in, dependency-free interpreter.
- **JS interpreter** (`src/js/`): Lexer + recursive-descent parser + tree-walking evaluator. When `--javascript` is set, inline `<script>` blocks run and `document.write` output is folded into the page. No `boa`/`v8` dependency.
- **PageToMarkdown** (`markdown.rs`): HTML-to-Markdown conversion. Strips scripts, styles, iframes, images (optional).
- **McpServer** (`mcp.rs`): JSON-RPC server wrapper exposing a `fetch` tool.
- **CLI** (`main.rs`): `fetch` (one-shot), `browse` (interactive), `sitemap` (URL discovery), `batch` (bulk convert), `mcp` (server). Default mode is `browse`.

See [ARCHITECTURE.md](ARCHITECTURE.md) for details.

## Project Status

See [TODO.md](TODO.md) for remaining work and [SPEC.md](SPEC.md) for protocol contracts.
