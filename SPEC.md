# SPEC

## Scope

Web2MD is a tool that fetches web pages and returns them as Markdown. It is optimized for MCP (Model Context Protocol) integration where token efficiency is critical. It also functions as a minimal terminal browser (Lynx-like) for human use.

## Non-Goals

- Full DOM rendering / browser engine semantics (no headless Chrome/Firefox)
- Screenshot or PDF generation
- Session/cookie persistence across requests
- Replacing the in-house HTML-to-Markdown converter with third-party crates (`htmd`, `html2md`, etc.)

## Technical Stack

| Component | Implementation |
|---|---|
| HTTP client | `reqwest` + `tokio` |
| HTML parsing | `scraper` 0.23 (html5ever) in `html_to_md.rs` |
| Markdown rendering | `pulldown-cmark` (ANSI terminal via `render_markdown_ansi`) |
| Language detection | `whatlang` (ISO 639-3 fallback when HTML metadata lacks language) |
| HTML utilities | `html_util.rs` — case-insensitive search, entity decoding |
| Conversion pipeline | `markdown.rs` (`PageToMarkdown`) wraps `html_to_md::parse_html` |
| CLI | `clap` 4.x |
| Test HTTP mocking | `mockito` (dev) |

No dedicated HTML-to-Markdown crate — conversion is implemented in-house with pre/post-processing in `PageToMarkdown`.

## Optional JS Execution

When enabled (`--javascript` / `enable_javascript`), inline `<script>` blocks are evaluated by the project's own dependency-free interpreter (`src/js/`) — no `boa`, `v8`, or other external engine. A pragmatic JS subset is supported (variables, closures, control flow, template literals, `document.write`, `setTimeout`, `setInterval`, `clearTimeout`, `clearInterval`, `requestAnimationFrame`, strings, arrays, `Math`, `JSON`). Timer callbacks run when their scheduled time ≤ `--wait` (milliseconds). Unsupported features fail fast and are skipped, so a script can never break conversion. External (`src=`) and module scripts are not executed.

## URL Blacklist

By default (`filter_blacklisted_urls: true`), Web2MD skips known non-content URLs:

- **Iframe inlining**: blacklisted `src` URLs are not fetched (empty replacement)
- **Batch processing**: blacklisted URLs in the input file are skipped with a log message
- **Sitemap output**: blacklisted URLs are filtered from printed results

Primary user-requested URLs (explicit `fetch` or browse navigation) are always fetched. Use `--no-blacklist` on `fetch`, `browse`, or `batch` to disable filtering.

### Custom blacklist file

When URL filtering is enabled, Web2MD also loads `~/.web2md/blacklist.txt` if it exists. Each non-empty, non-comment line is a host suffix (e.g. `evil-tracker.com`) or path fragment (e.g. `/ads/`). Use `--blacklist-file <path>` for additional pattern files and `--no-user-blacklist` to skip the default file.

## Recursive Crawl

`fetch --depth N` performs a breadth-first crawl of same-origin links starting from the given URL:

- **Depth 0** (default): single-page fetch (existing behavior)
- **Depth N > 0**: fetch the start page, extract `<a href>` links on the same host, convert each to Markdown, and repeat up to N link hops
- External links, `mailto:`, fragments, and blacklisted URLs are not followed
- Output: `--output <dir>` writes one `.md` file per page; without `--output`, pages are printed separated by `---` headers
- Requires markdown output format (`--format json` / `--format html` are incompatible with `--depth`)

## robots.txt

By default, Web2MD fetches `/robots.txt` once per origin and:

- **Blocks** fetches to paths matched by a `Disallow` rule for `*` or the Web2MD user-agent
- **Waits** the greater of `--delay` and `Crawl-delay` (seconds) between requests to that host
- Missing or unreadable robots.txt → all paths allowed
- `/robots.txt` itself is always fetchable (no circular check)
- Use `--ignore-robots` on `fetch`, `browse`, or `batch` to disable

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
  --format text        Output plain text (Markdown syntax stripped)
  --format csv         Output Trafilatura-style CSV (header + one data row)
  --format tei         Output XML-TEI document (teiHeader + body paragraphs)
  --format xml         Output plain Trafilatura-style XML (<doc> + <main>)
  --lang CODE          Require page language to match ISO 639-1 or 639-3 (e.g. en, eng)
  --render             ANSI colors: bold headings, underlined links, colored code
  --delay MS           Polite delay between requests in milliseconds
  --keep-header        Preserve <header> tags (stripped by default)
  --cache-ttl SECONDS  Cache fetched pages for N seconds (0 = disabled)
  --main-content       Extract only <article>, <main>, or [role=main] content
  -o, --output FILE    Write output to file instead of stdout
  --frontmatter         Prepend YAML frontmatter (metadata) to Markdown output
  --exclude-selector SEL  Strip HTML elements matching .class or #id selector (repeatable)
  --javascript          Execute inline <script> blocks via the built-in JS interpreter
  --wait MS             Post-load wait in milliseconds (also caps setTimeout callback delay)
  --no-blacklist        Disable URL blacklist filtering for ads/tracking pixels
  --blacklist-file PATH Additional blacklist pattern file (repeatable)
  --no-user-blacklist   Do not load ~/.web2md/blacklist.txt
  --depth N             Recursively crawl same-origin links up to N levels (markdown only)
  --ignore-robots       Ignore robots.txt disallow rules and crawl-delay

