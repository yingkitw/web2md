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
- [x] Fix raw `[text](url)` Markdown links from html2md multi-line output
- [x] Rate limiting / polite delay between requests (`--delay` flag)
- [x] Noise tag stripping: `<nav>`, `<footer>`, `<aside>`, `<noscript>`, `<form>`, HTML comments
- [x] Competitive intelligence: compared html2md vs htmd, html-to-markdown, Readability.js
- [x] Code language detection: `<code class="language-xxx">` → ` ```xxx ` fenced blocks
- [x] Deploy release binaries: optimized release profile + GitHub Actions CI/release workflows
- [x] Metadata extraction: meta description, Open Graph description, author in MCP response
- [x] `<header>` tag stripping with `--keep-header` opt-out flag
- [x] Caching layer: in-memory cache with TTL (`--cache-ttl` flag)
- [x] Content hash deduplication: removes duplicate paragraph-level blocks in Markdown output
- [x] Main content extraction: extracts `<article>`, `<main>`, or `[role="main"]` content (`--main-content` flag)

## In Progress

## Pending

## Brainstorming

- Optional JavaScript execution via headless Chrome bridge (behind feature flag)
- Switch to htmd crate for richer conversion options (heading styles, skip tags, faithful mode)
- Full readability scoring algorithm (text density, link density, paragraph scoring) for pages without semantic tags
