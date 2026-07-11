# ARCHITECTURE

## Module Relationships

```
main.rs
  ├── CLI parsing (clap)
  ├── <URL> (default) → browse_loop → Browser → PageToMarkdown → ANSI renderer → terminal
  ├── fetch command   → Browser → inline_iframes → run_inline_scripts → PageToMarkdown → stdout
  │                     ├── --depth N → BFS crawl via crawl.rs (same-origin links) → multiple Markdown outputs
  │                     ├── --format json → extract_page_metadata → structured JSON output
  │                     ├── --format csv → extract_page_metadata → Trafilatura-style CSV row
  │                     └── --format tei → extract_page_metadata → TEI XML document
  ├── sitemap command → Browser → parse_sitemap_urls / extract_feed_links → URL list
  ├── feed command    → Browser → parse_feed → feed_to_markdown (or JSON) → stdout / file
  ├── batch command   → Browser → run_inline_scripts → PageToMarkdown → stdout or output directory
  └── mcp command     → McpServer → Browser → inline_iframes → run_inline_scripts → PageToMarkdown → JSON-RPC

lib.rs
  ├── browser.rs   : HTTP client, fetch raw HTML, inline iframe content, in-memory cache with TTL, sitemap XML parsing, RSS/Atom feed link extraction, run_inline_scripts() (gated by enable_javascript), URL blacklist filtering on secondary fetches
  ├── feed.rs      : RSS 2.0 / Atom / JSON Feed parser (`parse_feed`) and Markdown converter (`feed_to_markdown`)
  ├── url_blacklist.rs : Host/path pattern matching for ads, analytics, and tracking pixels; BlacklistPatterns with built-in + `~/.web2md/blacklist.txt` + `--blacklist-file` merge
  ├── crawl.rs       : HTML link extraction, same-origin filtering, URL normalization for recursive crawl (`--depth N`)
  ├── robots.rs      : robots.txt parser (Disallow, Crawl-delay), per-origin cache in Browser
  ├── js/          : Built-in dependency-free JavaScript subset interpreter
  │     ├── ast.rs     : AST node types (expressions, statements, operators)
  │     ├── lexer.rs   : Tokenizer (numbers, strings, templates, keywords, punctuators)
  │     ├── parser.rs  : Recursive-descent parser → Vec<Stmt>
  │     ├── eval.rs    : Tree-walking evaluator with lexical scopes, closures, control flow, and builtins (document.write, strings, arrays, Math, JSON, console, global constructors)
  │     └── mod.rs     : run_inline_scripts(html, wait_ms) — extracts inline <script> blocks, runs them, flushes timer callbacks, returns document.write output; inject_before_body_close()
  ├── html_util.rs  : Shared HTML helpers (`find_ci`, entity decoding, `strip_html_tags`)
  ├── html_meta.rs  : Shared `<meta>`, JSON-LD, `<link rel>`, and `<html lang>` parsing (`collect_meta_property_values`, `extract_json_ld_string_list`)
  ├── html_to_md.rs : In-house HTML → Markdown converter via `scraper`/html5ever DOM walk (headings, links, images, lists, code blocks, tables, inline formatting)
  ├── markdown.rs  : PageToMarkdown — HTML→Markdown pipeline; page-type profiles; `extraction_quality()` / `detect_page_type()`; main-content heuristics; forum comments; product JSON-LD details; dedup; link absolutization
  └── mcp.rs       : JSON-RPC server; `PageMetadata` (serde flatten); `extract_metadata` / `extract_page_metadata` (adds extraction_quality, page_type, whatlang language fallback); `to_csv` / `to_tei`; `truncate_with_marker`

main.rs (helpers)
  ├── render_markdown_ansi() : pulldown-cmark → ANSI escape codes (headings, links, tables, code)
  ├── fix_raw_links()        : Post-process multi-line `[text](url)` patterns
  ├── extract_links()          : Parse Markdown links for browse navigation
  ├── url_to_filename()        : Convert URL to safe filename for batch output
  └── browse_loop()           : Interactive terminal browser with history
```

## Data Flow