# Sitemap/feed discovery
web2md sitemap <URL> [FLAGS]
  --timeout SECONDS    Request timeout (default: 30)
  --cookie NAME=VAL    Send cookie (repeatable)
  --header "Name: Val" Send custom header (repeatable)
  --feeds              Also check HTML page for RSS/Atom/JSON Feed links

# Fetch RSS/Atom/JSON Feed and convert entries to Markdown
web2md feed <URL> [FLAGS]
  --timeout SECONDS    Request timeout (default: 30)
  --cookie NAME=VAL    Send cookie (repeatable)
  --header "Name: Val" Send custom header (repeatable)
  --max-entries N      Limit number of entries included
  --json               Emit structured JSON instead of Markdown
  -o, --output FILE    Write output to file instead of stdout

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
  --wait MS             Post-load wait in milliseconds (also caps setTimeout callback delay)
  --no-blacklist        Disable URL blacklist filtering for ads/tracking pixels
  --blacklist-file PATH Additional blacklist pattern file (repeatable)
  --no-user-blacklist   Do not load ~/.web2md/blacklist.txt
  --ignore-robots       Ignore robots.txt disallow rules and crawl-delay

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
  "keywords": ["Rust", "Web Scraping", "Markdown"],
  "categories": ["Technology", "Open Source"],
  "excerpt": "Opening paragraph text truncated to ~160 characters…",
  "canonical_url": "https://example.com/article",
  "language": "en",
  "extraction_quality": 0.86,
  "page_type": "article",
  "fingerprint": "a1b2c3d4e5f60718"
}
```

`description`, `author`, `published_date`, `image`, `headline`, `site_name`, `keywords`, `categories`, `excerpt`, `canonical_url`, `language`, `extraction_quality`, `page_type`, and `fingerprint` are optional — omitted when the page has no corresponding meta tags or structured data. Title falls back to Dublin Core `DC.title` / `dcterms.title` when `<title>` is absent. `author` is extracted from `<meta name="author">`, JSON-LD `author` (string or `{"name":"..."}` object), or Dublin Core `DC.creator` / `dcterms.creator`. `published_date` is extracted from `<meta property="article:published_time">`, `<time datetime="...">`, JSON-LD `datePublished`, or Dublin Core `DC.date` / `dcterms.date` (in priority order). `description` also falls back to `DC.description` / `dcterms.description`. `image` is extracted from `<meta property="og:image">` or JSON-LD `image` (string, `{"url":"..."}` object, or array — first item used). `headline` is extracted from JSON-LD `headline`. `site_name` is extracted from `<meta property="og:site_name">`. `keywords` is extracted from multiple `<meta property="article:tag">` tags, `<meta name="keywords">` (comma-separated), or JSON-LD `keywords` (string or array), in priority order. `categories` is extracted from multiple `<meta property="article:section">` tags or JSON-LD `articleSection` (string or array). `excerpt` is generated from the first substantive `<p>` paragraph (≥40 chars, truncated to ~160). `canonical_url` comes from `<meta property="og:url">` or `<link rel="canonical">`. `language` comes from `<html lang>`, `og:locale`, or JSON-LD `inLanguage`; when those are absent, `whatlang` detects an ISO 639-3 code from extracted text (minimum length and reliability gates). `extraction_quality` is a 0.0–1.0 confidence score from Markdown length, structure, semantic HTML, metadata, and link density. `page_type` is one of `article`, `forum`, `product`, or `page`. `fingerprint` is a 64-bit simhash of extracted plain text (16 hex chars) for near-duplicate detection. CLI `--lang CODE` rejects pages whose language does not match the requested ISO 639-1 or 639-3 code.

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
  "keywords": ["Rust", "Web Scraping", "Markdown"],
  "categories": ["Technology", "Open Source"],
  "excerpt": "Opening paragraph text truncated to ~160 characters…",
  "canonical_url": "https://example.com/article",
  "language": "en",
  "extraction_quality": 0.86,
  "page_type": "article",
  "fingerprint": "a1b2c3d4e5f60718"
}
```

Same metadata fields as the MCP response, minus the `url` field. Omitted fields are excluded from the JSON output (not `null`).

### CLI `--format csv` Output

Trafilatura-style CSV with a header row and one data row:

```
url,title,author,published_date,language,page_type,extraction_quality,fingerprint,text
https://example.com/article,Article Title,Jane Doe,2025-01-15T08:30:00Z,en,article,0.86,a1b2c3d4e5f60718,"Plain text body..."
```

Fields containing commas, quotes, or newlines are RFC 4180–escaped. The `text` column is plain text (Markdown stripped).

