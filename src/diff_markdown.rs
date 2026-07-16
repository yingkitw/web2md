//! Line-level Markdown diff between two pages (URL vs URL, or URL vs cache).
//!
//! Used by the `diff` subcommand. The smallest correct implementation:
//! compute the LCS of the two line arrays, then emit a unified-diff string
//! with ` ` for equal, `-` for deleted-only lines, and `+` for inserted-only
//! lines.
//!
//! Pages are typically a few hundred lines so an O(n*m) LCS is fast enough
//! and avoids pulling a heavier crate like `similar`.

/// Compute the longest-common-subsequence line indices between two line arrays.
fn lcs_indices(a: &[&str], b: &[&str]) -> Vec<(usize, usize)> {
    let n = a.len();
    let m = b.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in 0..n {
        for j in 0..m {
            dp[i + 1][j + 1] = if a[i] == b[j] {
                dp[i][j] + 1
            } else {
                dp[i][j + 1].max(dp[i + 1][j])
            };
        }
    }
    let mut out = Vec::new();
    let mut i = n;
    let mut j = m;
    while i > 0 && j > 0 {
        if a[i - 1] == b[j - 1] {
            out.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if dp[i - 1][j] >= dp[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    out.reverse();
    out
}

/// Emit a unified-diff string between two lines of Markdown text, prefixed by URL labels.
pub fn diff_markdown(
    label_a: &str,
    markdown_a: &str,
    label_b: &str,
    markdown_b: &str,
) -> String {
    let a: Vec<&str> = markdown_a.lines().collect();
    let b: Vec<&str> = markdown_b.lines().collect();
    let matches = lcs_indices(&a, &b);

    let mut out = String::new();
    out.push_str(&format!("--- {}\n", label_a));
    out.push_str(&format!("+++ {}\n", label_b));

    let mut ai = 0usize;
    let mut bi = 0usize;
    let mut k = 0usize;
    while ai < a.len() || bi < b.len() {
        if k < matches.len() {
            let (ma, mb) = matches[k];
            // Delete all A lines up to the next match.
            while ai < ma {
                out.push_str(&format!("-{}\n", a[ai]));
                ai += 1;
            }
            // Insert B lines up to the next match.
            while bi < mb {
                out.push_str(&format!("+{}\n", b[bi]));
                bi += 1;
            }
            // The matched line.
            out.push_str(&format!(" {}\n", a[ai]));
            ai += 1;
            bi += 1;
            k += 1;
        } else {
            // Tail: only A or only B left.
            while ai < a.len() {
                out.push_str(&format!("-{}\n", a[ai]));
                ai += 1;
            }
            while bi < b.len() {
                out.push_str(&format!("+{}\n", b[bi]));
                bi += 1;
            }
        }
    }
    out
}

/// Count added/removed line pairs in a diff string.
/// Used to print a one-line summary when running `diff`.
pub fn summarize(diff: &str) -> (usize, usize) {
    let mut added = 0usize;
    let mut removed = 0usize;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix('+') {
            if !rest.starts_with("++") {
                added += 1;
            }
        } else if let Some(rest) = line.strip_prefix('-') {
            if !rest.starts_with("--") {
                removed += 1;
            }
        }
    }
    (added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_marks_identical_lines_as_unchanged() {
        let diff = diff_markdown("a.md", "hello\nworld\n", "b.md", "hello\nworld\n");
        let (added, removed) = summarize(&diff);
        assert_eq!(added, 0);
        assert_eq!(removed, 0);
    }

    #[test]
    fn diff_marks_insertions() {
        let diff = diff_markdown("a.md", "hello\n", "b.md", "hello\nworld\n");
        assert!(diff.lines().any(|l| l.starts_with('+') && l.contains("world")));
        let (added, removed) = summarize(&diff);
        assert_eq!(added, 1);
        assert_eq!(removed, 0);
    }

    #[test]
    fn diff_marks_deletions() {
        let diff = diff_markdown("a.md", "hello\nworld\n", "b.md", "hello\n");
        let (added, removed) = summarize(&diff);
        assert_eq!(removed, 1);
        assert_eq!(added, 0);
    }

    #[test]
    fn diff_detects_changes_mid_text() {
        let diff = diff_markdown(
            "a",
            "line one\nline two\nline three\n",
            "b",
            "line one\nline changed\nline three\n",
        );
        let (added, removed) = summarize(&diff);
        assert_eq!(added + removed, 2);
    }

    #[test]
    fn diff_handles_empty_a() {
        let diff = diff_markdown("a", "", "b", "new line\n");
        let (added, removed) = summarize(&diff);
        assert_eq!(added, 1);
        assert_eq!(removed, 0);
    }

    #[test]
    fn diff_handles_empty_b() {
        let diff = diff_markdown("a", "old line\n", "b", "");
        let (added, removed) = summarize(&diff);
        assert_eq!(removed, 1);
        assert_eq!(added, 0);
    }
}
