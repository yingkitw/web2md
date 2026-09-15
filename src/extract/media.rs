//! Part of the `extract` module.

use serde::Serialize;

use crate::html_meta::{extract_attr, iter_json_ld_blocks};
use crate::html_util::find_ci;
use super::{json_ld_value_is_type, json_string, resolve_url};

/// A single video extracted from the page.
#[derive(Debug, Serialize)]
pub struct VideoEntry {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Thumbnail / poster image URL (≈ Firecrawl `thumbnail`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    /// Duration in seconds when known (from `duration` attr or JSON-LD VideoObject).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

/// Extract all video URLs from HTML: `<video src>`, `<video><source src>`,
/// `<iframe>` embeds (YouTube, Vimeo, etc.), and JSON-LD `VideoObject` blocks.
/// URLs are resolved against `base_url`. Returns deduplicated entries in document order.
pub fn extract_videos(html: &str, base_url: &str) -> Vec<VideoEntry> {
    let mut videos = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut pos = 0;

    // Extract from <video src="..."> and <video><source src="...">
    while pos < html.len() {
        let Some(start) = find_ci(&html[pos..], "<video") else {
            break;
        };
        let start = pos + start;
        let Some(end) = html[start..].find('>') else {
            break;
        };
        let tag = &html[start..=start + end];
        let tag_end = start + end + 1;

        let title = extract_attr(tag, "title").filter(|s| !s.is_empty());
        let thumbnail = extract_attr(tag, "poster")
            .filter(|s| !s.is_empty())
            .and_then(|s| resolve_url(&s, base_url));
        let duration = extract_attr(tag, "duration")
            .and_then(|s| parse_duration_seconds(&s));

        // Check for src attribute on <video> tag itself
        if let Some(src) = extract_attr(tag, "src")
            && let Some(resolved) = resolve_url(&src, base_url)
                && seen.insert(resolved.clone()) {
                    videos.push(VideoEntry {
                        url: resolved,
                        source: Some("video".to_string()),
                        title: title.clone(),
                        thumbnail: thumbnail.clone(),
                        duration,
                    });
                }

        // Look for <source> tags within the <video> element
        let video_close = find_ci(&html[tag_end..], "</video>").unwrap_or(200);
        let video_inner = &html[tag_end..tag_end + video_close.min(html.len() - tag_end)];
        let mut source_pos = 0;
        while source_pos < video_inner.len() {
            let Some(s_start) = find_ci(&video_inner[source_pos..], "<source") else {
                break;
            };
            let s_start = source_pos + s_start;
            let Some(s_end) = video_inner[s_start..].find('>') else {
                break;
            };
            let source_tag = &video_inner[s_start..=s_start + s_end];
            if let Some(src) = extract_attr(source_tag, "src")
                && let Some(resolved) = resolve_url(&src, base_url)
                    && seen.insert(resolved.clone()) {
                        videos.push(VideoEntry {
                            url: resolved,
                            source: Some("source".to_string()),
                            title: title.clone(),
                            thumbnail: thumbnail.clone(),
                            duration,
                        });
                    }
            source_pos = s_start + s_end + 1;
        }

        pos = tag_end;
    }

    // Extract from <iframe> embeds (YouTube, Vimeo, etc.)
    pos = 0;
    while pos < html.len() {
        let Some(start) = find_ci(&html[pos..], "<iframe") else {
            break;
        };
        let start = pos + start;
        let Some(end) = html[start..].find('>') else {
            break;
        };
        let tag = &html[start..=start + end];
        let tag_end = start + end + 1;

        if let Some(src) = extract_attr(tag, "src")
            && let Some(resolved) = resolve_url(&src, base_url)
                && is_video_embed(&resolved)
                    && seen.insert(resolved.clone()) {
                        let source = embed_source_name(&resolved);
                        let title = extract_attr(tag, "title").filter(|s| !s.is_empty());
                        videos.push(VideoEntry {
                            url: resolved,
                            source: Some(source.to_string()),
                            title,
                            thumbnail: None,
                            duration: None,
                        });
                    }
        pos = tag_end;
    }

    // Merge JSON-LD VideoObject blocks (title / thumbnail / duration enrichment).
    for json in iter_json_ld_blocks(html) {
        if !json_ld_value_is_type(&json, "VideoObject") {
            continue;
        }
        let Some(url) = json
            .get("contentUrl")
            .and_then(json_string)
            .or_else(|| json.get("embedUrl").and_then(json_string))
            .or_else(|| json.get("url").and_then(json_string))
            .and_then(|u| resolve_url(&u, base_url))
        else {
            continue;
        };
        let title = json.get("name").and_then(json_string);
        let thumbnail = json
            .get("thumbnailUrl")
            .and_then(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| {
                        v.as_array()
                            .and_then(|a| a.first())
                            .and_then(json_string)
                    })
            })
            .and_then(|u| resolve_url(&u, base_url));
        let duration = json
            .get("duration")
            .and_then(json_string)
            .and_then(|s| parse_duration_seconds(&s));

        if let Some(existing) = videos.iter_mut().find(|v| v.url == url) {
            if existing.title.is_none() {
                existing.title = title;
            }
            if existing.thumbnail.is_none() {
                existing.thumbnail = thumbnail;
            }
            if existing.duration.is_none() {
                existing.duration = duration;
            }
        } else if seen.insert(url.clone()) {
            videos.push(VideoEntry {
                url,
                source: Some("json-ld".to_string()),
                title,
                thumbnail,
                duration,
            });
        }
    }

