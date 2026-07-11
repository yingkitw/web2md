//! Shared HTML string helpers used across conversion and extraction modules.

/// Case-insensitive byte search. Returns byte position of `needle` in `haystack`.
pub(crate) fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let needle_lower: Vec<u8> = needle.bytes().map(|b| b.to_ascii_lowercase()).collect();
    let h = haystack.as_bytes();
    if needle_lower.len() > h.len() {
        return None;
    }
    'outer: for i in 0..=h.len() - needle_lower.len() {
        for (j, &n) in needle_lower.iter().enumerate() {
            if h[i + j].to_ascii_lowercase() != n {
                continue 'outer;
            }
        }
        return Some(i);
    }
    None
}

/// Strip HTML tags, leaving only text content.
pub(crate) fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(c);
        }
    }
    out
}

/// Decode common HTML character entities in text nodes.
pub(crate) fn decode_html_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some((decoded, len)) = decode_entity(&text[i..]) {
                out.push_str(&decoded);
                i += len;
                continue;
            }
        }
        out.push(text[i..].chars().next().unwrap());
        i += text[i..].chars().next().unwrap().len_utf8();
    }
    out
}

fn decode_entity(rest: &str) -> Option<(String, usize)> {
    let semi = rest.find(';')?;
    let entity = &rest[..=semi];
    let body = &rest[1..semi];

    let decoded = match body {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        "nbsp" => "\u{00A0}".to_string(),
        _ if body.starts_with("#x") || body.starts_with("#X") => {
            u32::from_str_radix(&body[2..], 16)
                .ok()
                .and_then(char::from_u32)
                .map(|c| c.to_string())?
        }
        _ if body.starts_with('#') => body[1..]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(|c| c.to_string())?,
        _ => return None,
    };
    Some((decoded, entity.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_ci_is_case_insensitive() {
        assert_eq!(find_ci("Hello WORLD", "world"), Some(6));
    }

    #[test]
    fn decodes_named_entities() {
        assert_eq!(decode_html_entities("a &amp; b &lt; c"), "a & b < c");
    }

    #[test]
    fn decodes_numeric_entities() {
        assert_eq!(decode_html_entities("&#169; &#x50;"), "© P");
    }

    #[test]
    fn leaves_unknown_entities_unchanged() {
        assert_eq!(decode_html_entities("&unknown;"), "&unknown;");
    }

    #[test]
    fn strip_html_tags_removes_markup() {
        assert_eq!(strip_html_tags("<p>Hello <b>world</b></p>"), "Hello world");
    }
}
