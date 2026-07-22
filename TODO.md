# TODO

## Done

- [x] Project scaffold (workspace, crate, deps)
- [x] Competitive intelligence v1: compared HTML-to-Markdown tools (Readability.js, etc.); in-house converter retained
- [x] Competitive intelligence v2: brainstormed vs Firecrawl + Context7 (see Brainstorming v2)
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
- [x] Dependency trim: remove `whatlang` (stopword heuristic instead); slim `reqwest` to `native-tls`/`http2`/`charset` only
- [x] CSV export (`--format csv`): Trafilatura-style header + row (url, title, author, date, language, page_type, quality, text)
- [x] Page-type extraction profiles: article/product prefer main-content; product keeps images and appends JSON-LD details; forum keeps full thread + comments
- [x] XML-TEI export (`--format tei`): Trafilatura-style `TEI` document with `teiHeader` metadata and `div type="entry"` body paragraphs
- [x] Content fingerprint: 64-bit simhash of extracted text in MCP / JSON / CSV / TEI / XML / frontmatter
- [x] Target-language filter: `--lang` ISO 639-1/639-3 rejects pages whose language does not match
- [x] Plain XML export (`--format xml`): Trafilatura-style `<doc>` with metadata and `<main>` paragraphs
- [x] Extraction presets: `--precision` / `--recall`, `--no-comments`, `--only-with-metadata` (CLI + MCP)
- [x] Competitive intelligence v2: brainstormed vs Firecrawl + Context7 (see Brainstorming v2 below)
- [x] `--topic` query-focused paragraph extraction (LLM-free; beats Firecrawl `highlights` 4 cr + Context7 `query-docs`)
- [x] `--summary` extractive summarization via TF-IDF + position scoring (LLM-free; beats Firecrawl `summary` 4 cr)
- [x] `--max-tokens` token-budget output (chars/4 approximation; cheaper than character `max_length`)
- [x] `peek` subcommand — return title + excerpt + key metadata only; skip body conversion
- [x] Recipe extractor (`--type recipe`) — JSON-LD Recipe → ingredient list + numbered steps + YAML frontmatter (prep/cook/servings)
- [x] FAQ extractor (`--type faq`) — JSON-LD FAQPage → Q+A as Markdown headings
- [x] Job posting extractor (`--type job`) — JSON-LD JobPosting → title/company/salary/date/apply
- [x] Event extractor (`--type event`) — JSON-LD Event → name/venue/start-end/ticket URL
- [x] Persistent file cache (`--cache-dir <path>`) — JSON files keyed by URL SHA-256; survives runs; better than in-memory TTL
- [x] Per-host token-bucket rate limiting (`--rate <req/s>`) — independent rate clock per host
- [x] `diff` subcommand — diff two URLs (or current vs cached file via `--cached-b`) at the Markdown line level
- [x] YouTube transcript extraction (watch page → captionTracks → timed text → MD with timestamps)
- [x] New shared modules: `transform.rs`, `structured.rs`, `persistent_cache.rs`, `diff_markdown.rs`, `youtube.rs`, `branding.rs`
- [x] New direct deps: `sha2` (cache key hashing), `regex` (already transitive; promoted to direct for YouTube captions)
- [x] `watch` subcommand (`src/main.rs::poll_once`): poll a URL on `--every` interval, emit a tab-separated line (timestamp, url, simhash, snippet) whenever the fingerprint changes; persists last-seen fingerprint under `--cache-dir` so restarts don't re-fire
- [x] `--webhook <url>` flag: POST `{event,url,format,result}` JSON to a webhook URL after each `fetch` completes (Firecrawl webhook parity for n8n/Make/Zapier integrations)
- [x] `branding` output format (`--format branding`, `src/branding.rs`): deterministic color/font/heading extraction from inline `<style>` blocks (≈ Firecrawl `branding` format, no LLM)
- [x] Competitive intelligence v3: brainstormed vs Firecrawl v2.11 (see Brainstorming v3 below)
- [x] `--format links` — all `<a href>` + text as JSON (Firecrawl `links` parity, free)
- [x] `--format images` — all `<img src>` + alt/title as JSON (Firecrawl `images` parity, free)
- [x] `--format product` — JSON-LD Product → structured JSON with variants/offers (Firecrawl `product` parity, deterministic, free)
- [x] `--include-selector` — keep only HTML elements matching CSS selectors before conversion (Firecrawl `includeTags` parity)
- [x] `--pii-redact` — regex redact emails, phone numbers, SSNs, credit cards from output (Firecrawl PII redaction, 4 cr → free)
- [x] `--mobile` flag — mobile User-Agent for responsive sites (Firecrawl `mobile: true` parity)
- [x] `map` subcommand — discover all URLs from HTML `<a href>` links (Firecrawl `/map` endpoint parity)
- [x] `search` subcommand — DuckDuckGo HTML web search, no API key, with `--fetch` to convert results to Markdown (Firecrawl `/search` parity, free)
- [x] `docs` subcommand — fetch README + metadata from crates.io, docs.rs, npm, or PyPI (poor-person's Context7, no API key, free)
- [x] `--proxy <url>` flag — route requests through HTTP/SOCKS proxy (Firecrawl proxy parity)
- [x] `--auth user:pass` flag — basic authentication for protected pages
- [x] New shared modules: `extract.rs` (links/images/product), `redact.rs` (PII redaction), `search.rs` (DDG web search), `docs.rs` (library doc fetcher)
- [x] `--proxy`/`--auth` extended to `peek` and `batch` commands (consistency — all HTTP-making commands now support proxy/auth)
- [x] Codebase audit: fixed regex-in-loop perf bug in `branding.rs` (OnceLock), collapsed dead if/else, resolved 96 clippy warnings → 0, added `--proxy`/`--auth` to `peek`/`batch`

## In Progress

_None — v3 cycle is complete. See Brainstorming for next-wave ideas._

## Brainstorming

_Competitive gaps vs Trafilatura, Firecrawl, Readability.js, and rs-trafilatura:_

- Use `readabilityrs` or `legible` crate for full Mozilla Readability.js compatibility (93.8% test pass rate)
- PDF output format for archival pipelines — plain text done via `--format text`; PDF remains future work
- Headless browser backend (Playwright/Chromium) for full SPA rendering beyond inline-script subset
- `--no-tables` / `--include-links` element toggles (Trafilatura parity)
- Word/character count fields on `PageMetadata`

### Brainstorming v2 — beating Firecrawl and Context7

**Our strategic advantages** (local-first, no API key, no SaaS, in-house extraction, free forever):

1. **No LLM dependency** — every feature below is deterministic from JSON-LD, microdata, or local statistics. Cheaper, faster, deterministic, private.
2. **No API key / no rate limit** — usable offline, in CI, in air-gapped environments. (`--proxy` available for corporate networks.)
3. **Context7-equivalent token shaping for any page** — Firecrawl just hands back the page; we can shape output by query, budget, or topic.
4. **Domain-specific extractors** that Firecrawl doesn't ship as turnkey formats (Recipe, FAQPage, JobPosting, Event — all JSON-LD).

**What Firecrawl has that we don't**: browser automation (`actions`/`interact`), screenshots, audio/video extraction, PDF/DOCX parsing.

**What Context7 has that we don't**: pre-curated library index, automatic version detection, library-aware retrieval. (We now have `docs` subcommand for live README fetching from any registry.)

**Prioritized backlog** (see Pending for items being implemented this cycle):

| # | Feature | Beats | Status |
|---|---|---|---|
| 1 | `--topic` query-focused paragraph extraction | Firecrawl `highlights`, Context7 `query-docs` | ✅ Done |
| 2 | `--summary` extractive summarization (LLM-free TF scoring) | Firecrawl `summary` (LLM, 4 cr) | ✅ Done |
| 3 | `--max-tokens` token-budget output | Firecrawl `maxAge`/`max_length`, Context7 token shaping | ✅ Done |
| 4 | `peek` subcommand — metadata + excerpt only | Firecrawl none — saves full fetch cost | ✅ Done |
| 5 | Recipe extractor (`--type recipe`, JSON-LD Recipe) | Firecrawl `json` schema (4 cr) | ✅ Done |
| 6 | FAQ extractor (`--type faq`, JSON-LD FAQPage) | Firecrawl `json` schema (4 cr) | ✅ Done |
| 7 | Job posting extractor (`--type job`, JSON-LD JobPosting) | Firecrawl none specifically | ✅ Done |
| 8 | Event extractor (`--type event`, JSON-LD Event) | Firecrawl none specifically | ✅ Done |
| 9 | Persistent file cache (`--cache-dir`) survives runs | Firecrawl cloud cache (paid) | ✅ Done |
| 10 | Per-host token-bucket rate limiting (`--rate`) | Firecrawl flat 60s/page + paid enhanced proxy | ✅ Done |
| 11 | `diff` subcommand — URL vs URL or vs cached | Firecrawl `changeTracking` (paywalled) | ✅ Done |
| 12 | YouTube transcript extraction (text-only) | Firecrawl `video`/`audio` extraction (5 cr) | ✅ Done |
| 13 | `watch` subcommand — poll URL, emit on simhash change | Firecrawl `changeTracking` (polling) | ✅ Done |
| 14 | `--webhook <url>` delivery (n8n/Make/Zapier) | Firecrawl webhooks (paid tier) | ✅ Done |
| 15 | `branding` output format (top-N CSS colors + fonts) | Firecrawl `branding` (paid format) | ✅ Done |

### Brainstorming v3 — surpassing Firecrawl v2.11

**Firecrawl's current edge** (from docs.firecrawl.dev, v2.11):
- `links` format — all links as JSON
- `images` format — all images as JSON
- `product` format — deterministic structured product (title, price, variants)
- `includeTags` / `excludeTags` — CSS selector content filtering
- `mobile: true` — mobile device emulation
- PII redaction — 4 credits/page, regex-based
- `/map` endpoint — discover all URLs on a site
- `/search` — web search (paid proxy)
- `/interact` — browser automation (paid)
- `screenshot` — page screenshots (paid)
- PDF/DOCX parsing (paid, 1 cr/page)

**Our strategy**: implement every deterministic Firecrawl feature locally, free, offline. Skip only features that fundamentally require a SaaS proxy or headless browser.

**Prioritized backlog v3**:

| # | Feature | Beats | Status |
|---|---|---|---|
| 16 | `--format links` — all `<a href>` + text as JSON | Firecrawl `links` format | ✅ Done |
| 17 | `--format images` — all `<img>` + alt as JSON | Firecrawl `images` format | ✅ Done |
| 18 | `--format product` — JSON-LD Product → structured JSON | Firecrawl `product` (deterministic, free) | ✅ Done |
| 19 | `--include-selector` — keep only matching elements | Firecrawl `includeTags` | ✅ Done |
| 20 | `--pii-redact` — regex redact emails/phones/SSN/cards | Firecrawl PII redaction (4 cr → free) | ✅ Done |
| 21 | `--mobile` flag — mobile User-Agent | Firecrawl `mobile: true` | ✅ Done |
| 22 | `map` subcommand — discover all URLs from HTML | Firecrawl `/map` endpoint | ✅ Done |
| 23 | `search` subcommand — DDG HTML web search, no API key | Firecrawl `/search` (paid proxy) | ✅ Done |
| 24 | `docs` subcommand — fetch README from any registry | Context7 (curated index) | ✅ Done |
| 25 | `--proxy` flag — HTTP/SOCKS proxy support | Firecrawl proxy support | ✅ Done |
| 26 | `--auth` flag — basic authentication | Firecrawl auth header | ✅ Done |

**Later** (lower leverage, requires external services or large effort):
- Web search — ✅ Done (DuckDuckGo HTML, `search` subcommand)
- Headless browser backend (`headless_chrome` opt-in) for true SPA support and screenshot
- PDF/DOCX parsing from URLs (`lopdf`, `docx-rs`) — keep out unless demand warrants the binary-size cost
- Local-web search backend: index CLI docs and serve them via Context7-compatible endpoints
- Library doc fetcher — ✅ Done (`docs` subcommand, crates.io/docs.rs/npm/PyPI)
- Use `readabilityrs` or `legible` crate for full Mozilla Readability.js compatibility (93.8% test pass rate)
- `--no-tables` / `--include-links` explicit element toggles for Trafilatura parity — ✅ Already implemented