    videos
}

/// Parse a duration string into seconds. Accepts plain numbers or ISO-8601
/// (`PT1H2M3S`, `PT90S`, `PT1M30S`).
fn parse_duration_seconds(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    if let Ok(n) = trimmed.parse::<f64>() {
        return Some(n);
    }
    let upper = trimmed.to_ascii_uppercase();
    if !upper.starts_with("PT") {
        return None;
    }
    let mut rest = &upper[2..];
    let mut total = 0.0;
    if let Some(h_pos) = rest.find('H') {
        let hours: f64 = rest[..h_pos].parse().ok()?;
        total += hours * 3600.0;
        rest = &rest[h_pos + 1..];
    }
    if let Some(m_pos) = rest.find('M') {
        let mins: f64 = rest[..m_pos].parse().ok()?;
        total += mins * 60.0;
        rest = &rest[m_pos + 1..];
    }
    if let Some(s_pos) = rest.find('S') {
        let secs: f64 = rest[..s_pos].parse().ok()?;
        total += secs;
    }
    if total > 0.0 {
        Some(total)
    } else {
        None
    }
}

/// Check if a URL is a known video embed (YouTube, Vimeo, Dailymotion, etc.).
fn is_video_embed(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("youtube.com/embed/")
        || lower.contains("youtube-nocookie.com/embed/")
        || lower.contains("player.vimeo.com")
        || lower.contains("dailymotion.com/embed")
        || lower.contains("player.twitch.tv")
        || lower.contains("wistia.com")
        || lower.contains("loom.com/embed")
    || lower.contains("videopress.com/embed")
}

/// Extract the platform name from a video embed URL.
fn embed_source_name(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("youtube") {
        "youtube"
    } else if lower.contains("vimeo") {
        "vimeo"
    } else if lower.contains("dailymotion") {
        "dailymotion"
    } else if lower.contains("twitch") {
        "twitch"
    } else if lower.contains("wistia") {
        "wistia"
    } else if lower.contains("loom") {
        "loom"
    } else if lower.contains("videopress") {
        "videopress"
    } else {
        "embed"
    }
}

/// Check if a URL is a known audio embed (SoundCloud, Spotify, podcast players, etc.).
fn is_audio_embed(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("soundcloud.com")
        || lower.contains("spotify.com")
        || lower.contains("mixcloud.com")
        || lower.contains("anchor.fm")
        || lower.contains("podcasts.apple.com")
        || lower.contains("iheart.com")
        || lower.contains("podbean.com")
        || lower.contains("buzzsprout.com")
        || lower.contains("transistor.fm")
        || lower.contains("libsyn.com")
        || lower.contains("spreaker.com")
        || lower.contains("simplecast.com")
        || lower.contains("captivate.fm")
}

/// Extract the platform name from an audio embed URL.
fn audio_embed_source_name(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("soundcloud") {
        "soundcloud"
    } else if lower.contains("spotify") {
        "spotify"
    } else if lower.contains("mixcloud") {
        "mixcloud"
    } else if lower.contains("anchor") {
        "anchor"
    } else if lower.contains("podcasts.apple") {
        "apple-podcasts"
    } else if lower.contains("iheart") {
        "iheart"
    } else if lower.contains("podbean") {
        "podbean"
    } else if lower.contains("buzzsprout") {
        "buzzsprout"
    } else if lower.contains("transistor") {
        "transistor"
    } else if lower.contains("libsyn") {
        "libsyn"
    } else if lower.contains("spreaker") {
        "spreaker"
    } else if lower.contains("simplecast") {
        "simplecast"
    } else if lower.contains("captivate") {
        "captivate"
    } else {
        "embed"
    }
}

