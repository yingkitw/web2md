# SPEC

## Scope

BrowseDown is a headless browser crate that fetches web pages and returns them as Markdown. It is optimized for MCP (Model Context Protocol) integration where token efficiency is critical. It also functions as a minimal terminal browser (Lynx-like) for human use.

## Non-Goals

- JavaScript execution (no DOM rendering)
- Screenshot or PDF generation
- Session/cookie persistence across requests

## CLI

```bash
# Default: interactive terminal browser (Lynx-like)
browsedown <URL>
# Controls: [1-N] follow link, [b]ack, [f]orward, [u] enter URL, [q]uit

# One-shot fetch to stdout
browsedown fetch <URL> [FLAGS]
  --max-length N       Truncate output after N characters
  --timeout SECONDS    Request timeout (default: 30)
  --include-images     Emit Markdown image references
  --cookie NAME=VAL    Send cookie (repeatable)
  --header "Name: Val" Send custom header (repeatable)
  --format markdown    Output as Markdown (default)
  --format html        Output raw HTML
  --render             ANSI colors: bold headings, underlined links, colored code

# MCP server (stdio JSON-RPC)
browsedown mcp
```

### MCP JSON-RPC Request

```json
{
  "url": "https://example.com/article",
  "include_images": false,
  "max_length": 4000
}
```

### MCP JSON-RPC Response

```json
{
  "url": "https://example.com/article",
  "markdown": "# Article Title\n\nBody content...",
  "title": "Article Title"
}
```

## HTML Processing Pipeline

1. **Browser.fetch()** → raw HTML
2. **Browser.inline_iframes()** → replace `<iframe src="...">` with fetched content
3. **PageToMarkdown.convert()** → Markdown
   - Strip `<script>`, `<style>`, `<iframe>`
   - Strip `<img>` unless `include_images` is true
   - Collapse excessive whitespace
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
