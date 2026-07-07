//! Built-in JavaScript subset interpreter.
//!
//! A small, dependency-free JS engine used to capture content produced by
//! simple inline `<script>` blocks (notably `document.write(...)`) when the
//! browser option `enable_javascript` is set. The supported language subset is
//! defined by [`ast`], [`lexer`], and [`parser`]; evaluation lives in
//! [`eval`]. Scripts that use features outside the subset fail fast and are
//! silently skipped, so unsupported scripts never break page conversion.

pub mod ast;
pub mod eval;
pub mod lexer;
pub mod parser;

pub use eval::Interpreter;

/// Run every inline `<script>` block in `html` through a fresh interpreter and
/// return the concatenated HTML captured via `document.write` / `writeln`.
///
/// External scripts (`<script src=...>`), module scripts, and non-JS script
/// types are ignored. Parse/evaluation errors in any single block are skipped
/// so that one unsupported script cannot abort the rest. The returned string
/// is intended to be injected back into the page so downstream conversion can
/// see the JS-generated content.
pub fn run_inline_scripts(html: &str, wait_ms: u64) -> String {
    let mut interp = Interpreter::new();
    for src in extract_inline_scripts(html) {
        let _ = interp.run_script(&src);
    }
    interp.flush_timers(wait_ms);
    interp.document_html()
}

/// Extract the source text of executable inline `<script>` blocks.
///
/// A block is included only when:
/// - it has no `src` attribute (external scripts are not fetched here),
/// - its `type` attribute is absent, empty, or `text/javascript` /
///   `application/javascript` (module scripts and custom types are skipped).
///
/// `<script>` tags are matched case-insensitively and the search is tolerant
/// of missing closing tags.
fn extract_inline_scripts(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = html.as_bytes();
    let mut i = 0usize;
    while let Some(rel) = find_ci(html, i, "<script") {
        let start = rel;
        let tag_end = match find_sub(bytes, start, b">") {
            Some(p) => p,
            None => break,
        };
        let tag = &html[start..tag_end];
        let body_start = tag_end + 1;
        if has_attr_ci(tag, "src") || !is_js_type(tag) {
            // Skip external / non-JS script; advance past this tag.
            i = body_start;
            continue;
        }
        let body_end = find_ci(html, body_start, "</script>").unwrap_or(html.len());
        out.push(html[body_start..body_end].to_string());
        i = body_end + "</script>".len().min(html.len());
    }
    out
}

/// Inject `html_fragment` into `html` just before `</body>` (case-insensitive);
/// fall back to `</html>` then to appending at the end.
pub fn inject_before_body_close(html: &str, html_fragment: &str) -> String {
    if let Some(pos) = find_ci(html, 0, "</body>") {
        let mut out = String::with_capacity(html.len() + html_fragment.len());
        out.push_str(&html[..pos]);
        out.push_str(html_fragment);
        out.push_str(&html[pos..]);
        return out;
    }
    if let Some(pos) = find_ci(html, 0, "</html>") {
        let mut out = String::with_capacity(html.len() + html_fragment.len());
        out.push_str(&html[..pos]);
        out.push_str(html_fragment);
        out.push_str(&html[pos..]);
        return out;
    }
    let mut out = String::with_capacity(html.len() + html_fragment.len());
    out.push_str(html);
    out.push_str(html_fragment);
    out
}

fn find_ci(haystack: &str, from: usize, needle: &str) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }
    let hay = &haystack[from..];
    let nlen = needle.len();
    if nlen == 0 || nlen > hay.len() {
        return None;
    }
    let nlower: String = needle.chars().map(|c| c.to_ascii_lowercase()).collect();
    for i in 0..=(hay.len() - nlen) {
        if hay[i..i + nlen]
            .chars()
            .map(|c| c.to_ascii_lowercase())
            .eq(nlower.chars())
        {
            return Some(from + i);
        }
    }
    None
}

fn find_sub(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from > haystack.len() {
        return None;
    }
    let nlen = needle.len();
    for i in from..=(haystack.len() - nlen) {
        if &haystack[i..i + nlen] == needle {
            return Some(i);
        }
    }
    None
}

/// Case-insensitive check for the presence of a `name=` attribute.
fn has_attr_ci(tag: &str, name: &str) -> bool {
    attr_value_ci(tag, name).is_some()
}

