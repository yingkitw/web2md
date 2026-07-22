//! Persistent file-based cache for HTTP responses.
//!
//! Each entry is stored as one JSON file: `{sha256(url)}.json`.
//! Files contain `{ "url", "fetched_at" (unix millis), "body" }`.
//! Lookups respect TTL: stale entries are ignored and may be overwritten.
//!
//! Used when `--cache-dir` is set on the CLI; otherwise the in-memory cache in
//! `browser.rs` is used. Persistent cache survives process restarts and is
//! shareable across multiple `web2md` runs (perfect for agent loops).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    url: String,
    fetched_at: u64,
    body: String,
}

pub struct PersistentCache {
    dir: PathBuf,
    ttl: Duration,
}

impl PersistentCache {
    /// Create a persistent cache rooted at `dir`. The directory is created
    /// if it does not already exist.
    pub fn new(dir: impl AsRef<Path>, ttl: Duration) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating cache directory {}", dir.display()))?;
        Ok(Self { dir, ttl })
    }

    /// Look up `url` in the cache. Returns `Some(body)` if a non-expired entry exists.
    pub fn get(&self, url: &str) -> Option<String> {
        let entry = self.read_entry(url).ok()?;
        if !self.is_fresh(&entry) {
            return None;
        }
        Some(entry.body)
    }

    /// Store `body` under `url` in the cache. Overwrites any existing entry.
    pub fn put(&self, url: &str, body: &str) -> Result<()> {
        let entry = CacheEntry {
            url: url.to_string(),
            fetched_at: unix_millis(),
            body: body.to_string(),
        };
        let path = self.path_for(url);
        let json =
            serde_json::to_string(&entry).context("serializing cache entry")?;
        std::fs::write(&path, json)
            .with_context(|| format!("writing cache entry to {}", path.display()))?;
        Ok(())
    }

    /// Remove the cache entry for `url`. No-op if absent.
    pub fn invalidate(&self, url: &str) -> Result<()> {
        let path = self.path_for(url);
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("removing cache entry {}", path.display()))?;
        }
        Ok(())
    }

    /// Remove expired entries under the cache directory.
    /// Returns the number of entries removed.
    pub fn prune(&self) -> Result<usize> {
        if self.ttl.is_zero() {
            return Ok(0);
        }
        let mut removed = 0usize;
        for entry in std::fs::read_dir(&self.dir)
            .with_context(|| format!("reading cache dir {}", self.dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Ok(data) = std::fs::read_to_string(&path)
                && let Ok(c) = serde_json::from_str::<CacheEntry>(&data)
                    && !self.is_fresh(&c) {
                        let _ = std::fs::remove_file(&path);
                        removed += 1;
                    }
        }
        Ok(removed)
    }

    fn path_for(&self, url: &str) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        let digest = hasher.finalize();
        let name = digest
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        self.dir.join(format!("{}.json", name))
    }

    fn read_entry(&self, url: &str) -> Result<CacheEntry> {
        let path = self.path_for(url);
        let data = std::fs::read_to_string(&path)
            .with_context(|| format!("reading cache entry {}", path.display()))?;
        serde_json::from_str(&data).context("parsing cache entry")
    }

    fn is_fresh(&self, entry: &CacheEntry) -> bool {
        if self.ttl.is_zero() {
            return false;
        }
        let now = unix_millis();
        let age = now.saturating_sub(entry.fetched_at);
        age < self.ttl.as_millis() as u64
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "web2md-cache-test-{}-{}",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn put_then_get_returns_body() {
        let dir = temp_dir("putget");
        let cache = PersistentCache::new(&dir, Duration::from_secs(60)).unwrap();
        cache.put("https://example.com/", "<html>Hi</html>").unwrap();
        assert_eq!(
            cache.get("https://example.com/").as_deref(),
            Some("<html>Hi</html>")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn get_returns_none_after_ttl() {
        let dir = temp_dir("ttl");
        let cache = PersistentCache::new(&dir, Duration::from_millis(50)).unwrap();
        cache.put("https://example.com/", "x").unwrap();
        assert!(cache.get("https://example.com/").is_some());
        std::thread::sleep(Duration::from_millis(80));
        assert!(cache.get("https://example.com/").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn url_keys_dont_collide() {
        let dir = temp_dir("keys");
        let cache = PersistentCache::new(&dir, Duration::from_secs(60)).unwrap();
        cache.put("https://a.example/", "A").unwrap();
        cache.put("https://b.example/", "B").unwrap();
        assert_eq!(cache.get("https://a.example/").as_deref(), Some("A"));
        assert_eq!(cache.get("https://b.example/").as_deref(), Some("B"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalidate_removes_entry() {
        let dir = temp_dir("invalidate");
        let cache = PersistentCache::new(&dir, Duration::from_secs(60)).unwrap();
        cache.put("https://example.com/", "x").unwrap();
        cache.invalidate("https://example.com/").unwrap();
        assert!(cache.get("https://example.com/").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_removes_stale_entries() {
        let dir = temp_dir("prune");
        let cache = PersistentCache::new(&dir, Duration::from_millis(50)).unwrap();
        cache.put("u1", "x").unwrap();
        cache.put("u2", "y").unwrap();
        std::thread::sleep(Duration::from_millis(80));
        let n = cache.prune().unwrap();
        assert_eq!(n, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
