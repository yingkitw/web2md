# SPEC

## Scope

Web2MD is a tool that fetches web pages and returns them as Markdown. It is optimized for MCP (Model Context Protocol) integration where token efficiency is critical. It also functions as a minimal terminal browser (Lynx-like) for human use.

## Non-Goals

- Full DOM rendering / browser engine semantics in the default build (headless Chrome is an opt-in `--features headless` backend)
- Screenshot or PDF generation
- Session/cookie persistence across requests
- Replacing the in-house HTML-to-Markdown converter with third-party crates (`htmd`, `html2md`, etc.)

## Technical Stack

| Component | Implementation |
|---|---|
| HTTP client | `reqwest` + `tokio` |
| HTML parsing | `scraper` 0.23 (html5ever) in `html_to_md.rs` |
| Markdown rendering | `pulldown-cmark` (ANSI terminal via `render_markdown_ansi`) |
| Language detection | Built-in stopword heuristic (ISO 639-3 fallback when HTML metadata lacks language) |
| HTML utilities | `html_util.rs` — case-insensitive search, entity decoding |
| Persistent cache keys | `sha2` (SHA-256 of URL → JSON file) |
| Optional regex | `regex` (PII redaction, brand/design extraction) |
| Conversion pipeline | `markdown/` (`PageToMarkdown`) wraps `html_to_md::parse_html` |
| Output transforms | `transform.rs` — `--topic`, `--summary`, `--max-tokens`, paragraph splitter |
| Domain-specific extractors | `structured.rs` — Recipe, FAQPage, QAPage, JobPosting, Event, HowTo, Speakable → Markdown |
| Page diffing | `diff_markdown.rs` — LCS-based unified diff for the `diff` subcommand |
| Persistent cache | `persistent_cache.rs` — JSON files keyed by URL hash; `--cache-dir` |
| Brand/design extraction | `branding.rs` — top-N colors / fonts / heading sizes from inline `<style>` blocks |
| CLI | `clap` 4.x |
| Test HTTP mocking | `mockito` (dev) |

No dedicated HTML-to-Markdown crate — conversion is implemented in-house with pre/post-processing in `PageToMarkdown`.

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
- Pages at each BFS level are fetched **in parallel** (up to 10 concurrent requests)
- External links, `mailto:`, fragments, and blacklisted URLs are not followed
- Output: `--output <dir>` writes one `.md` file per page; without `--output`, pages are printed separated by `---` headers
- Requires `--format markdown` (default), `--format llms-txt`, or `--format atom` (`--format json` / `--format html` are incompatible with `--depth`)
- **`--sitemap-only`**: with `--depth > 0`, fetch `{origin}/sitemap.xml`, recursively expand any nested `<sitemapindex>` documents, and convert those URLs only — do not follow in-page links (≈ Firecrawl sitemap-only crawl)
- **`--max-pages N`**: stop after converting N pages (applies to both link-following and sitemap-only crawls)
- **`--path-prefix` / `--path-exclude`**: restrict crawl targets by URL path prefix (e.g. `--path-prefix /docs --path-exclude /docs/internal`)

## robots.txt

robots.txt is **off by default**. Enable with `--respect-robots` (or `BrowserOptions { respect_robots_txt: true, .. }`). When enabled, Web2MD fetches `/robots.txt` once per origin and:

- **Blocks** fetches to paths matched by a `Disallow` rule for `*` or the Web2MD user-agent
- **Waits** the greater of `--delay` and `Crawl-delay` (seconds) between requests to that host
- Missing or unreadable robots.txt → all paths allowed
- `/robots.txt` itself is always fetchable (no circular check)
- The `--ignore-robots` flag is a no-op (kept for backward compatibility)

## CLI