```
URL ──► Browser.fetch() ──► raw HTML
                                  │
                                  ▼
                          Browser.inline_iframes()
                                  │
                                  ▼
                          Browser.run_inline_scripts()   (only when --javascript / enable_javascript)
                                  │
                                  ├── extracts inline <script> blocks (no src; classic JS type)
                                  ├── parses + evaluates each via the built-in interpreter
                                  ├── captures document.write / writeln output
                                  └── injects captured HTML before </body>
                                  │
                                  ▼
                          PageToMarkdown.convert()
                                  │
                                  ├── extracts main content (if --main-content: Trafilatura-style fallback — semantic tags + block readability + paragraph clustering, best-candidate pick, boilerplate strip, JSON-LD/OG structured fallback)
                                  ├── strips <script>, <style>, <iframe>
                                  ├── strips <nav>, <footer>, <aside>, <noscript>, <form>, <header> (unless keep_header)
                                  ├── strips HTML comments
                                  ├── strips elements matching --exclude-selector (.class / #id)
                                  ├── extracts code languages from <code class="language-xxx">
                                  ├── strips <img> (unless include_images)
                                  ├── html_to_md::parse_html() — scraper/html5ever DOM walk
                                  │     ├── entity decoding, Markdown escaping, malformed-HTML tolerance
                                  │     └── headings, links, images, lists, tables, code blocks
                                  ├── injects languages into fenced code blocks
                                  ├── deduplicates repeated paragraph blocks
                                  ├── extracts forum comments (author, nesting, blockquotes)
                                  └── collapses excessive whitespace
                                  │
                                  ▼
                          PageToMarkdown.absolutize_links() (relative → absolute URLs)
                                  │
                                  ▼
                          Markdown
                                  │
                    ┌─────────────┴─────────────┐
                    ▼                           ▼
            render_markdown_ansi()      raw output (--format markdown)
                    │
                    ├── headings: bold + color
                    ├── links: underlined cyan + [N] numbers (browse mode)
                    ├── tables: box-drawing characters
                    ├── code: light gray on dark bg
                    └── blockquotes: gray bar + italic
```

## Key Decisions

1. **Optional JS execution, in-house**: By default the crate does not execute JavaScript (keeping it lightweight and deterministic). With `--javascript` / `enable_javascript`, inline `<script>` blocks are evaluated by the project's own dependency-free interpreter (`src/js/`) — no `boa`, `v8`, or external engine. The interpreter supports a pragmatic subset (variables, closures, control flow, template literals, `document.write`, timer APIs including `clearTimeout`/`clearInterval`, strings, arrays, `Math`, `JSON`); timer callbacks flush when scheduled time ≤ `--wait`. Scripts using unsupported features fail fast and are silently skipped, so they never break conversion. External and module scripts are not executed.

2. **In-house HTML-to-Markdown converter**: HTML-to-Markdown conversion uses `html_to_md.rs` — a DOM walker built on `scraper` (html5ever) for malformed-HTML tolerance. No dedicated HTML-to-Markdown crate (`html2md`, `htmd`, etc.). The converter handles headings, links, images, lists, tables, code blocks, entity decoding, and Markdown control-character escaping. Pre/post-processing in `PageToMarkdown` (noise stripping, main-content extraction, dedup, code language injection, forum comments, link absolutization) wraps `html_to_md::parse_html`.

3. **Iframe inlining**: Instead of discarding `<iframe>` tags (which removes embedded content like widgets and videos), we fetch the `src` URL and inject the content into the parent HTML. This provides a more complete page representation at the cost of additional HTTP requests.

4. **ANSI terminal rendering**: `render_markdown_ansi()` uses `pulldown-cmark` with `ENABLE_TABLES` to parse Markdown into an event stream, then converts syntax markers into ANSI escape codes. A second pass (`fix_raw_links`) catches multi-line `[text](url)` patterns that the parser misses.

5. **MCP over stdio**: The MCP server reads JSON-RPC requests from stdin and writes responses to stdout. This aligns with the MCP spec and makes it trivial to wire into any MCP host.

## Dependencies

| Crate | Role |
|---|---|
| `reqwest` | HTTP client |
| `tokio` | Async runtime |
| `scraper` | HTML parsing (html5ever) for `html_to_md` |
| `pulldown-cmark` | Markdown → ANSI terminal rendering |
| `clap` | CLI argument parsing |
| `serde` / `serde_json` | JSON serialization (MCP, `--format json`) |
| `whatlang` | Language detection fallback (ISO 639-3) when HTML metadata lacks language |
| `url` | URL parsing, resolution, absolutization |
| `anyhow` | Error handling |
| `mockito` | HTTP mocking in tests (dev) |

## Deployment Topology

- Local interactive: `cargo run -- <URL>` (browse mode)
- Local one-shot: `cargo run -- fetch <URL> [--render] [--format json]`
- MCP Host: `cargo run -- mcp` (stdio transport)
- Release binary: `cargo build --release` (LTO + stripped, ~4MB)
- CI/Release: GitHub Actions builds for Linux (x86_64/aarch64), macOS (x86_64/aarch64), Windows (x86_64) on tag push