/// Return the trimmed value of an attribute (case-insensitive name match),
/// or `None` if the attribute is absent. Handles quoted and bare values.
fn attr_value_ci(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip to the start of an attribute name (a letter).
        if !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
            i += 1;
            continue;
        }
        let name_start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric()
                || bytes[i] == b'-'
                || bytes[i] == b'_'
                || bytes[i] == b':')
        {
            i += 1;
        }
        let attr_name = &tag[name_start..i];
        if attr_name.eq_ignore_ascii_case(name) {
            // Skip whitespace.
            while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'=' {
                i += 1;
                while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() {
                    i += 1;
                }
                if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                    let quote = bytes[i];
                    i += 1;
                    let val_start = i;
                    while i < bytes.len() && bytes[i] != quote {
                        i += 1;
                    }
                    return Some(tag[val_start..i].trim().to_string());
                }
                let val_start = i;
                while i < bytes.len() && !(bytes[i] as char).is_ascii_whitespace() {
                    i += 1;
                }
                return Some(tag[val_start..i].trim().to_string());
            }
            return Some(String::new()); // boolean attribute present
        }
    }
    None
}

/// True when a `<script>` tag's `type` attribute permits execution by this
/// interpreter. Absent / empty / classic JS types are allowed; modules and
/// JSON/importmap/etc. are not.
fn is_js_type(tag: &str) -> bool {
    match attr_value_ci(tag, "type") {
        None => true,
        Some(t) => {
            let t = t.trim();
            t.is_empty()
                || t.eq_ignore_ascii_case("text/javascript")
                || t.eq_ignore_ascii_case("application/javascript")
                || t.eq_ignore_ascii_case("application/ecmascript")
                || t.eq_ignore_ascii_case("text/ecmascript")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_inline_only() {
        let html = r#"
        <script>document.write("a");</script>
        <script type="application/javascript">document.write("b");</script>
        <script src="ext.js"></script>
        <script type="module">import x;</script>
        <script type="application/ld+json">{"@type":"NewsArticle"}</script>
        <p>x</p>
        <script>document.write("c");</script>
        "#;
        let scripts = extract_inline_scripts(html);
        assert_eq!(scripts.len(), 3);
        assert!(scripts[0].contains(r#""a""#));
        assert!(scripts[1].contains(r#""b""#));
        assert!(scripts[2].contains(r#""c""#));
    }

    #[test]
    fn run_inline_captures_writes() {
        let html = r#"
        <main>Static</main>
        <script>document.write("<p>Dynamic</p>");</script>
        <script type="application/ld+json">{"x":1}</script>
        <script>for (var i=0;i<2;i++){document.write(i);}</script>
        "#;
        let captured = run_inline_scripts(html, 0);
        assert_eq!(captured, "<p>Dynamic</p>01");
    }

    #[test]
    fn cleartimeout_cancels_callback() {
        let html = r#"<script>
var id = setTimeout(function(){document.write("late");}, 50);
clearTimeout(id);
</script>"#;
        assert_eq!(run_inline_scripts(html, 100), "");
    }

    #[test]
    fn clearinterval_stops_repeats() {
        let html = r#"<script>
var id = setInterval(function(){ document.write("x"); }, 40);
setTimeout(function(){ clearInterval(id); }, 60);
</script>"#;
        assert_eq!(run_inline_scripts(html, 100), "x");
    }

    #[test]
    fn inject_appends_before_body_close() {
        let html = "<html><body>hi</body></html>";
        let out = inject_before_body_close(html, "<p>x</p>");
        assert_eq!(out, "<html><body>hi<p>x</p></body></html>");
    }

    #[test]
    fn inject_falls_back_to_html_close_then_end() {
        let no_body = "<html>hi</html>";
        assert_eq!(inject_before_body_close(no_body, "X"), "<html>hiX</html>");
        let plain = "just text";
        assert_eq!(inject_before_body_close(plain, "X"), "just textX");
    }

    #[test]
    fn attr_value_parsing() {
        assert_eq!(attr_value_ci(r#"<script src="a.js">"#, "src"), Some("a.js".into()));
        assert_eq!(attr_value_ci(r#"<script type='text/javascript'>"#, "type"), Some("text/javascript".into()));
        assert!(has_attr_ci("<script src=a.js>", "src"));
        assert!(!has_attr_ci("<script>", "src"));
    }
}
