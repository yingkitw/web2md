//! YouTube transcript extraction.
//!
//! Detects `youtube.com/watch?v=` and `youtu.be/<id>` URLs, fetches the watch
//! page, and extracts the `captionTracks` JSON config. We then fetch the
//! caption file (XML `vtt` or `srv3`) and convert it to a Markdown transcript
//! with timestamps.
//!
//! No video download, no LLM. Cheaper than Firecrawl's `video`/`audio` formats
//! (each call costs 5 credits there) — web2md returns plain text only.

use anyhow::{Context, Result};
use regex::Regex;

#[derive(Debug, Clone)]
pub struct TranscriptCue {
    pub start_ms: u64,
    pub duration_ms: u64,
    pub text: String,
}

/// True if `url` looks like a YouTube watch or shortlink URL.
pub fn is_youtube_url(url: &str) -> bool {
    extract_video_id(url).is_some()
}

/// Extract the canonical video id from a YouTube URL.
pub fn extract_video_id(url: &str) -> Option<String> {
    let patterns = [
        // https://www.youtube.com/watch?v=ID
        Regex::new(r"(?i)youtube\.com/watch\?(?:[^#]*&)*v=([a-zA-Z0-9_-]{1,15})").ok()?,
        // https://www.youtube.com/shorts/ID
        Regex::new(r"(?i)youtube\.com/shorts/([a-zA-Z0-9_-]{1,15})").ok()?,
        // https://youtu.be/ID
        Regex::new(r"(?i)youtu\.be/([a-zA-Z0-9_-]{1,15})").ok()?,
        // https://m.youtube.com/watch?v=ID
        Regex::new(r"(?i)m\.youtube\.com/watch\?(?:[^#]*&)*v=([a-zA-Z0-9_-]{1,15})").ok()?,
        // https://www.youtube.com/embed/ID
        Regex::new(r"(?i)youtube\.com/embed/([a-zA-Z0-9_-]{1,15})").ok()?,
    ];
    for re in patterns {
        if let Some(cap) = re.captures(url) {
            return Some(cap[1].to_string());
        }
    }
    None
}

/// Parse a `captionTracks` JSON array embedded in the YouTube watch page HTML.
/// Returns the first manual track's `baseUrl`, ready to be fetched.
pub fn extract_caption_track_url(watch_html: &str, prefer_lang: Option<&str>) -> Option<String> {
    // Find the opening `"captionTracks":[`
    let open = watch_html.find("\"captionTracks\":[")?;
    let bytes = watch_html.as_bytes();
    // Walk forward from `[` to the matching `]`, tracking object depth.
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut end_idx = None;
    let start = open + "\"captionTracks\":".len();
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    end_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let block = &watch_html[start..end_idx?];
    parse_caption_array(block, prefer_lang)
}

