# SPEC

## Scope

Web2MD is a tool that fetches web pages and returns them as Markdown. It is optimized for MCP (Model Context Protocol) integration where token efficiency is critical. It also functions as a minimal terminal browser (Lynx-like) for human use.

## Non-Goals

- Full DOM rendering / browser engine semantics
- Screenshot or PDF generation
- Session/cookie persistence across requests

## Optional JS Execution

When enabled (`--javascript` / `enable_javascript`), inline `<script>` blocks are evaluated by the project's own dependency-free interpreter (`src/js/`) — no `boa`, `v8`, or other external engine. A pragmatic JS subset is supported (variables, closures, control flow, template literals, `document.write`, strings, arrays, `Math`, `JSON`). Unsupported features fail fast and are skipped, so a script can never break conversion. External (`src=`) and module scripts are not executed.

## CLI

```bash
# Default: interactive terminal browser (Lynx-like)
web2md <URL>
# Controls: [1-N] follow link, [b]ack, [f]orward, [u] enter URL, [q]uit

# One-shot fetch to stdout
web2md fetch <URL> [FLAGS]
  --max-length N       Truncate output after N characters
  --timeout SECONDS    Request timeout (default: 30)
  --include-images     Emit Markdown image references
  --cookie NAME=VAL    Send cookie (repeatable)
  --header "Name: Val" Send custom header (repeatable)
  --format markdown    Output as Markdown (default)
  --format html        Output raw HTML
  --format json        Output structured JSON (markdown + metadata)
  --render             ANSI colors: bold headings, underlined links, colored code
  --delay MS           Polite delay between requests in milliseconds
  --keep-header        Preserve <header> tags (stripped by default)
  --cache-ttl SECONDS  Cache fetched pages for N seconds (0 = disabled)
  --main-content       Extract only <article>, <main>, or [role=main] content
  -o, --output FILE    Write output to file instead of stdout
  --frontmatter         Prepend YAML frontmatter (metadata) to Markdown output
  --exclude-selector SEL  Strip HTML elements matching .class or #id selector (repeatable)
  --javascript          Execute inline <script> blocks via the built-in JS interpreter

# Sitemap/feed discovery
web2md sitemap <URL> [FLAGS]
  --timeout SECONDS    Request timeout (default: 30)
  --cookie NAME=VAL    Send cookie (repeatable)
  --header "Name: Val" Send custom header (repeatable)
  --feeds              Also check HTML page for RSS/Atom feed links

# Batch convert multiple URLs
web2md batch <FILE> [FLAGS]
  --timeout SECONDS    Request timeout (default: 30)
  --include-images     Emit Markdown image references
  --cookie NAME=VAL    Send cookie (repeatable)
  --header "Name: Val" Send custom header (repeatable)
  --delay MS           Polite delay between requests in milliseconds
  --keep-header        Preserve <header> tags (stripped by default)
  --cache-ttl SECONDS  Cache fetched pages for N seconds (0 = disabled)
  --main-content       Extract only <article>, <main>, or [role=main] content
  -o, --output DIR     Write Markdown files to directory (default: stdout)
  --frontmatter         Prepend YAML frontmatter (metadata) to each Markdown output
  --exclude-selector SEL  Strip HTML elements matching .class or #id selector (repeatable)
  --javascript          Execute inline <script> blocks via the built-in JS interpreter

# MCP server (stdio JSON-RPC)
web2md mcp
```

### MCP JSON-RPC Request

```json
{
  "url": "https://example.com/article",
  "include_images": false,
  "keep_header": false,
  "main_content": false,
  "max_length": 4000
}
```

### MCP JSON-RPC Response

```json
{
  "url": "https://example.com/article",
  "markdown": "# Article Title\n\nBody content...",
  "title": "Article Title",
  "description": "A summary of the article",
  "author": "Jane Doe",
  "published_date": "2025-01-15T08:30:00Z",
  "image": "https://example.com/cover.jpg",
  "headline": "Breaking News Story",
  "site_name": "Tech Blog",
  "keywords": ["Rust", "Web Scraping", "Markdown"]
}
```

`description`, `author`, `published_date`, `image`, `headline`, `site_name`, and `keywords` are optional — omitted when the page has no corresponding meta tags or structured data. `author` is extracted from `<meta name="author">` or JSON-LD `author` (string or `{"name":"..."}` object). `published_date` is extracted from `<meta property="article:published_time">`, `<time datetime="...">`, or JSON-LD `datePublished` (in priority order). `image` is extracted from `<meta property="og:image">` or JSON-LD `image` (string, `{"url":"..."}` object, or array — first item used). `headline` is extracted from JSON-LD `headline`. `site_name` is extracted from `<meta property="og:site_name">`. `keywords` is extracted from multiple `<meta property="article:tag">` tags, `<meta name="keywords">` (comma-separated), or JSON-LD `keywords` (string or array), in priority order.

### CLI `--format json` Output

```json
{
  "markdown": "# Article Title\n\nBody content...",
  "title": "Article Title",
  "description": "A summary of the article",
  "author": "Jane Doe",
  "published_date": "2025-01-15T08:30:00Z",
  "image": "https://example.com/cover.jpg",
  "headline": "Breaking News Story",
  "site_name": "Tech Blog",
  "keywords": ["Rust", "Web Scraping", "Markdown"]
}
```

Same metadata fields as the MCP response, minus the `url` field. Omitted fields are excluded from the JSON output (not `null`).

## HTML Processing Pipeline

1. **Browser.fetch()** → raw HTML
2. **Browser.inline_iframes()** → replace `<iframe src="...">` with fetched content
3. **PageToMarkdown.convert()** → Markdown
   - Extract main content if `main_content` is true (`<article>`, `<main>`, `[role="main"]`, or readability fallback: text-density scoring of top-level `<div>`/`<section>` blocks, then paragraph-level sliding window scoring of `<p>` blocks)
   - Strip `<script>`, `<style>`, `<iframe>`
   - Strip `<nav>`, `<footer>`, `<aside>`, `<noscript>`, `<form>`, `<header>` (unless `keep_header`), HTML comments
   - Extract code languages from `<code class="language-xxx">`
   - Strip `<img>` unless `include_images` is true
   - Inject languages into fenced code blocks (` ```rust `)
   - Deduplicate repeated paragraph-level blocks (>20 chars, first occurrence kept)
   - Collapse excessive whitespace
   - Extract comments from forum/thread pages (detects `class="comment"`, `id="comment-N"`, `data-testid="comment"`, `data-author`; extracts author + text + nesting depth; appends as `## Comments` section with blockquotes and indentation)
   - Absolutize links: convert relative URLs in `[text](url)` patterns to absolute URLs using the page URL as base
4. **render_markdown_ansi()** → ANSI-styled terminal output (when `--render` or `browse`)
   - Headings: bold + color-coded by level
   - Links: underlined cyan (with `[N]` numbers in browse mode)
   - Tables: box-drawing characters (`┌─┬─┐`)
   - Code: light gray on dark background
   - Blockquotes: gray bar + italic

## Error Handling

- Invalid URL → immediate error
- HTTP non-2xx → error with status code
- Timeout → error after 30s default (configurable)
- Malformed HTML → best-effort Markdown extraction
- Iframe fetch failure → silently omitted (no error)

## Quality Bar

- All features have unit tests
- `cargo test` must pass before merge
- Warnings noted but not blocking