```bash
# Default: interactive terminal browser (Lynx-like)
web2md <URL>
# Keys: Space/+/-/j/k scroll pages, g/G top/end, /pat search, n/N next/prev match,
#       1-N follow link, u enter URL, b/f back/forward, r reload, s save page as .md,
#       a bookmark page, v list bookmarks, L list links, y copy URL (OSC 52),
#       m toggle raw/ansi view, ? help, q quit

# One-shot fetch to stdout
web2md fetch <URL> [FLAGS]
  --max-length N       Truncate output after N characters
  --max-tokens N       Cap output by approximate token budget (≈ 4 chars / token); prefers paragraph boundaries
  --timeout SECONDS    Request timeout (default: 30)
  --include-images     Emit Markdown image references
  --no-comments         Skip forum/thread comment extraction
  --no-tables           Strip HTML tables from output
  --no-links            Emit link text only (strip Markdown [text](url))
  --cookie NAME=VAL    Send cookie (repeatable)
  --cookie-file PATH   Load cookies from a Netscape or name=value file
  --header "Name: Val" Send custom header (repeatable)
  --format markdown    Output as Markdown (default)
  --format html        Output raw HTML
  --format json        Output structured JSON (markdown + metadata)
  --format text        Output plain text (Markdown syntax stripped)
  --format csv         Output Trafilatura-style CSV (header + one data row)
  --format tei         Output XML-TEI document (teiHeader + body paragraphs)
  --format xml         Output plain Trafilatura-style XML (<doc> + <main>)
  --format branding    Output deterministic brand/design profile (top colors / fonts / heading sizes)
  --format links       Output all links as JSON (≈ Firecrawl `links` format)
  --format images      Output all images as JSON (≈ Firecrawl `images` format)
  --format product     Output structured product from JSON-LD (≈ Firecrawl `product` format, deterministic)
  --format video       Output videos as JSON (title/thumbnail/duration when present)
  --format audio       Output audio clips as JSON (title/duration when present)
  --format attributes  Output HTML attribute values (requires `--attr selector:attribute`)
  --format menu        Output restaurant menu from JSON-LD Menu (≈ Firecrawl `menu`, deterministic)
  --format breadcrumbs Output JSON-LD BreadcrumbList as JSON
  --format meta        Output Open Graph / Twitter / Dublin Core / standard meta as JSON
  --format llms-txt    Output single-page llms.txt-style summary (title, description, links)
  --format atom        Output Atom 1.0 feed (single page, or multi-page with --depth / --sitemap-only)
  --type TYPE          Domain extractor: recipe | faq | job | event | howto | qapage | speakable | course | software
  --topic QUERY        Keep only paragraphs relevant to this query (LLM-free; ≈ Firecrawl `highlights`)
  --summary N          Return top-N sentences by TF-IDF + positional scoring (LLM-free; ≈ Firecrawl `summary`)
  --readability        Apply Mozilla Readability.js (requires --features readability) before conversion
  --headless           Render the page with a real headless Chrome / Chromium (requires --features headless)
  --chrome-path PATH   Path to a Chrome / Chromium binary (used with --headless)
  --wait-for-selector SEL  Wait for CSS selector after headless navigation (≈ Firecrawl waitFor)
  --scroll [N]         Scroll to bottom N times for lazy-load (requires --headless; default 3)
  --lang CODE          Require page language to match ISO 639-1 or 639-3 (e.g. en, eng)
  --precision           Favor extraction precision (less noise, stricter main-content)
  --recall              Favor extraction recall (more text, looser main-content)
  --only-with-metadata  Require title and published_date or fail
  --render             ANSI colors: bold headings, underlined links, colored code
  --delay MS           Polite delay between requests in milliseconds
  --rate RPS           Per-host requests-per-second cap (independent rate clock per host)
  --cache-ttl SECONDS  Cache fetched pages for N seconds (0 = disabled)
  --cache-dir DIR      Persist fetched pages as JSON files (sha256 → file) under DIR; survives restarts
  --no-cache           Bypass cache read/write for this request (≈ Firecrawl `ignoreCache`)
  --cache-max-age N    Only reuse cache entries younger than N seconds (≈ Firecrawl `minAge`)
  --webhook URL        POST result JSON to this webhook URL after fetch (n8n/Make/Zapier)
  --include-selector SEL  Keep only HTML elements matching CSS selector (e.g. article, .content); repeatable
  --include-tags TAG      Keep only these HTML tag names (repeatable; ≈ Firecrawl includeTags)
  --attr SEL:ATTR      For `--format attributes`: CSS selector + attribute (e.g. a:href); repeatable
  --pii-redact         Redact PII (emails, phones, SSNs, credit cards) from output
  --mobile             Use mobile User-Agent for the request
  --proxy URL          Route requests through HTTP/SOCKS proxy (e.g. http://proxy:8080, socks5://proxy:1080)
  --auth USER:PASS     Basic authentication credentials (format: user:password)
  --skip-tls-verify    Accept invalid TLS certificates (corporate MITM / self-signed proxies)
  --main-content       Extract only <article>, <main>, or [role=main] content
  -o, --output FILE    Write output to file instead of stdout
  --frontmatter         Prepend YAML frontmatter (metadata) to Markdown output
  --exclude-selector SEL  Strip HTML elements matching .class or #id selector (repeatable)
  --exclude-tags TAG      Strip HTML tag names before conversion (repeatable; ≈ Firecrawl excludeTags)
  --no-blacklist        Disable URL blacklist filtering for ads/tracking pixels
  --blacklist-file PATH Additional blacklist pattern file (repeatable)
  --no-user-blacklist   Do not load ~/.web2md/blacklist.txt
  --depth N             Recursively crawl same-origin links up to N levels (markdown only, parallel BFS)
  --sitemap-only        With --depth > 0: fetch sitemap.xml URLs only (no page-link following)
  --max-pages N         Cap crawl at N pages (with --depth / --sitemap-only)
  --path-prefix PATH    Only crawl URLs under this path prefix (repeatable)
  --path-exclude PATH   Skip crawl URLs under this path prefix (repeatable)
  --remove-base64-images  Strip data: (base64) image URIs from output
  --respect-robots      Honor robots.txt Disallow and Crawl-delay (off by default)
  --ignore-robots       No-op (robots.txt is off by default; kept for backward compatibility)

# Sitemap discovery
web2md sitemap <URL> [FLAGS]
  --timeout SECONDS    Request timeout (default: 30)
  --cookie NAME=VAL    Send cookie (repeatable)
  --header "Name: Val" Send custom header (repeatable)

Recursively expands `<sitemapindex>` nested sitemaps up to a depth limit and emits leaf page URLs.

# Discover all URLs on a page by extracting <a href> links (≈ Firecrawl /map)
web2md map <URL> [FLAGS]
  --timeout SECONDS    Request timeout
  --cookie NAME=VAL    Send cookie (repeatable)
  --header "Name: Val" Send custom header (repeatable)
  --same-origin        Only list URLs on the same origin as the target
  --json               Output as JSON array instead of one URL per line
  --ignore-robots      No-op (robots.txt is off by default; kept for backward compatibility)

# Web search via DuckDuckGo (no API key required; ≈ Firecrawl /search, free)
web2md search <QUERY> [FLAGS]
  -t, --timeout SECONDS  Request timeout
  -l, --limit N          Maximum number of results to return
  --json                 Output as JSON array instead of Markdown
  --fetch                Fetch and convert each result URL to Markdown
  --include-domains D    Only include results from these domains (repeatable)
  --exclude-domains D    Exclude results from these domains (repeatable)
  --cookie NAME=VAL      Send cookie (repeatable)
  -H, --header "Name: Val"  Send custom header (repeatable)

# Batch convert multiple URLs
web2md batch <FILE> [FLAGS]
  --timeout SECONDS    Request timeout (default: 30)
  --include-images     Emit Markdown image references
  --cookie NAME=VAL    Send cookie (repeatable)
  --cookie-file PATH   Load cookies from a Netscape or name=value file
  --header "Name: Val" Send custom header (repeatable)
  --delay MS           Polite delay between requests in milliseconds
  --keep-header        Preserve <header> tags (stripped by default)
  --cache-ttl SECONDS  Cache fetched pages for N seconds (0 = disabled)
  --main-content       Extract only <article>, <main>, or [role=main] content
  -o, --output DIR     Write Markdown files to directory (default: stdout)
  --frontmatter         Prepend YAML frontmatter (metadata) to each Markdown output
  --exclude-selector SEL  Strip HTML elements matching .class or #id selector (repeatable)
  --exclude-tags TAG      Strip HTML tag names before conversion (repeatable; ≈ Firecrawl excludeTags)
  --no-blacklist        Disable URL blacklist filtering for ads/tracking pixels
  --blacklist-file PATH Additional blacklist pattern file (repeatable)
  --no-user-blacklist   Do not load ~/.web2md/blacklist.txt
  --ignore-robots       No-op (robots.txt is off by default; kept for backward compatibility)
  --links-summary        Append deduplicated link list at end of Markdown output
  --images-summary       Append deduplicated image list at end of Markdown output
  --chunk                Split Markdown by headings for RAG pipelines (sections separated by ---)
  --proxy URL           Route requests through HTTP/SOCKS proxy
  --auth USER:PASS      Basic authentication credentials
  --skip-tls-verify     Accept invalid TLS certificates

# Build (or rebuild) a BM25 index over a directory of Markdown files
web2md corpus index <DIR> [--output PATH]

# Query a corpus directory for a free-form query (returns top-N with snippets)
web2md corpus query <DIR> <QUERY> [-l LIMIT] [--json]

# Fetch README + metadata from a package registry (≈ Context7, no API key)
web2md docs <NAME> [FLAGS]
  -r, --registry NAME    Registry: crates, npm, or pypi (default: crates)
  -t, --timeout SECONDS  Request timeout
  --json                 Output structured JSON instead of Markdown

# MCP server (stdio JSON-RPC)
web2md mcp

# Peek at a URL: return title + excerpt + key metadata only (cheaper than `fetch`)
web2md peek <URL> [FLAGS]
  --json                     Emit structured JSON instead of plain text
  --timeout SECONDS          Request timeout
  --cookie NAME=VAL          Send cookie (repeatable)
  --header "Name: Val"       Send custom header (repeatable)
  --delay MS                 Polite delay between requests
  --ignore-robots            No-op (robots.txt is off by default)
  --no-blacklist             Disable URL blacklist filtering
  --proxy URL                Route requests through HTTP/SOCKS proxy
  --auth USER:PASS           Basic authentication credentials

# Diff two URLs at the Markdown level (or use --cached-b for a local file)
web2md diff <URL_A> <URL_B> [FLAGS]
  --cached-b                 Treat URL_B as a path to a local Markdown file
  --json                     Emit machine-readable JSON summary
  --timeout SECONDS          Request timeout
  --cookie NAME=VAL          Send cookie (repeatable)
  --header "Name: Val"       Send custom header (repeatable)

# Poll a URL on an interval and emit whenever the content fingerprint (simhash) changes
web2md watch <URL> [FLAGS]
  --every SECONDS            Poll interval (default: 300)
  --cache-dir DIR            Persist fingerprints (and body when --diff/--webhook) across restarts
  --diff                     On change, emit unified Markdown diff vs previous body
  --webhook URL              POST JSON change event on fingerprint change
  --timeout SECONDS          Request timeout
  --cookie NAME=VAL          Send cookie (repeatable)
  --header "Name: Val"       Send custom header (repeatable)
  --ignore-robots            No-op (robots.txt is off by default)
```