### CLI `--format tei` Output

Trafilatura-style TEI XML with metadata in `teiHeader` and plain-text paragraphs in `div type="entry"`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<TEI xmlns="http://www.tei-c.org/ns/1.0">
  <teiHeader>
    <fileDesc>
      <titleStmt>
        <title>Article Title</title>
        <author>Jane Doe</author>
      </titleStmt>
      <publicationStmt>
        <publisher>Tech Blog</publisher>
        <date when="2025-01-15T08:30:00Z">2025-01-15T08:30:00Z</date>
        <idno type="URL">https://example.com/article</idno>
      </publicationStmt>
      <sourceDesc>
        <p>Converted from web page by web2md</p>
      </sourceDesc>
    </fileDesc>
    <profileDesc>
      <langUsage>
        <language ident="en"/>
      </langUsage>
    </profileDesc>
  </teiHeader>
  <text>
    <body>
      <div type="entry">
        <p>Plain text body...</p>
      </div>
    </body>
  </text>
</TEI>
```

Special characters in text and attribute values are XML-escaped. Language, page type, extraction quality, and fingerprint are included when available.

### CLI `--format xml` Output

Trafilatura-style plain XML:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<doc>
  <url>https://example.com/article</url>
  <title>Article Title</title>
  <author>Jane Doe</author>
  <date>2025-01-15T08:30:00Z</date>
  <language>en</language>
  <fingerprint>a1b2c3d4e5f60718</fingerprint>
  <main>
    <p>Plain text body...</p>
  </main>
</doc>
```

## HTML Processing Pipeline

1. **Browser.fetch()** → raw HTML
2. **Browser.inline_iframes()** → replace `<iframe src="...">` with fetched content (blacklisted URLs skipped)
3. **Browser.post_load_wait()** → sleep `--wait` milliseconds after fetch (optional)
4. **Browser.run_inline_scripts()** → evaluate inline `<script>` blocks when `--javascript` / `enable_javascript` (optional); flush `setTimeout`, `setInterval`, and `requestAnimationFrame` callbacks up to `--wait`
5. **PageToMarkdown.convert()** → Markdown
   - Detect page type (`article` / `forum` / `product` / `page`) and apply extraction profile: article/product prefer main-content; product prefers keeping images
   - Extract main content if `main_content` is true or the profile prefers it (Trafilatura-style fallback: score semantic tags with bonus, top-level blocks, paragraph clusters; pick best candidate; strip boilerplate; fall back to JSON-LD `articleBody` / `description` or Open Graph description when heuristics score ≤ 100)
   - Strip `<script>`, `<style>`, `<iframe>`
   - Strip `<nav>`, `<footer>`, `<aside>`, `<noscript>`, `<form>`, `<header>` (unless `keep_header`), HTML comments
   - Strip elements matching `--exclude-selector` (`.class` or `#id`)
   - Extract code languages from `<code class="language-xxx">`
   - Strip `<img>` unless `include_images` is true or the product profile prefers images
   - **html_to_md::parse_html()** — DOM walk via `scraper`/html5ever:
     - Headings, paragraphs, links, images, lists, tables, blockquotes, inline bold/italic
     - HTML entity decoding (`&amp;`, `&#169;`, etc.)
     - Markdown control-character escaping in plain text (`*`, `#`, `_`, etc.)
     - Tolerant of malformed/unclosed tags (html5ever tree repair)
   - Inject languages into fenced code blocks (` ```rust `)
   - Deduplicate repeated paragraph-level blocks (>20 chars, first occurrence kept)
   - Collapse excessive whitespace
   - Append JSON-LD Product details (`## Product details`) for product pages when name/brand/SKU/price are available
   - Extract comments from forum/thread pages (detects `class="comment"`, `id="comment-N"`, `data-testid="comment"`, `data-author`; extracts author + text + nesting depth; appends as `## Comments` section with blockquotes and indentation)
5. **PageToMarkdown.absolutize_links()** → convert relative URLs in `[text](url)` patterns to absolute URLs using the page URL as base
6. **PageToMarkdown.to_plain_text()** → strip Markdown syntax when `--format text` (optional; uses `pulldown-cmark` event walk)
7. **render_markdown_ansi()** → ANSI-styled terminal output (when `--render` or `browse`)
   - Headings: bold + color-coded by level
   - Links: underlined cyan (with `[N]` numbers in browse mode)
   - Tables: box-drawing characters (`┌─┬─┐`)
   - Code: light gray on dark background
   - Blockquotes: gray bar + italic
   - `fix_raw_links()` post-pass catches multi-line `[text](url)` patterns

## Error Handling

- Invalid URL → immediate error
- HTTP non-2xx → error with status code
- Timeout → error after 30s default (configurable)
- Malformed HTML → best-effort Markdown extraction via html5ever tree repair (`scraper`)
- Iframe fetch failure → silently omitted (no error)

## Quality Bar

- All features have unit tests (221 tests across lib, main, and integration suites)
- `cargo test` must pass before merge
- Warnings noted but not blocking
