# ARCHITECTURE

## Module Relationships

```
main.rs
  ├── CLI parsing (clap)
  ├── <URL> (default) → browse_loop → Browser → PageToMarkdown → ANSI renderer → terminal
  ├── fetch command   → Browser → inline_iframes → run_inline_scripts → PageToMarkdown → stdout
  │                     ├── --depth N → BFS crawl via crawl.rs (same-origin links) → multiple Markdown outputs
  │                     └── --format json → extract_metadata → structured JSON output
  ├── sitemap command → Browser → parse_sitemap_urls / extract_feed_links → URL list
  ├── batch command   → Browser → run_inline_scripts → PageToMarkdown → stdout or output directory
  └── mcp command     → McpServer → Browser → inline_iframes → run_inline_scripts → PageToMarkdown → JSON-RPC

lib.rs
  ├── browser.rs   : HTTP client, fetch raw HTML, inline iframe content, in-memory cache with TTL, sitemap XML parsing, RSS/Atom feed link extraction, run_inline_scripts() (gated by enable_javascript), URL blacklist filtering on secondary fetches
  ├── url_blacklist.rs : Host/path pattern matching for ads, analytics, and tracking pixels; filter_blacklisted_urls() helper
  ├── crawl.rs       : HTML link extraction, same-origin filtering, URL normalization for recursive crawl (`--depth N`)
  ├── js/          : Built-in dependency-free JavaScript subset interpreter
  │     ├── ast.rs     : AST node types (expressions, statements, operators)
  │     ├── lexer.rs   : Tokenizer (numbers, strings, templates, keywords, punctuators)
  │     ├── parser.rs  : Recursive-descent parser → Vec<Stmt>
  │     ├── eval.rs    : Tree-walking evaluator with lexical scopes, closures, control flow, and builtins (document.write, strings, arrays, Math, JSON, console, global constructors)
  │     └── mod.rs     : run_inline_scripts(html) — extracts inline <script> blocks, runs them, returns document.write output; inject_before_body_close()
  ├── markdown.rs  : HTML → Markdown conversion (strip scripts, styles, iframes, noise tags, comments; extract code languages; main content extraction with readability fallback + paragraph-level sliding window; dedup; images; forum comment extraction with author attribution and nesting; link URL absolutization; CSS selector exclusion via --exclude-selector)
  └── mcp.rs       : JSON-RPC server wrapper, metadata extraction (title, description, author, published_date, image, headline, site_name, keywords), PageMetadata struct with to_frontmatter() for YAML output, extract_metadata() public function

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
                                  ├── extracts main content (if --main-content: <article>, <main>, [role=main], or readability fallback scoring top-level <div>/<section> by text density vs link density)
                                  ├── strips <script>, <style>, <iframe>
                                  ├── strips <nav>, <footer>, <aside>, <noscript>, <form>, <header> (unless keep_header)
                                  ├── strips HTML comments
                                  ├── extracts code languages from <code class="language-xxx">
                                  ├── strips <img> (unless include_images)
                                  ├── injects languages into fenced code blocks
                                  ├── deduplicates repeated paragraph blocks
                                  └── collapses excessive whitespace
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

1. **Optional JS execution, in-house**: By default the crate does not execute JavaScript (keeping it lightweight and deterministic). With `--javascript` / `enable_javascript`, inline `<script>` blocks are evaluated by the project's own dependency-free interpreter (`src/js/`) — no `boa`, `v8`, or external engine. The interpreter supports a pragmatic subset (variables, closures, control flow, template literals, `document.write`, strings, arrays, `Math`, `JSON`); scripts using unsupported features fail fast and are silently skipped, so they never break conversion. External and module scripts are not executed.

2. **html2md crate**: Delegates HTML parsing to a mature, lightweight library rather than building a custom parser.

3. **Iframe inlining**: Instead of discarding `<iframe>` tags (which removes embedded content like widgets and videos), we fetch the `src` URL and inject the content into the parent HTML. This provides a more complete page representation at the cost of additional HTTP requests.

4. **ANSI terminal rendering**: `render_markdown_ansi()` uses `pulldown-cmark` with `ENABLE_TABLES` to parse Markdown into an event stream, then converts syntax markers into ANSI escape codes. A second pass (`fix_raw_links`) catches multi-line `[text](url)` patterns that the parser misses.

5. **MCP over stdio**: The MCP server reads JSON-RPC requests from stdin and writes responses to stdout. This aligns with the MCP spec and makes it trivial to wire into any MCP host.

## Deployment Topology

- Local interactive: `cargo run -- <URL>` (browse mode)
- Local one-shot: `cargo run -- fetch <URL> [--render] [--format json]`
- MCP Host: `cargo run -- mcp` (stdio transport)
- Release binary: `cargo build --release` (LTO + stripped, ~4MB)
- CI/Release: GitHub Actions builds for Linux (x86_64/aarch64), macOS (x86_64/aarch64), Windows (x86_64) on tag push