/// Walk a JSON-like array of caption track objects and pick a `baseUrl`.
/// YouTube's track URLs contain raw `&` characters which break strict JSON
/// parsing, so we use a robust hand-rolled scanner that finds top-level `{...}`
/// objects and uses simple regex captures on each.
fn parse_caption_array(block: &str, prefer_lang: Option<&str>) -> Option<String> {
    // Find each top-level `{...}` object in the array (depth tracked).
    let bytes = block.as_bytes();
    let mut objects: Vec<String> = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut start: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0
                    && let Some(s) = start.take() {
                        objects.push(
                            std::str::from_utf8(&bytes[s..=i]).unwrap_or("").to_string(),
                        );
                    }
            }
            _ => {}
        }
    }

    let url_re = Regex::new(r#""baseUrl"\s*:\s*"([^"]*)""#).ok()?;
    let lang_re = Regex::new(r#""languageCode"\s*:\s*"([^"]*)""#).ok()?;

    let mut first: Option<String> = None;
    if let Some(lang) = prefer_lang {
        for obj in &objects {
            if lang_re.captures(obj).map(|c| c[1].to_string()).as_deref() == Some(lang)
                && let Some(u) = url_re.captures(obj) {
                    return Some(unquote_yt(&u[1]));
                }
        }
    }
    for obj in &objects {
        if let Some(u) = url_re.captures(obj) {
            let url = unquote_yt(&u[1]);
            if first.is_none() {
                first = Some(url);
            }
        }
    }
    first
}

fn unquote_yt(s: &str) -> String {
    s.replace("\\u0026", "&")
        .replace("\\/", "/")
        .replace("\\u003c", "<")
        .replace("\\u003e", ">")
}

/// Parse a YouTube timed-text XML/SRV3 response into cues.
/// Supports the most common XML form: `<text start="X" dur="Y">caption</text>`.
pub fn parse_timed_text(xml: &str) -> Result<Vec<TranscriptCue>> {
    let mut cues = Vec::new();
    let re = Regex::new(r#"<text\s+([^/>]*)/?>(.*?)</text>|<text\s+([^/>]*)/>"#)
        .context("compiling timed-text regex")?;
    for cap in re.captures_iter(xml) {
        let attrs = cap.get(1).or_else(|| cap.get(3)).map(|m| m.as_str()).unwrap_or("");
        let text = cap
            .get(2)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        if text.is_empty() {
            continue;
        }
        let start_ms = attr_u64(attrs, "start").unwrap_or(0);
        let duration_ms = attr_u64(attrs, "dur").unwrap_or(0);
        cues.push(TranscriptCue {
            start_ms,
            duration_ms,
            text: decode_xml_entities(&text),
        });
    }
    Ok(cues)
}

fn attr_u64(attrs: &str, name: &str) -> Option<u64> {
    let re = Regex::new(&format!(r#"{}="(\d+)""#, name)).ok()?;
    re.captures(attrs)
        .and_then(|c| c[1].parse::<u64>().ok())
}

fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&nbsp;", " ")
        .replace("<br />", "\n")
        .replace("<br>", "\n")
}

/// Render cues as Markdown: each cue becomes a `HH:MM:SS` timestamp + text line.
pub fn render_transcript_markdown(cues: &[TranscriptCue]) -> String {
    let mut out = String::new();
    out.push_str("# YouTube Transcript\n\n");
    for cue in cues {
        let ts = format_timestamp(cue.start_ms);
        out.push_str(&format!("**`{}`** {}\n\n", ts, cue.text));
    }
    out
}

fn format_timestamp(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}

/// Top-level helper: returns Markdown transcript for `url`. The caller must pass
/// the raw HTML of the watch page. The function fetches the captions file with
/// the supplied async `fetcher` (so the caller controls networking).
pub async fn transcript_from_watch_html<F, Fut>(
    url: &str,
    watch_html: &str,
    prefer_lang: Option<&str>,
    fetcher: F,
) -> Result<Option<String>>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<String>>,
{
    let video_id = match extract_video_id(url) {
        Some(id) => id,
        None => return Ok(None),
    };
    let track_url = match extract_caption_track_url(watch_html, prefer_lang) {
        Some(u) => u,
        None => return Ok(None),
    };
    let caption_xml = fetcher(track_url).await?;
    let cues = parse_timed_text(&caption_xml)?;
    let _ = video_id;
    Ok(Some(render_transcript_markdown(&cues)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_video_id_handles_watch_url() {
        assert_eq!(
            extract_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn extract_video_id_handles_shortlink() {
        assert_eq!(
            extract_video_id("https://youtu.be/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn extract_video_id_handles_shorts() {
        assert_eq!(
            extract_video_id("https://www.youtube.com/shorts/abcdef12345"),
            Some("abcdef12345".to_string())
        );
    }

    #[test]
    fn extract_video_id_returns_none_for_non_youtube() {
        assert!(extract_video_id("https://example.com/").is_none());
    }

    #[test]
    fn extract_caption_track_url_finds_first_track() {
        let html = r#"
        {"responseContext":{"...stuff..."},
         "captions":{"playerCaptionsTracklistRenderer":{"captionTracks":[
           {"baseUrl":"https://www.youtube.com/api/timedtext?lang=en&v=abc","languageCode":"en","name":"English"},
           {"baseUrl":"https://www.youtube.com/api/timedtext?lang=fr&v=abc","languageCode":"fr","name":"French"}
         ]}}}
        "#;
        let url = extract_caption_track_url(html, None).expect("track url");
        assert!(url.contains("lang=en"));
    }

    #[test]
    fn extract_caption_track_url_prefers_requested_lang() {
        let html = r#"
        "captionTracks":[
           {"baseUrl":"https://www.youtube.com/api/timedtext?lang=en&v=abc","languageCode":"en"},
           {"baseUrl":"https://www.youtube.com/api/timedtext?lang=fr&v=abc","languageCode":"fr"}
        ]
        "#;
        let url = extract_caption_track_url(html, Some("fr")).expect("track url");
        assert!(url.contains("lang=fr"));
    }

    #[test]
    fn parse_timed_text_extracts_cues() {
        let xml = r#"
        <transcript>
          <text start="1000" dur="4000">Hello world</text>
          <text start="5000" dur="3500">Second line</text>
        </transcript>
        "#;
        let cues = parse_timed_text(xml).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Hello world");
        assert_eq!(cues[0].start_ms, 1000);
        assert_eq!(cues[1].text, "Second line");
    }

    #[test]
    fn format_timestamp_pads_zero() {
        assert_eq!(format_timestamp(0), "00:00");
        assert_eq!(format_timestamp(1000), "00:01");
        assert_eq!(format_timestamp(3_661_000), "01:01:01");
    }

    #[test]
    fn render_transcript_outputs_timestamps() {
        let cues = vec![
            TranscriptCue {
                start_ms: 0,
                duration_ms: 1000,
                text: "Hi".to_string(),
            },
            TranscriptCue {
                start_ms: 60_000,
                duration_ms: 1000,
                text: "World".to_string(),
            },
        ];
        let md = render_transcript_markdown(&cues);
        assert!(md.contains("**`00:00`** Hi"));
        assert!(md.contains("**`01:00`** World"));
    }

    #[test]
    fn is_youtube_url_detects_known_forms() {
        assert!(is_youtube_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(is_youtube_url("https://youtu.be/dQw4w9WgXcQ"));
        assert!(!is_youtube_url("https://example.com/watch?v=abc"));
    }

    #[tokio::test]
    async fn transcript_from_watch_html_end_to_end() {
        let watch_html = r#"
        "captionTracks":[
          {"baseUrl":"https://www.youtube.com/api/timedtext?v=abc&lang=en","languageCode":"en"}
        ]
        "#;
        let captions_xml =
            r#"<transcript><text start="0" dur="1000">hello</text></transcript>"#;
        let md = transcript_from_watch_html(
            "https://www.youtube.com/watch?v=abc",
            watch_html,
            None,
            |_url| async move { Ok(captions_xml.to_string()) },
        )
        .await
        .unwrap()
        .expect("transcript");
        assert!(md.contains("# YouTube Transcript"));
        assert!(md.contains("**`00:00`** hello"));
    }
}