/// A single audio clip extracted from the page.
#[derive(Debug, Serialize)]
pub struct AudioEntry {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Duration in seconds when known (from `duration` attr or JSON-LD AudioObject).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

/// Extract all audio URLs from HTML: `<audio src>`, `<audio><source src>`,
/// `<iframe>` audio embeds (SoundCloud, Spotify, etc.), and JSON-LD `AudioObject` blocks.
/// URLs are resolved against `base_url`. Returns deduplicated entries in document order.
pub fn extract_audios(html: &str, base_url: &str) -> Vec<AudioEntry> {
    let mut audios = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut pos = 0;

    // Extract from <audio src="..."> and <audio><source src="...">
    while pos < html.len() {
        let Some(start) = find_ci(&html[pos..], "<audio") else {
            break;
        };
        let start = pos + start;
        let Some(end) = html[start..].find('>') else {
            break;
        };
        let tag = &html[start..=start + end];
        let tag_end = start + end + 1;

        let title = extract_attr(tag, "title").filter(|s| !s.is_empty());
        let duration = extract_attr(tag, "duration")
            .and_then(|s| parse_duration_seconds(&s));

        if let Some(src) = extract_attr(tag, "src")
            && let Some(resolved) = resolve_url(&src, base_url)
                && seen.insert(resolved.clone()) {
                    audios.push(AudioEntry {
                        url: resolved,
                        source: Some("audio".to_string()),
                        title: title.clone(),
                        duration,
                    });
                }

        let audio_close = find_ci(&html[tag_end..], "</audio>").unwrap_or(200);
        let audio_inner = &html[tag_end..tag_end + audio_close.min(html.len() - tag_end)];
        let mut source_pos = 0;
        while source_pos < audio_inner.len() {
            let Some(s_start) = find_ci(&audio_inner[source_pos..], "<source") else {
                break;
            };
            let s_start = source_pos + s_start;
            let Some(s_end) = audio_inner[s_start..].find('>') else {
                break;
            };
            let source_tag = &audio_inner[s_start..=s_start + s_end];
            if let Some(src) = extract_attr(source_tag, "src")
                && let Some(resolved) = resolve_url(&src, base_url)
                    && seen.insert(resolved.clone()) {
                        audios.push(AudioEntry {
                            url: resolved,
                            source: Some("source".to_string()),
                            title: title.clone(),
                            duration,
                        });
                    }
            source_pos = s_start + s_end + 1;
        }

        pos = tag_end;
    }

    // Extract from <iframe> audio embeds.
    pos = 0;
    while pos < html.len() {
        let Some(start) = find_ci(&html[pos..], "<iframe") else {
            break;
        };
        let start = pos + start;
        let Some(end) = html[start..].find('>') else {
            break;
        };
        let tag = &html[start..=start + end];
        let tag_end = start + end + 1;

        if let Some(src) = extract_attr(tag, "src")
            && let Some(resolved) = resolve_url(&src, base_url)
                && is_audio_embed(&resolved)
                    && seen.insert(resolved.clone()) {
                        let source = audio_embed_source_name(&resolved);
                        let title = extract_attr(tag, "title").filter(|s| !s.is_empty());
                        audios.push(AudioEntry {
                            url: resolved,
                            source: Some(source.to_string()),
                            title,
                            duration: None,
                        });
                    }
        pos = tag_end;
    }

    // Merge JSON-LD AudioObject blocks (title / duration enrichment).
    for json in iter_json_ld_blocks(html) {
        if !json_ld_value_is_type(&json, "AudioObject") {
            continue;
        }
        let Some(url) = json
            .get("contentUrl")
            .and_then(json_string)
            .or_else(|| json.get("embedUrl").and_then(json_string))
            .or_else(|| json.get("url").and_then(json_string))
            .and_then(|u| resolve_url(&u, base_url))
        else {
            continue;
        };
        let title = json.get("name").and_then(json_string);
        let duration = json
            .get("duration")
            .and_then(json_string)
            .and_then(|s| parse_duration_seconds(&s));

        if let Some(existing) = audios.iter_mut().find(|a| a.url == url) {
            if existing.title.is_none() {
                existing.title = title;
            }
            if existing.duration.is_none() {
                existing.duration = duration;
            }
        } else if seen.insert(url.clone()) {
            audios.push(AudioEntry {
                url,
                source: Some("json-ld".to_string()),
                title,
                duration,
            });
        }
    }

    audios
}

