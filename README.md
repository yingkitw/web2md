# Web2MD

[![Crates.io](https://img.shields.io/crates/v/web2md)](https://crates.io/crates/web2md)
[![docs.rs](https://docs.rs/web2md/badge.svg)](https://docs.rs/web2md)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org)
[![MCP](https://img.shields.io/badge/MCP-compatible-purple.svg)](#mcp-server)
[![GitHub](https://img.shields.io/badge/github-yingkitw%2Fweb2md-lightgrey.svg)](https://github.com/yingkitw/web2md)

Turn web pages into clean Markdown — locally, without API keys, proxies, or LLM credits.

Web2MD fetches a URL, strips ads, navigation, scripts, and boilerplate, and returns the article as readable Markdown. Built for terminal users and AI agents that need web content they can reason over without burning context tokens on HTML markup.

## Why Web2MD?

| Raw HTML | Web2MD output |
|---|---|
| Nested `<div>`/`<span>` markup, inline styles, ad slots | Flat Markdown with headings, lists, code blocks, and links |
| Relative URLs that agents cannot follow | Absolute URLs ready for the next fetch |
| 5–10× more tokens for the same article | Compact Markdown that fits more pages in context |
| Gated by SaaS quotas and API keys | Runs locally, offline, deterministic |

LLMs are trained on Markdown from GitHub, docs, and forums. They parse structure and intent from Markdown more reliably than from tag soup. Web2MD makes that the default representation for web content.

## Install

```bash
cargo install web2md
```

Or build from source:

```bash
git clone https://github.com/yingkitw/web2md
cd web2md
cargo build --release
```

Optional heavier features (build separately):

```bash
# Mozilla Readability.js article isolation
cargo install web2md --features readability

# Headless Chrome for JavaScript-heavy SPAs (requires Chrome at runtime)
cargo install web2md --features headless
```

## Quick start

```bash
# Fetch a page as Markdown
web2md fetch https://example.com

# Terminal browser with numbered links (Lynx-like)
web2md https://example.com

# Isolate the article body (requires the readability feature)
web2md fetch https://example.com/article --readability

# JSON output for scripts and agents
web2md fetch https://example.com --format json

# Crawl a site 2 levels deep into ./pages
web2md fetch https://example.com --depth 2 --output ./pages
```

## Common recipes

### Use with AI agents (MCP)

Web2MD ships a stdio MCP server. Add it to your client config:

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

Call the `fetch` tool:

```json
{ "url": "https://example.com/article", "main_content": true, "max_length": 4000 }
```

Response includes `markdown`, `title`, `description`, `author`, `published_date`, `canonical_url`, `language`, `excerpt`, and extraction `quality`.

### Extract only what matters

```bash
# Query-focused paragraphs (LLM-free)
web2md fetch https://blog.rust-lang.org/2026/01/... --topic "rust cargo"

# Extractive summary, 5 sentences
web2md fetch https://en.wikipedia.org/wiki/Rust --summary 5

# Token-budget output
web2md fetch https://example.com/article --max-tokens 800
```

### Search and batch

```bash
# DuckDuckGo search, fetch top 3 results as Markdown
web2md search "rust web scraping" --limit 3 --fetch

# Convert a list of URLs to Markdown files
web2md batch urls.txt --output ./pages
```

### Output formats

```bash
web2md fetch https://example.com --format json   # markdown + metadata
web2md fetch https://example.com --format text   # plain text
web2md fetch https://example.com --format csv   # corpus row
web2md fetch https://example.com --format tei    # XML-TEI
web2md fetch https://example.com --format xml    # Trafilatura-style XML
web2md fetch https://example.com --frontmatter   # YAML frontmatter on Markdown
```

## Features at a glance

- **In-house HTML-to-Markdown** converter via `scraper`/html5ever
- **Real-time streaming** — download progress on stderr + incremental Markdown blocks to stdout as they are converted
- **Optional Mozilla Readability.js** article isolation (`--features readability --readability`)
- **Optional headless Chrome** for SPAs (`--features headless`)
- **Main-content extraction**, noise stripping, content deduplication
- **Query-focused extraction** (`--topic`), extractive summarization (`--summary`), token-budget truncation (`--max-tokens`)
- **Recipe/FAQ/Job/Event** JSON-LD extractors (`--type`)
- **Persistent file cache**, per-host rate limiting, `robots.txt` respect
- **Recursive crawl**, sitemap discovery
- **Links**, **images**, **product**, and **branding** extraction (`--format`)
- Page **diff**, **watch** mode, **webhook** delivery
- **PII redaction**, proxy support, basic auth, mobile User-Agent
- **Local BM25 corpus** index over Markdown directories
- **MCP server** with structured metadata

See [SPEC.md](SPEC.md) and [ARCHITECTURE.md](ARCHITECTURE.md) for full protocol and design details.

## Comparison

|  | Web2MD | Firecrawl | Context7 |
|---|---|---|---|
| Cost | Free, local | Subscription | Free tier + paid |
| API key | None | Required | Required |
| Markdown extraction | ✅ | ✅ | ❌ |
| Query highlights | ✅ LLM-free | ✅ LLM | ✅ LLM |
| Summary | ✅ LLM-free | ✅ LLM | ❌ |
| Offline / CI | ✅ | ❌ | ❌ |

## Tech stack

Rust, `reqwest`, `tokio`, `scraper` (html5ever), `clap`, `serde`, `pulldown-cmark`. Optional: `headless_chrome` (`--features headless`), `readabilityrs` (`--features readability`).

## License

Apache 2.0
