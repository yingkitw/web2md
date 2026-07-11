# TODO

## Done

- [x] Project scaffold (workspace, crate, deps)
- [x] Browser module: HTTP fetch with mock tests
- [x] Markdown module: HTML-to-Markdown conversion
- [x] MCP server module: JSON-RPC request/response types
- [x] CLI: `fetch`, `browse`, and `mcp` subcommands
- [x] README, SPEC, ARCHITECTURE docs
- [x] Integration test: end-to-end fetch → markdown flow
- [x] Fix build: removed unused `reqwest/cookies` feature (broke with `time` 0.3.52)
- [x] Fix `PageToMarkdown`: actually strip `<script>` and `<style>` blocks
- [x] Add `--timeout` CLI flag
- [x] Respect `include_images` flag (emit Markdown image references)
- [x] Cookie jar support for authenticated pages (`--cookie`)
- [x] Custom headers via CLI (`--header`)
- [x] `--format` CLI option: `markdown` (default) or `html` raw
- [x] `--render` flag: ANSI terminal rendering of Markdown (colors, bold, underlined links)
- [x] `browse` subcommand: interactive Lynx-like terminal browser with back/forward history and link navigation
- [x] Default command: passing a URL directly launches `browse` mode
- [x] Strip `<iframe>` tags from HTML before Markdown conversion
- [x] Inline iframe content: fetch `src` and consolidate into parent page
- [x] Table rendering: box-drawing characters for terminal display
- [x] Numbered links in browse mode: `[N]` prefix before each link
- [x] Fix empty-text links not getting numbered
- [x] Fix raw `[text](url)` Markdown links from multi-line converter output
- [x] Rate limiting / polite delay between requests (`--delay` flag)
- [x] Noise tag stripping: `<nav>`, `<footer>`, `<aside>`, `<noscript>`, `<form>`, HTML comments
- [x] Competitive intelligence: compared HTML-to-Markdown tools (Readability.js, etc.); in-house converter retained
- [x] Replace `html2md` dependency with in-house `html_to_md` module
- [x] Code language detection: `<code class="language-xxx">` → ` ```xxx ` fenced blocks
- [x] Deploy release binaries: optimized release profile + GitHub Actions CI/release workflows
- [x] Metadata extraction: meta description, Open Graph description, author in MCP response
- [x] `<header>` tag stripping with `--keep-header` opt-out flag
- [x] Caching layer: in-memory cache with TTL (`--cache-ttl` flag)
- [x] Content hash deduplication: removes duplicate paragraph-level blocks in Markdown output
- [x] Main content extraction: extracts `<article>`, `<main>`, or `[role="main"]` content (`--main-content` flag)
- [x] Readability scoring: text-density and link-density scoring fallback for pages without semantic tags
- [x] Publication date extraction: `<meta property="article:published_time">`, `<time datetime>`, JSON-LD `datePublished`
- [x] `--format json` CLI option: emit structured JSON (markdown + metadata) from CLI, not just MCP
- [x] JSON-LD author extraction: `author` field from JSON-LD blocks (string or `{"name":"..."}` object), as fallback when `<meta name="author">` is absent
- [x] JSON-LD structured data extraction: `image` (og:image or JSON-LD string/object/array) and `headline` (JSON-LD) fields in MCP response and `--format json` output
- [x] Paragraph-level readability scoring: sliding window over `<p>` blocks as fallback when div/section scoring fails
- [x] Comments extraction for forum pages: detects forum/thread pages, extracts author + text + nesting depth, formats as Markdown with blockquotes and indentation
- [x] Site name extraction: `og:site_name` meta tag in MCP response and `--format json` output
- [x] Keywords/tags extraction: `article:tag` meta tags (multiple), `meta name="keywords"` fallback, JSON-LD `keywords` (string or array) fallback
- [x] Link URL absolutization: convert relative URLs to absolute in Markdown output using the page URL as base
- [x] Sitemap/feed discovery: `sitemap` subcommand fetches sitemap.xml and discovers RSS/Atom feed links from HTML pages
- [x] Batch processing: `batch` subcommand reads URLs from a file (one per line, # comments supported) and converts each to Markdown
- [x] Output to file: `--output` flag on `fetch` writes result to a file instead of stdout; `--output` on `batch` writes to a directory
- [x] YAML frontmatter output: `--frontmatter` flag prepends metadata (title, description, author, date, etc.) as a YAML block at the top of Markdown output
- [x] CSS selector targeting: `--exclude-selector` flag strips HTML elements matching `.class` or `#id` selectors before conversion
- [x] Built-in JavaScript interpreter (`src/js/`): dependency-free lexer/parser/evaluator for a JS subset, executes inline `<script>` blocks when `--javascript` is set and folds `document.write` output into the page (replaces any need for boa/v8)
- [x] URL blacklist filtering: skip known non-content URLs (ads, tracking pixels, analytics hosts) on iframe inlining, batch processing, and sitemap output; `--no-blacklist` to disable
- [x] Recursive crawl: `--depth N` on `fetch` discovers and converts same-origin linked pages (BFS); `--output` writes to a directory
- [x] robots.txt respect: parse and honor Disallow rules and Crawl-delay before fetching; `--ignore-robots` to disable
- [x] Custom user blacklist file: load additional URL patterns from `~/.web2md/blacklist.txt` and `--blacklist-file`; `--no-user-blacklist` to skip the default file
- [x] Shared `html_util` module: extracted `find_ci` and HTML entity decoding for the in-house converter
- [x] Markdown control-character escaping in `html_to_md` (list/heading markers in raw text)
- [x] Robust HTML parsing with `scraper` crate (html5ever-based) for malformed/unclosed tags in `html_to_md`
- [x] Plain-text output format (`--format text`) for archival and NLP pipelines
- [x] Trafilatura-style fallback chain: multi-candidate scoring (semantic tags with bonus, block readability, paragraph clustering), best-candidate selection, jusText-style boilerplate paragraph stripping
- [x] Post-load wait (`--wait` MS): delay after fetch before processing; `setTimeout` callbacks fire when delay ≤ wait budget (with `--javascript`)
- [x] JS timer scheduling: `setInterval` (repeating within `--wait`) and `requestAnimationFrame` (~16ms) in the built-in interpreter
- [x] `clearTimeout` / `clearInterval` for cancelling scheduled JS callbacks
- [x] Structured content fallback: JSON-LD `articleBody` / `description`, `og:description`, and meta description when main-content heuristics fail
- [x] Shared `html_meta` module: deduplicated JSON-LD and `<meta>` parsing used by `mcp.rs` and structured content fallback in `markdown.rs`
- [x] Extended metadata: excerpt (first substantive paragraph), canonical URL (`og:url` / `<link rel="canonical">`), and language (`html lang`, `og:locale`, JSON-LD `inLanguage`)
- [x] Article categories/sections: `article:section` meta tags and JSON-LD `articleSection` (string or array) in MCP response, `--format json`, and YAML frontmatter
- [x] RSS/Atom feed parsing: `feed` subcommand fetches RSS 2.0 or Atom feeds and converts entries to Markdown (or `--json`); supports `--max-entries` and `--output`
- [x] Codebase audit: unified `PageMetadata` via serde flatten (MCP + CLI JSON), shared meta-property collector / tag stripper / truncate helper, shared `build_browser_options` CLI wiring, removed dead `follow_redirects`
- [x] Dublin Core metadata fallbacks: `DC.title` / `dcterms.title`, `DC.creator` / `dcterms.creator`, `DC.date` / `dcterms.date`, `DC.description` / `dcterms.description`
- [x] JSON Feed parsing: `parse_feed` accepts JSON Feed 1/1.1; `feed` subcommand and `sitemap --feeds` discover `application/feed+json` links
- [x] Extraction quality score (0.0–1.0) and page-type classification (`article` / `forum` / `product` / `page`) in MCP, `--format json`, and YAML frontmatter

## In Progress

## Pending

_None — all planned features are implemented. See Brainstorming for future ideas._

## Brainstorming

_Competitive gaps vs Trafilatura, Firecrawl, Readability.js, and rs-trafilatura:_

- Use `readabilityrs` or `legible` crate for full Mozilla Readability.js compatibility (93.8% test pass rate)
- PDF output format for archival pipelines — plain text done via `--format text`; PDF remains future work
- Headless browser backend (Playwright/Chromium) for full SPA rendering beyond inline-script subset
- Language detection on extracted text content (Trafilatura optional add-on)
- CSV/XML-TEI export formats for corpus pipelines
- Page-type-specific extraction profiles (specialize convert path per `page_type`)