### MCP JSON-RPC Request
```json
{
  "url": "https://example.com/article",
  "include_images": false,
  "keep_header": false,
  "main_content": false,
  "max_length": 4000,
  "format": "video",
  "attr": ["a:href", "img:src"]
}
```

Optional `format` values: `markdown` (default), `video`, `audio`, `images`, `links`, `product`, `menu`, `branding`, `attributes`. Use `attr` with `attributes`.

### MCP JSON-RPC Response

When a `format` is requested, the response includes a `result` field containing the structured data and `markdown` is empty:

```json
{
  "url": "https://example.com/article",
  "markdown": "",
  "result": [
    {"url": "https://example.com/clip.mp4", "source": "video", "title": "Demo", "duration": 90}
  ],
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

`description`, `author`, `published_date`, `image`, `headline`, `site_name`, `keywords`, `categories`, `excerpt`, `canonical_url`, `language`, `extraction_quality`, `page_type`, and `fingerprint` are optional — omitted when the page has no corresponding meta tags or structured data. Title falls back to Dublin Core `DC.title` / `dcterms.title` when `<title>` is absent. `author` is extracted from `<meta name="author">`, JSON-LD `author` (string or `{"name":"..."}` object), or Dublin Core `DC.creator` / `dcterms.creator`. `published_date` is extracted from `<meta property="article:published_time">`, `<time datetime="...">`, JSON-LD `datePublished`, or Dublin Core `DC.date` / `dcterms.date` (in priority order). `description` also falls back to `DC.description` / `dcterms.description`. `image` is extracted from `<meta property="og:image">` or JSON-LD `image` (string, `{"url":"..."}` object, or array — first item used). `headline` is extracted from JSON-LD `headline`. `site_name` is extracted from `<meta property="og:site_name">`. `keywords` is extracted from multiple `<meta property="article:tag">` tags, `<meta name="keywords">` (comma-separated), or JSON-LD `keywords` (string or array), in priority order. `categories` is extracted from multiple `<meta property="article:section">` tags or JSON-LD `articleSection` (string or array). `excerpt` is generated from the first substantive `<p>` paragraph (≥40 chars, truncated to ~160). `canonical_url` comes from `<meta property="og:url">` or `<link rel="canonical">`. `language` comes from `<html lang>`, `og:locale`, or JSON-LD `inLanguage`; when those are absent, a stopword heuristic detects an ISO 639-3 code from extracted text (minimum length and score gates). `extraction_quality` is a 0.0–1.0 confidence score from Markdown length, structure, semantic HTML, metadata, and link density. `page_type` is one of `article`, `forum`, `product`, or `page`. `fingerprint` is a 64-bit simhash of extracted plain text (16 hex chars) for near-duplicate detection. CLI `--lang CODE` rejects pages whose language does not match the requested ISO 639-1 or 639-3 code.

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
3. **apply_readability()** → (optional, `--features readability --readability`) Mozilla Readability.js article isolation
4. **filter_by_include_selectors()** → (optional, `--include-selector`) keep only matching elements
5. **PageToMarkdown.convert()** → Markdown
   - Apply `ConvertOptions`: `--precision` / `--recall` adjust main-content score thresholds and boilerplate stripping; `--no-comments` skips forum comment append
   - Detect page type (`article` / `forum` / `product` / `page`) and apply extraction profile: article/product prefer main-content; product prefers keeping images
   - Extract main content if `main_content` is true or the profile prefers it (Trafilatura-style fallback: score semantic tags with bonus, top-level blocks, paragraph clusters; pick best candidate; strip boilerplate; fall back to JSON-LD `articleBody` / `description` or Open Graph description when heuristics score ≤ 100)
   - Strip `<script>`, `<style>`, `<iframe>`
   - Strip `<nav>`, `<footer>`, `<aside>`, `<noscript>`, `<form>`, `<header>` (unless `keep_header`), HTML comments
   - Strip boilerplate elements by `class`/`id` keywords (cookie, consent, gdpr, social, share, breadcrumb, newsletter, subscribe, popup, modal, overlay, toolbar, advert, related-posts, etc.)
   - Strip elements matching `--exclude-selector` (`.class` or `#id`)
   - Extract code languages from `<code class="language-xxx">`
   - Strip `<img>` unless `include_images` is true or the product profile prefers images
   - **html_to_md::parse_html()** — DOM walk via `scraper`/html5ever:
     - Headings, paragraphs, links, images, lists, tables, blockquotes, inline bold/italic
     - HTML entity decoding (`&amp;`, `&#169;`, etc.)
     - Markdown control-character escaping in plain text (`*`, `#`, `_`, etc.)
     - Tolerant of malformed/unclosed tags (html5ever tree repair)
   - Inject languages into opening fenced code blocks only (` ```rust `; closing fences stay bare)
   - Deduplicate repeated paragraph-level blocks (>20 chars, first occurrence kept)
   - Clean Markdown noise: Wikipedia citation markers (`^([[ N ]](#cite_note-…))`), `[edit]` section links, `[File:…]`/`[Image:…]` links, heading self-anchor unwrapping (`### [Title](#anchor) ###` → `### Title ###`)
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

## Output Transforms (`transform.rs`)

Apply **after** HTML → Markdown conversion, before output:

- `--topic <query>`: keep only paragraphs whose plain-text matches query tokens (case-insensitive substring + token overlap); preserves original order; fails fast if nothing matches. LLM-free.
- `--summary <n>`: rank sentences by TF-IDF with title-weighted terms + early-position bonus; emit the top-N sentences in source order. LLM-free.
- `--max-tokens <N>`: cap Markdown length by approximate token budget (≈ 4 chars / token); prefers paragraph boundaries; falls back to character cut with `[truncated]` marker.

## Domain-Specific Extractors (`structured.rs`)

Deterministic rendering of JSON-LD blocks (no LLM, no schema to define):

- `--type recipe`: JSON-LD `Recipe` → ingredient bullet list + numbered `recipeInstructions` + YAML frontmatter (title, description, author, yield, category, cuisine, prep_minutes, cook_minutes, total_minutes).
- `--type faq`: JSON-LD `FAQPage` `mainEntity` array → Q+A as Markdown headings (each `Question` = heading, `acceptedAnswer`/`suggestedAnswer` = body).
- `--type qapage`: JSON-LD `QAPage` (falls back to `FAQPage`) → same Q+A Markdown shape.
- `--type speakable`: JSON-LD `SpeakableSpecification` / `speakable.cssSelector` → scrape matching element text into Markdown sections.
- `--type course`: JSON-LD `Course` → name, provider, course code, language, description.
- `--type software`: JSON-LD `SoftwareApplication` / `WebApplication` → category, OS, version, price, rating, description.
- `--type job`: JSON-LD `JobPosting` → title, company, location, salary (formatted as `amount currency /unit`), date_posted, apply URL, description. Frontmatter + labeled body.
- `--type event`: JSON-LD `Event` → name, start/end date, venue (name + address), ticket URL, price.
- `--type howto`: JSON-LD `HowTo` → steps, tools, supplies.

When the JSON-LD is missing or ambiguous (multiple distinct blocks of the same type), the extractor returns `None` or `Err`; the CLI falls back to plain Markdown conversion with a stderr notice.

## Persistent Cache

When `--cache-dir <dir>` is set alongside `--cache-ttl`, fetched pages persist as `{sha256(url)}.json` files inside `dir`. Same TTL semantics as the in-memory cache; survives process restarts; shared across runs. `--cache-dir` with `cache_ttl = 0` disables both layers.

- `--no-cache`: skip both cache lookup and store for this request (always hit the live URL).
- `--cache-max-age N`: only reuse an entry if it is younger than N seconds (stricter than `--cache-ttl` for the current request).

## Per-host Rate Limiting

`--rate <req/s>` enforces an independent token-bucket per host. The heavier of the global `--delay` (or robots.txt `Crawl-delay`) and the per-host clock applies on every request. Per-host clocks are keyed by host name only, so two different hosts queried in parallel will not block each other.

## Brand/Design Profile (`--format branding`)

`extract_branding` walks inline `<style>` blocks (no external stylesheet fetches) and emits a JSON profile:

- `color_scheme` (`"light"` / `"dark"` / `"unknown"`) — guessed from average luminance of the top color.
- `colors` — top-20 hex tokens by frequency (normalized; `#fff` → `#ffffff`).
- `fonts` — distinct font families referenced in any `font-family:` declaration.
- `background_colors` — distinct tokens used as `background` or `background-color`.
- `heading_sizes` — `font-size:` declarations scoped to `h1` / `h2` / `h3` selectors, in source order.

No LLM, no JS rendering, fully deterministic. Cost is the same as `fetch` plus serialization of the profile.

## Links Extraction (`--format links`)

`extract_links` walks all `<a href>` tags in the HTML and emits a JSON array of `{url, text}` objects:

- Relative URLs are resolved to absolute using the page URL as base.
- Fragment-only (`#...`), empty, and `data:` hrefs are skipped.
- Links are deduplicated by URL (first occurrence kept) in document order.
- Link text is extracted from the anchor's inner HTML (tags stripped, entities decoded).

No LLM, fully deterministic (≈ Firecrawl `links` format, free).

## Images Extraction (`--format images`)

`extract_images` walks all `<img>` tags and CSS `background-image` / `background: url(...)` references (inline `style` attributes and `<style>` blocks) and emits a JSON array of `{src, alt?, title?}` objects:

- Relative `src` URLs are resolved to absolute using the page URL as base.
- `data:` URLs are skipped.
- Images are deduplicated by `src` (first occurrence kept) in document order.
- `alt` and `title` are included when present and non-empty (CSS backgrounds have neither).

No LLM, fully deterministic (≈ Firecrawl `images` format + v2.11 background image extraction, free).

## Product Extraction (`--format product`)

`extract_product` reads JSON-LD `Product` blocks and emits a structured JSON object:

```json
{
  "name": "Widget",
  "brand": "Acme",
  "description": "...",
  "category": "Tools",
  "sku": "W-100",
  "mpn": "...",
  "gtin": "...",
  "image": "https://...",
  "url": "https://...",
  "variants": [
    {"price": "19.99", "currency": "USD", "availability": "https://schema.org/InStock"}
  ]
}
```

- First `Product` JSON-LD block wins; subsequent blocks are ignored.
- `offers` can be a single `Offer` object or an array — each becomes a variant.
- Returns an error if no `Product` JSON-LD is found on the page.

No LLM, fully deterministic (≈ Firecrawl `product` format, free).

## Video Extraction (`--format video`)

`extract_videos` collects `<video src>`, nested `<source>`, known iframe embeds (YouTube/Vimeo/…), and JSON-LD `VideoObject` blocks into a JSON array of `{url, source?, title?, thumbnail?, duration?}`:

- `thumbnail` comes from the `<video poster>` attribute or JSON-LD `thumbnailUrl`.
- `title` comes from the element `title` attribute or JSON-LD `name`.
- `duration` is seconds (plain number or ISO-8601 `PT…`).
- JSON-LD entries enrich matching HTML videos or add standalone entries.

No LLM, fully deterministic (≈ Firecrawl `document.videos[]`, free).

## Audio Extraction (`--format audio`)

`extract_audios` collects `<audio src>`, nested `<source>`, known iframe embeds (SoundCloud/Spotify/Apple Podcasts/…), and JSON-LD `AudioObject` blocks into a JSON array of `{url, source?, title?, duration?}`:

- `source` is `audio` for `<audio src>`, `source` for `<source>`, the embed platform for known players, or `json-ld` for JSON-LD entries.
- `title` comes from the element `title` attribute or JSON-LD `name`.
- `duration` is seconds (plain number or ISO-8601 `PT…`).
- JSON-LD entries enrich matching HTML audio or add standalone entries.

No LLM, fully deterministic (≈ Firecrawl `document.audio[]`, free).

## Attributes Extraction (`--format attributes`)

`extract_attributes` takes one or more `--attr selector:attribute` pairs (split on the last `:`) and emits:

```json
[
  {"selector": "a", "attribute": "href", "values": ["/about", "/contact"]},
  {"selector": "img", "attribute": "src", "values": ["/hero.png"]}
]
```

Uses `scraper` CSS selectors. Values are trimmed, non-empty, and deduplicated in document order. Requires at least one `--attr` (≈ Firecrawl `attributes` format, free).

## Menu Extraction (`--format menu`)

`extract_menu` reads JSON-LD `Menu` / `MenuSection` / `MenuItem` blocks (including `@graph`) and emits:

```json
{
  "name": "Dinner",
  "currency": "USD",
  "sections": [
    {"name": "Mains", "items": [{"name": "Pasta", "price": "14.50", "currency": "USD"}]}
  ]
}
```

Deterministic restaurant-menu extraction (≈ Firecrawl `menu` format without LLM). Errors if no Menu JSON-LD is present.

## Breadcrumbs Extraction (`--format breadcrumbs`)

`extract_breadcrumbs` reads JSON-LD `BreadcrumbList` / `ListItem` blocks and emits:

```json
{
  "items": [
    {"name": "Home", "url": "https://example.com/", "position": 1},
    {"name": "Docs", "url": "https://example.com/docs", "position": 2}
  ]
}
```

Items are sorted by `position` when present. Relative item URLs are resolved against the page URL. Errors if no BreadcrumbList is found.

## Meta Dump (`--format meta`)

`extract_meta_dump` collects page metadata without Markdown conversion:

```json
{
  "open_graph": {"og:title": ["Hello"], "article:author": ["Ada"]},
  "twitter": {"twitter:card": ["summary"]},
  "dublin_core": {"DC.title": ["Hello"]},
  "standard": {"description": ["A page"], "keywords": ["a,b"]},
  "html_lang": "en",
  "canonical": "https://example.com/page"
}
```

Groups: Open Graph (`og:` / `article:` properties), Twitter Cards (`twitter:` name/property), Dublin Core (`DC.` / `dcterms.`), and remaining standard `<meta name>` tags. Duplicate keys accumulate as arrays. Also includes `html lang` and `<link rel="canonical">`.

## Llms.txt Summary (`--format llms-txt`)

`to_llms_txt` emits a single-page [llms.txt](https://llmstxt.org)-style Markdown summary:

```markdown
# Page Title

> Meta description when present

Source: https://example.com/page

## Links

- [Label](https://example.com/a)
```

Uses page title / description from metadata and up to 50 extracted links. Available via CLI and MCP (`format: "llms-txt"`).

With `--depth N` (or `--sitemap-only`), emits a **multi-page site index** instead: each crawled page becomes a `## Pages` entry (`title` + optional description from meta). Writes `llms.txt` under `--output DIR` when set; otherwise prints to stdout. Metadata-only (no full Markdown conversion) for speed.

## Atom Feed (`--format atom`)

Emits an Atom 1.0 XML feed. Single-page mode creates one `<entry>` from page title/description/`published_date`. With `--depth` / `--sitemap-only`, each crawled page becomes an entry (metadata-only). Writes `feed.atom` under `--output DIR` when set.

## Cookie File (`--cookie-file`)

`--cookie-file PATH` loads cookies from a simple `name=value` file or a Netscape cookie jar. Lines starting with `#` are ignored (except Netscape `#HttpOnly_` rows). Merged with repeatable `--cookie` flags. Available on `fetch`, `browse`, and `batch`.

## Skip TLS Verify (`--skip-tls-verify`)

When set, `reqwest` is built with `danger_accept_invalid_certs(true)` so corporate MITM / self-signed proxies work. Available on `fetch`, `browse`, and `batch`. Prefer installing the corporate CA when possible.

## Include Selector (`--include-selector`)

`--include-selector <SEL>` keeps only HTML elements matching the given CSS selector(s) before conversion. Multiple selectors can be given (repeatable). Uses `scraper`'s CSS selector engine (html5ever). If no matches are found, the original HTML is used unchanged. Complements `--exclude-selector` (≈ Firecrawl `includeTags`).

## Include Tags (`--include-tags`)

`--include-tags <TAG>` keeps only matching HTML elements (e.g. `article`, `main`) before conversion — tag-name form of Firecrawl `includeTags`. Repeatable; alphanumeric names only. Merged with `--include-selector` into the same keep-filter.

## Exclude Tags (`--exclude-tags`)

`--exclude-tags <TAG>` strips matching HTML element pairs (e.g. `figure`, `svg`) before conversion — ≈ Firecrawl `excludeTags`. Repeatable. Only alphanumeric tag names are accepted. Complements built-in noise-tag stripping (`nav`/`footer`/`aside`/…) and `--exclude-selector` (class/id).

## PII Redaction (`--pii-redact`)

When `--pii-redact` is set, the output is passed through regex-based PII redaction after conversion:

- **Emails** → `[REDACTED_EMAIL]`
- **SSNs** (US format `NNN-NN-NNNN`) → `[REDACTED_SSN]`
- **Credit cards** (13–19 digit groups with spaces or dashes) → `[REDACTED_CC]`
- **Phone numbers** (US and international) → `[REDACTED_PHONE]`

Applied after all other transforms (topic, summary, truncation). No LLM, fully deterministic (≈ Firecrawl PII redaction, 4 cr → free).

## Mobile User-Agent (`--mobile`)

When `--mobile` is set, the HTTP client sends an iPhone Safari User-Agent string instead of the default Web2MD desktop UA. Useful for sites that serve different content to mobile devices (≈ Firecrawl `mobile: true`).

## Proxy Support (`--proxy`)

When `--proxy <URL>` is set, all HTTP requests are routed through the specified proxy. Supports HTTP, HTTPS, and SOCKS5 proxy URLs:

- `http://proxy:8080` — HTTP proxy
- `https://proxy:8443` — HTTPS proxy
- `socks5://proxy:1080` — SOCKS5 proxy

Configured at the `reqwest::ClientBuilder` level via `reqwest::Proxy::all()`, so all requests including iframe inlining, robots.txt fetches, and persistent cache revalidation go through the proxy. Available on `fetch`, `peek`, and `batch` commands (≈ Firecrawl proxy support).

## Basic Authentication (`--auth`)

When `--auth user:password` is set, HTTP Basic Auth credentials are sent with every request via `reqwest::RequestBuilder::basic_auth()`. Applied per-request in `fetch_raw()` so it covers all fetches including iframes and robots.txt. Available on `fetch`, `peek`, and `batch` commands.

## Default request headers

Every fetch sends browser-like defaults unless overridden with `--header`:

- `Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8`
- `Accept-Language: en-US,en;q=0.9`

`Accept-Encoding` is left to `reqwest` so transparent decompression keeps working.

## URL Discovery (`map` subcommand)

`map <URL>` fetches a page and extracts all `<a href>` URLs:

- URLs are resolved to absolute and deduplicated.
- `--same-origin` filters to only URLs on the same scheme + host as the target.
- `--json` outputs a JSON array of URL strings; default is one URL per line.
- `--ignore-robots` is a no-op (robots.txt is off by default).

No LLM, fully deterministic (≈ Firecrawl `/map` endpoint, free).

## Web Search (`search` subcommand)

`search <QUERY>` performs a web search via DuckDuckGo's HTML endpoint — no API key required:

1. Builds URL `https://html.duckduckgo.com/html/?q=<encoded_query>&kp=1&kl=us-en&s=0&df=&v_q=<limit>`
2. Fetches the HTML response via the standard `Browser` HTTP client.
3. Parses `result__a` class links for titles and URLs, and `result__snippet` class elements for snippets.
4. Decodes DuckDuckGo redirect URLs (`//duckduckgo.com/l/?uddg=<encoded>`) to extract the actual target URL.
5. `--limit N` truncates to the first N results.
6. `--json` outputs a JSON array of `{title, url, snippet}` objects; default is Markdown with numbered links and blockquote snippets.
7. `--fetch` additionally fetches each result URL and converts it to Markdown, emitting `{index, title, url, snippet, markdown}` JSON or `---\n## N. [Title](URL)\n\n<markdown>` sections.

No API key, no subscription, fully local HTTP (≈ Firecrawl `/search`, free).

## Page Diffing (`diff` subcommand)

`diff <URL_A> <URL_B> [--cached-b]` computes a line-level unified diff between two Markdown renderings:

- Both URLs are fetched with the default options, converted to Markdown, and compared.
- `--cached-b <path>` skips the second fetch and reads `<path>` as a local Markdown file (e.g. a previous `--output` artifact).
- `--json` emits a summary `{url_a, url_b, lines_added, lines_removed, diff}` instead of the raw diff.
- The engine is a custom O(n·m) longest-common-subsequence over lines — no external crate dependency.

## Watch Mode (`watch` subcommand)

`watch <URL> [--every SECONDS] [--cache-dir DIR] [--diff] [--webhook URL]` polls a URL in a loop and emits whenever the content fingerprint (simhash) changes:

1. Fetch + convert → compute `content_fingerprint(text)`.
2. Compare with the previously-seen fingerprint.
3. If different:
   - default: print `timestamp\turl\tfingerprint\tsnippet` to stdout
   - `--diff`: print a unified Markdown diff vs the previous body (first change without a prior body dumps the baseline)
   - `--webhook URL`: POST `{event:"watch.change", url, timestamp, fingerprint, snippet, diff, baseline}`
4. `--cache-dir DIR` persists the last-seen fingerprint (and body when `--diff` / `--webhook`) across restarts so reboots don't re-fire.
5. Failures (timeouts, non-2xx) are printed to stderr but do not stop the loop.

Emits nothing while the page is unchanged — agents can `tail -f` it as a firehose.

## Streaming Output

By default, `fetch` streams in real time: download progress (byte count) is printed to stderr as chunks arrive, and once the full body is received, Markdown blocks are emitted to stdout incrementally as they are converted from the parsed DOM. This gives immediate feedback that the fetch is in progress and produces visible Markdown output as soon as the first content block is ready.

Streaming is active for plain Markdown output (`--format markdown`, the default). When any of the following flags are set, the normal batch path runs instead: `--format` (non-markdown), `--type`, `--topic`, `--summary`, `--max-tokens`, `--max-length`, `--frontmatter`, `--pii-redact`, `--output`, `--webhook`, `--headless`.

## Readability Extraction (`--readability`, `--features readability`)

When `--readability` is passed to `fetch` (and the crate is built with `--features readability`), the raw HTML is first scored and cleaned by [`readabilityrs`] — a Rust port of Mozilla's Readability library (the same algorithm behind Firefox Reader View). The article HTML it returns replaces the original body before our usual pipeline runs, so noise, nav, ads, and chrome are stripped deterministically by a battle-tested extractor. Readability's pre-flight check (`is_probably_readerable`) is also exposed via [`is_readerable`] for callers that want to short-circuit obvious non-articles. When Readability declines to score the page (returns `None` or fails to construct), the input HTML is passed through unchanged so the rest of the pipeline still has a chance.

## Headless Browser (`--headless`, `--features headless`)

When `web2md` is built with `--features headless`, the [`headless_chrome`] DevTools client is compiled in and `fetch --headless [--chrome-path PATH] [--wait-for-selector SEL]` routes the page through a real headless Chrome / Chromium. The launch helper:

1. Sets `LaunchOptions::path` from `--chrome-path` when given, otherwise relies on `headless_chrome::default_executable()` (system install or `CHROME` env var).
2. Disables the OS sandbox by default — most CI / container hosts lack the user-namespace setup Chrome's sandbox needs; users who want it can re-enable it via `CHROME_DEVEL_SANDBOX`.
3. Calls `Browser::new`, opens a new tab, navigates, waits for navigation to settle, optionally `wait_for_element(selector)` when `--wait-for-selector` is set (≈ Firecrawl `waitFor`), optionally scrolls to bottom `--scroll N` times (400ms between) for lazy-load, and returns the rendered HTML.

The work is run on a `tokio::task::spawn_blocking` thread because `headless_chrome` is a synchronous library. Without the feature compiled in, `render_url` returns a clear "rebuild with `--features headless`" error so the CLI fails fast with an actionable message.

## Local BM25 Corpus Index (`corpus` subcommand)

Two operations, both free of any external service:

- `web2md corpus index <DIR> [--output PATH]` — walks `DIR` recursively, reads every `.md` / `.markdown` file (hidden files / dirs skipped), tokenizes the body, and persists an inverted index to `<DIR>/.web2md-index.json` (or `--output`). Index format:

```json
{
  "version": 1,
  "avg_doc_length": 42.0,
  "docs": [{"path": "rust.md", "length": 12}],
  "index": {"rust": [{"doc": 0, "tf": 3}]}
}
```

- `web2md corpus query <DIR> <QUERY> [--limit N] [--json]` — loads the persisted index, ranks documents for the free-form query with the standard BM25 formula

```
score(D, Q) = Σ_tf_in_Q  IDF(tf) * (tf * (k1 + 1)) / (tf + k1 * (1 - b + b * |D| / avgdl))
```

with `k1 = 1.2`, `b = 0.75`, and returns the top-N hits with a short snippet around the first matching token. Default output is Markdown (`## 1. path / Score / > snippet`); `--json` emits the structured `CorpusHit` array.

Tokenization is the same as the existing in-pipeline extractor: lowercased alphanumeric tokens, stopwords dropped, single-character tokens dropped. There is no stemming — the goal is fast, deterministic, language-agnostic recall on natural-language queries.

## Webhook Delivery (`--webhook`)

After each `fetch` completes, if `--webhook <URL>` is set, the result is POSTed as JSON:

```json
{
  "event": "fetch.completed",
  "url": "https://example.com/article",
  "format": "markdown",
  "result": "..."
}
```

Non-2xx responses are logged to stderr; the local fetch result is still printed to stdout. Drops into n8n / Make / Zapier / Hugging Face Pipelines.

## Quality Bar

- All features have unit and integration tests (395 tests across lib, main, and integration suites at last count)
- `cargo build` and `cargo test` must pass before merge
- `cargo clippy` passes with 0 warnings
