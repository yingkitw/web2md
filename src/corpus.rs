//! Local BM25 corpus index over a directory of Markdown files.
//!
//! Two operations, both free of any external service:
//!
//! - [`build_index`] walks `dir` recursively, reads every `.md` / `.markdown` file,
//!   tokenizes the text, and writes a JSON index to `<dir>/.web2md-index.json`.
//! - [`query_index`] loads the persisted index, ranks files by BM25 score, and
//!   returns the top-N matches with a short snippet around the first match.
//!
//! This mirrors Context7's "ask questions of a library" surface but is local
//! and operates on any directory of Markdown the user has previously fetched
//! (e.g. via `web2md batch --output ./docs`). No LLM, no API key, no network.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const INDEX_FILENAME: &str = ".web2md-index.json";
const INDEX_VERSION: u32 = 1;
const K1: f64 = 1.2;
const B: f64 = 0.75;
const SNIPPET_RADIUS: usize = 80;

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "have", "in", "is",
    "it", "its", "of", "on", "or", "that", "the", "this", "to", "was", "were", "will", "with",
    "but", "if", "not", "you", "your", "we", "our", "they", "their", "them", "i", "me", "he",
    "she", "his", "her", "about", "into", "than", "then", "so", "do", "does", "did", "can",
    "could", "should", "would", "may", "might", "must", "shall", "also", "any", "all", "one",
    "two", "three", "these", "those", "such", "when", "where", "while", "who", "what", "which",
    "how", "why",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexDoc {
    path: String,
    length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Posting {
    doc: usize,
    tf: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedIndex {
    version: u32,
    avg_doc_length: f64,
    docs: Vec<IndexDoc>,
    /// term -> list of (doc_index, term_frequency)
    index: HashMap<String, Vec<Posting>>,
}

/// A single ranked result from [`query_index`].
#[derive(Debug, Clone, Serialize)]
pub struct CorpusHit {
    pub path: String,
    pub score: f64,
    pub snippet: String,
}

/// Tokenize text the same way the indexer and the query path do.
/// Lowercased alphanumeric tokens; stopwords dropped; length > 1.
fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for low in ch.to_lowercase() {
                cur.push(low);
            }
        } else if !cur.is_empty() {
            if cur.len() > 1 && !STOPWORDS.contains(&cur.as_str()) {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if cur.len() > 1 && !STOPWORDS.contains(&cur.as_str()) {
        out.push(cur);
    }
    out
}

/// Build (or rebuild) the on-disk index for `dir`. Returns the number of
/// documents indexed. Symlinks are not followed. Hidden files / dirs are
/// skipped to keep the corpus predictable. Output is written to
/// `<dir>/.web2md-index.json` unless `output` overrides it.
pub fn build_index(dir: &Path, output: Option<&Path>) -> Result<usize> {
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_markdown(dir, &mut paths)?;
    paths.sort();

    let mut docs: Vec<IndexDoc> = Vec::with_capacity(paths.len());
    let mut inverted: HashMap<String, Vec<Posting>> = HashMap::new();
    let mut total_length = 0usize;

    for (idx, path) in paths.iter().enumerate() {
        let body = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("warning: skipping {}: {}", path.display(), e);
                continue;
            }
        };
        let tokens = tokenize(&body);
        let length = tokens.len();
        if length == 0 {
            continue;
        }
        let mut tf_map: HashMap<&str, usize> = HashMap::new();
        for tok in &tokens {
            *tf_map.entry(tok.as_str()).or_insert(0) += 1;
        }
        let rel = path
            .strip_prefix(dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        docs.push(IndexDoc {
            path: rel,
            length,
        });
        for (term, tf) in tf_map {
            inverted
                .entry(term.to_string())
                .or_default()
                .push(Posting { doc: idx, tf });
        }
        total_length += length;
    }

    let avg = if docs.is_empty() {
        0.0
    } else {
        total_length as f64 / docs.len() as f64
    };
    let persisted = PersistedIndex {
        version: INDEX_VERSION,
        avg_doc_length: avg,
        docs,
        index: inverted,
    };

    let out_path = output
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| dir.join(INDEX_FILENAME));
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
    let json = serde_json::to_string_pretty(&persisted)?;
    std::fs::write(&out_path, json)
        .with_context(|| format!("writing index to {}", out_path.display()))?;

    Ok(persisted.docs.len())
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            return Err(anyhow::anyhow!("reading {}: {}", dir.display(), e));
        }
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry under {}", dir.display()))?;
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            collect_markdown(&path, out)?;
        } else if ft.is_file() {
            let lower = name_str.to_ascii_lowercase();
            if lower.ends_with(".md") || lower.ends_with(".markdown") {
                out.push(path);
            }
        }
    }
    Ok(())
}

