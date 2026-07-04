# SPEC

## Scope

Web2MD is a tool that fetches web pages and returns them as Markdown. It is optimized for MCP (Model Context Protocol) integration where token efficiency is critical. It also functions as a minimal terminal browser (Lynx-like) for human use.

## Non-Goals

- JavaScript execution (no DOM rendering)
- Screenshot or PDF generation
- Session/cookie persistence across requests

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
  "headline": "Breaking News Story"
}
```

`description`, `author`, `published_date`, `image`, and `headline` are optional — omitted when the page has no corresponding meta tags or structured data. `author` is extracted from `<meta name="author">` or JSON-LD `author` (string or `{"name":"..."}` object). `published_date` is extracted from `<meta property="article:published_time">`, `<time datetime="...">`, or JSON-LD `datePublished` (in priority order). `image` is extracted from `<meta property="og:image">` or JSON-LD `image` (string, `{"url":"..."}` object, or array — first item used). `headline` is extracted from JSON-LD `headline`.

### CLI `--format json` Output

```json
{
  "markdown": "# Article Title\n\nBody content...",
  "title": "Article Title",
  "description": "A summary of the article",
  "author": "Jane Doe",
  "published_date": "2025-01-15T08:30:00Z",
  "image": "https://example.com/cover.jpg",
  "headline": "Breaking News Story"
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
