# ARCHITECTURE

## Module Relationships

```
main.rs
  ├── CLI parsing (clap)
  ├── <URL> (default) → browse_loop → Browser → PageToMarkdown → ANSI renderer → terminal
  ├── fetch command   → Browser → inline_iframes → PageToMarkdown → stdout
  └── mcp command     → McpServer → Browser → inline_iframes → PageToMarkdown → JSON-RPC

lib.rs
  ├── browser.rs   : HTTP client, fetch raw HTML, inline iframe content
  ├── markdown.rs  : HTML → Markdown conversion (strip scripts, styles, iframes, images)
  └── mcp.rs       : JSON-RPC server wrapper

main.rs (helpers)
  ├── render_markdown_ansi() : pulldown-cmark → ANSI escape codes (headings, links, tables, code)
  ├── fix_raw_links()        : Post-process multi-line `[text](url)` patterns
  ├── extract_links()          : Parse Markdown links for browse navigation
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
                          PageToMarkdown.convert()
                                  │
                                  ├── strips <script>, <style>, <iframe>
                                  ├── strips <img> (unless include_images)
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

1. **No rendering engine**: We do not execute JavaScript or render CSS. This keeps the crate lightweight and avoids DOM complexity. The tradeoff is that JS-heavy SPAs may return incomplete Markdown.

2. **html2md crate**: Delegates HTML parsing to a mature, lightweight library rather than building a custom parser.

3. **Iframe inlining**: Instead of discarding `<iframe>` tags (which removes embedded content like widgets and videos), we fetch the `src` URL and inject the content into the parent HTML. This provides a more complete page representation at the cost of additional HTTP requests.

4. **ANSI terminal rendering**: `render_markdown_ansi()` uses `pulldown-cmark` with `ENABLE_TABLES` to parse Markdown into an event stream, then converts syntax markers into ANSI escape codes. A second pass (`fix_raw_links`) catches multi-line `[text](url)` patterns that the parser misses.

5. **MCP over stdio**: The MCP server reads JSON-RPC requests from stdin and writes responses to stdout. This aligns with the MCP spec and makes it trivial to wire into any MCP host.

## Deployment Topology

- Local interactive: `cargo run -- <URL>` (browse mode)
- Local one-shot: `cargo run -- fetch <URL> [--render]`
- MCP Host: `cargo run -- mcp` (stdio transport)
- Future: release binary for standalone use