/// Load an index from disk. `path` may be the index file itself or the
/// directory containing it.
fn load_index(path: &Path) -> Result<PersistedIndex> {
    let index_path = if path.is_dir() {
        path.join(INDEX_FILENAME)
    } else {
        path.to_path_buf()
    };
    let body = std::fs::read_to_string(&index_path)
        .with_context(|| format!("reading index {}", index_path.display()))?;
    let idx: PersistedIndex = serde_json::from_str(&body)
        .with_context(|| format!("parsing index {}", index_path.display()))?;
    if idx.version != INDEX_VERSION {
        anyhow::bail!(
            "unsupported index version {} (expected {}) — rebuild with `web2md corpus index`",
            idx.version,
            INDEX_VERSION
        );
    }
    Ok(idx)
}

/// Rank documents for a free-form `query`. Returns the top `limit` matches
/// sorted by descending BM25 score. `path` is the corpus dir or the index file.
pub fn query_index(
    path: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<CorpusHit>> {
    let idx = load_index(path)?;
    let tokens = tokenize(query);
    if tokens.is_empty() {
        return Ok(Vec::new());
    }

    let n = idx.docs.len() as f64;
    let mut scores: Vec<f64> = vec![0.0; idx.docs.len()];

    for term in &tokens {
        let Some(postings) = idx.index.get(term) else { continue };
        let df = postings.len() as f64;
        if df == 0.0 {
            continue;
        }
        let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
        for p in postings {
            let len = idx.docs[p.doc].length as f64;
            let tf = p.tf as f64;
            let denom = tf + K1 * (1.0 - B + B * len / idx.avg_doc_length.max(1.0));
            if denom > 0.0 {
                scores[p.doc] += idf * (tf * (K1 + 1.0)) / denom;
            }
        }
    }

    let mut ranked: Vec<(usize, f64)> = scores
        .into_iter()
        .enumerate()
        .filter(|(_, s)| *s > 0.0)
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(limit);

    let mut out = Vec::with_capacity(ranked.len());
    for (doc_idx, score) in ranked {
        let rel = &idx.docs[doc_idx].path;
        let full = path.join(rel);
        let snippet = if let Ok(body) = std::fs::read_to_string(&full) {
            make_snippet(&body, &tokens)
        } else {
            String::new()
        };
        out.push(CorpusHit {
            path: rel.clone(),
            score,
            snippet,
        });
    }
    Ok(out)
}

/// Build a `~SNIPPET_RADIUS` character window around the first occurrence of
/// any query token. Falls back to the head of the document when nothing matches.
fn make_snippet(body: &str, query_tokens: &[String]) -> String {
    let lower = body.to_lowercase();
    for term in query_tokens {
        if let Some(pos) = lower.find(term.as_str()) {
            let start = pos.saturating_sub(SNIPPET_RADIUS);
            let end = (pos + term.len() + SNIPPET_RADIUS).min(body.len());
            // Snap to char boundaries.
            let mut s = start;
            while s > 0 && !body.is_char_boundary(s) {
                s -= 1;
            }
            let mut e = end;
            while e < body.len() && !body.is_char_boundary(e) {
                e += 1;
            }
            let mut window = body[s..e].to_string();
            window = window.replace('\n', " ");
            window = window.split_whitespace().collect::<Vec<_>>().join(" ");
            let prefix = if s > 0 { "…" } else { "" };
            let suffix = if e < body.len() { "…" } else { "" };
            return format!("{prefix}{window}{suffix}");
        }
    }
    let head: String = body.chars().take(SNIPPET_RADIUS * 2).collect();
    let head = head.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{head}…")
}

/// Render [`CorpusHit`]s as Markdown (default output for `corpus query`).
pub fn results_to_markdown(hits: &[CorpusHit]) -> String {
    if hits.is_empty() {
        return "_No matches._".to_string();
    }
    let mut out = String::new();
    for (i, hit) in hits.iter().enumerate() {
        out.push_str(&format!(
            "## {}. {}\n\nScore: `{:.3}`\n\n",
            i + 1,
            hit.path,
            hit.score
        ));
        if !hit.snippet.is_empty() {
            out.push_str(&format!("> {}\n\n", hit.snippet));
        }
    }
    out.trim_end().to_string()
}

/// Path of the index file for a corpus directory. Exposed for tests / CLI.
pub fn index_path_for(dir: &Path) -> PathBuf {
    dir.join(INDEX_FILENAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn tokenizer_lowercases_and_drops_stopwords() {
        let toks = tokenize("The Rust Cargo build system and the compiler");
        assert!(toks.contains(&"rust".to_string()));
        assert!(toks.contains(&"cargo".to_string()));
        assert!(toks.contains(&"build".to_string()));
        assert!(toks.contains(&"system".to_string()));
        assert!(toks.contains(&"compiler".to_string()));
        assert!(!toks.contains(&"the".to_string()));
        assert!(!toks.contains(&"and".to_string()));
    }

    #[test]
    fn build_and_query_finds_relevant_doc() {
        let dir = std::env::temp_dir().join(format!("web2md-corpus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write(
            &dir,
            "rust.md",
            "Rust is a systems programming language focused on safety and concurrency. \
             Cargo is the Rust package manager and build system.",
        );
        write(
            &dir,
            "python.md",
            "Python is a high-level dynamic language often used for scripting and data science. \
             pip is the package manager for Python.",
        );
        write(
            &dir,
            "nodejs.md",
            "Node.js is a JavaScript runtime built on Chrome's V8 engine. \
             npm is the default package manager.",
        );

        let n = build_index(&dir, None).unwrap();
        assert_eq!(n, 3);
        assert!(index_path_for(&dir).exists());

        let hits = query_index(&dir, "rust cargo", 5).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].path, "rust.md");
        assert!(hits[0].snippet.to_lowercase().contains("rust"));
        assert!(hits[0].score > 0.0);

        let py_hits = query_index(&dir, "data science", 5).unwrap();
        assert!(!py_hits.is_empty());
        assert_eq!(py_hits[0].path, "python.md");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn query_respects_limit() {
        let dir = std::env::temp_dir().join(format!("web2md-corpus-lim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..10 {
            write(
                &dir,
                &format!("doc{i}.md"),
                &format!("Rust Cargo package manager {i}"),
            );
        }
        build_index(&dir, None).unwrap();
        let hits = query_index(&dir, "rust cargo", 3).unwrap();
        assert_eq!(hits.len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn query_empty_returns_empty() {
        let dir = std::env::temp_dir().join(format!("web2md-corpus-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write(&dir, "a.md", "Rust Cargo.");
        build_index(&dir, None).unwrap();
        let hits = query_index(&dir, "", 5).unwrap();
        assert!(hits.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn query_handles_no_match() {
        let dir = std::env::temp_dir().join(format!("web2md-corpus-nomatch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write(&dir, "a.md", "Rust Cargo.");
        build_index(&dir, None).unwrap();
        let hits = query_index(&dir, "klingon bakery", 5).unwrap();
        assert!(hits.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_skips_hidden_and_non_markdown() {
        let dir = std::env::temp_dir().join(format!("web2md-corpus-skip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write(&dir, "kept.md", "Rust Cargo build system");
        write(&dir, ".hidden.md", "Should be skipped");
        write(&dir, "ignored.txt", "Should be skipped");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        write(&dir, ".git/HEAD.md", "Should be skipped");
        let n = build_index(&dir, None).unwrap();
        assert_eq!(n, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_recurses_into_subdirectories() {
        let dir = std::env::temp_dir().join(format!("web2md-corpus-recurse-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        write(&dir, "top.md", "Rust Cargo top level");
        write(&dir, "sub/nested.md", "Python pip nested");
        let n = build_index(&dir, None).unwrap();
        assert_eq!(n, 2);
        let hits = query_index(&dir, "python", 5).unwrap();
        assert_eq!(hits[0].path, "sub/nested.md");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn results_markdown_renders_empty_state() {
        let md = results_to_markdown(&[]);
        assert!(md.contains("No matches"));
    }

    #[test]
    fn results_markdown_renders_hits() {
        let hits = vec![CorpusHit {
            path: "a.md".to_string(),
            score: 1.5,
            snippet: "snippet text".to_string(),
        }];
        let md = results_to_markdown(&hits);
        assert!(md.contains("a.md"));
        assert!(md.contains("1.500"));
        assert!(md.contains("> snippet text"));
    }

    #[test]
    fn custom_output_path_works() {
        let dir = std::env::temp_dir().join(format!("web2md-corpus-out-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write(&dir, "a.md", "Rust Cargo");
        let out = dir.join("custom-index.json");
        let n = build_index(&dir, Some(&out)).unwrap();
        assert_eq!(n, 1);
        assert!(out.exists());
        // Default index file should NOT exist
        assert!(!index_path_for(&dir).exists());
        let hits = query_index(&out, "rust", 5).unwrap();
        assert_eq!(hits[0].path, "a.md");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
