//! Shared HTML metadata helpers: `<meta>` tag parsing and JSON-LD block iteration.

use crate::html_util::find_ci;

/// Iterate over all JSON-LD blocks in the HTML, parsing each as JSON.
pub(crate) fn iter_json_ld_blocks(html: &str) -> impl Iterator<Item = serde_json::Value> + '_ {
    let mut pos = 0usize;
    std::iter::from_fn(move || {
        while pos < html.len() {
            let rest = &html[pos..];
            let ld_pos = find_ci(rest, "application/ld+json")?;
            let abs = pos + ld_pos;
            let script_close = find_ci(&html[abs..], "</script>")?;
            let block = &html[abs..abs + script_close];
            let gt = block.find('>')?;
            let json_content = &block[gt + 1..];
            pos = abs + script_close + 9;
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_content) {
                return Some(json);
            }
        }
        None
    })
}

/// Extract a string field from the first JSON-LD block that contains it.
pub(crate) fn extract_json_ld_field(html: &str, field: &str) -> Option<String> {
    for json in iter_json_ld_blocks(html) {
        if let Some(val) = json.get(field).and_then(|v| v.as_str()) {
            return Some(val.to_string());
        }
    }
    None
}

/// Extract `content` from a `<meta>` tag matching the given attribute key/value pair.
pub(crate) fn extract_meta_content(html: &str, attr_key: &str, attr_val: &str) -> Option<String> {
    let mut i = 0;
    while i < html.len() {
        if let Some(pos) = find_ci(&html[i..], "<meta") {
            let pos = i + pos;
            let tag_end = html[pos..].find('>').map(|e| pos + e)?;
            let tag = &html[pos..=tag_end];
            if find_ci(tag, &format!("{}=\"{}\"", attr_key, attr_val)).is_some()
                || find_ci(tag, &format!("{}='{}'", attr_key, attr_val)).is_some()
            {
                return extract_attr(tag, "content");
            }
            i = tag_end + 1;
        } else {
            break;
        }
    }
    None
}

/// Extract the value of an attribute from an HTML tag string.
pub(crate) fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let needle = format!("{}=", attr);
    let pos = find_ci(tag, &needle)?;
    let after = &tag[pos + needle.len()..];
    let mut i = 0;
    while i < after.len() && after.as_bytes()[i].is_ascii_whitespace() {
        i += 1;
    }
    let quote = *after.as_bytes().get(i)? as char;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let val_start = i + 1;
    let val_end = after[val_start..].find(quote)? + val_start;
    Some(after[val_start..val_end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iter_json_ld_blocks_parses_multiple_scripts() {
        let html = r#"
        <script type="application/ld+json">{"headline":"One"}</script>
        <script type="application/ld+json">{"headline":"Two"}</script>
        "#;
        let headlines: Vec<String> = iter_json_ld_blocks(html)
            .filter_map(|j| j.get("headline").and_then(|v| v.as_str()).map(str::to_string))
            .collect();
        assert_eq!(headlines, vec!["One", "Two"]);
    }

    #[test]
    fn extract_meta_content_finds_og_description() {
        let html = r#"<meta property="og:description" content="Hello world">"#;
        assert_eq!(
            extract_meta_content(html, "property", "og:description"),
            Some("Hello world".into())
        );
    }

    #[test]
    fn extract_json_ld_field_reads_headline() {
        let html = r#"<script type="application/ld+json">{"headline":"Big story"}</script>"#;
        assert_eq!(
            extract_json_ld_field(html, "headline"),
            Some("Big story".into())
        );
    }
}
