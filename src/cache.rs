use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use std::fs;
use std::path::PathBuf;

/// Simple TTL-based JSON cache.
#[derive(Debug, Clone)]
pub struct Cache {
    ttl_secs: u64,
    dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry<T> {
    data: T,
    timestamp: DateTime<Utc>,
}

impl Cache {
    pub fn new(ttl_secs: u64) -> Self {
        let dir = Self::cache_dir();
        Self { ttl_secs, dir }
    }

    /// Cache directory: ~/.cache/git-tui/ (macOS/Linux)
    /// or %LOCALAPPDATA%\git-tui\cache\ (Windows)
    fn cache_dir() -> PathBuf {
        let base = dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".cache"));
        base.join("git-tui")
    }

    /// Get a cached value if it exists and hasn't expired the HARD TTL (e.g. 48 hours).
    /// Returns `(Option<T>, bool)` where the boolean is `is_fresh` (true if < TTL).
    pub fn get_with_freshness<T: for<'de> Deserialize<'de>>(&self, key: &str) -> (Option<T>, bool) {
        let path = self.dir.join(format!("{}.json", key));
        if !path.exists() {
            return (None, false);
        }

        let contents = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return (None, false),
        };
        let entry: CacheEntry<T> = match serde_json::from_str(&contents) {
            Ok(e) => e,
            Err(_) => return (None, false),
        };

        let elapsed = Utc::now()
            .signed_duration_since(entry.timestamp)
            .num_seconds();

        let is_fresh = elapsed < self.ttl_secs as i64;
        let hard_ttl = 48 * 3600; // 48 hours max age for "stale" data

        if elapsed < hard_ttl {
            (Some(entry.data), is_fresh)
        } else {
            (None, false) // Too old even to show as stale
        }
    }

    /// Backwards compatible get (only fresh data)
    #[allow(dead_code)]
    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        let (data, is_fresh) = self.get_with_freshness(key);
        if is_fresh {
            data
        } else {
            None
        }
    }

    /// Get with a custom TTL override for freshness check (seconds).
    /// Returns `(Option<T>, bool)` where bool is `is_fresh` using the custom TTL.
    pub fn get_with_custom_ttl<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
        custom_ttl_secs: u64,
    ) -> (Option<T>, bool) {
        let path = self.dir.join(format!("{}.json", key));
        if !path.exists() {
            return (None, false);
        }

        let contents = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return (None, false),
        };
        let entry: CacheEntry<T> = match serde_json::from_str(&contents) {
            Ok(e) => e,
            Err(_) => return (None, false),
        };

        let elapsed = Utc::now()
            .signed_duration_since(entry.timestamp)
            .num_seconds();

        let is_fresh = elapsed < custom_ttl_secs as i64;
        let hard_ttl = 48 * 3600;

        if elapsed < hard_ttl {
            (Some(entry.data), is_fresh)
        } else {
            (None, false)
        }
    }

    /// Store a value in the cache. Write-to-temp + rename so a crash mid-write
    /// (or two app instances) can't leave a torn file.
    pub fn set<T: Serialize>(&self, key: &str, data: &T) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        let entry = CacheEntry {
            data,
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&entry)?;
        let path = self.dir.join(format!("{}.json", key));
        let tmp = self.dir.join(format!("{}.json.tmp", key));
        fs::write(&tmp, json)?;
        // Windows rename fails if the target exists - remove first.
        let _ = fs::remove_file(&path);
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Invalidate a specific cache key.
    #[allow(dead_code)]
    pub fn invalidate(&self, key: &str) {
        let path = self.dir.join(format!("{}.json", key));
        let _ = fs::remove_file(path);
    }

    /// Invalidate all cached data.
    pub fn clear(&self) {
        let _ = fs::remove_dir_all(&self.dir);
    }

    /// Get the timestamp of a cached entry (when it was last written).
    #[allow(dead_code)]
    pub fn get_timestamp(&self, key: &str) -> Option<DateTime<Utc>> {
        let path = self.dir.join(format!("{}.json", key));
        if !path.exists() {
            return None;
        }
        let contents = fs::read_to_string(&path).ok()?;
        // Parse just the timestamp field from the entry
        let entry: serde_json::Value = serde_json::from_str(&contents).ok()?;
        let ts_str = entry.get("timestamp")?.as_str()?;
        DateTime::parse_from_rfc3339(ts_str)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test cache with a temp directory so tests don't pollute the real cache.
    fn test_cache(ttl_secs: u64) -> Cache {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join("git-tui-test-cache")
            .join(format!("t{}_{:?}", id, std::thread::current().id()));
        Cache { ttl_secs, dir }
    }

    #[test]
    fn test_set_and_get() {
        let cache = test_cache(300);
        cache.set("test_key", &vec!["hello", "world"]).unwrap();
        let (result, is_fresh) = cache.get_with_freshness::<Vec<String>>("test_key");
        assert!(result.is_some());
        assert!(is_fresh);
        assert_eq!(result.unwrap(), vec!["hello", "world"]);
        cache.clear();
    }

    #[test]
    fn test_ttl_freshness() {
        // With TTL=1s, data written now should be fresh
        let cache = test_cache(1);
        cache.set("fresh_key", &42u32).unwrap();
        let (data, is_fresh) = cache.get_with_freshness::<u32>("fresh_key");
        assert_eq!(data, Some(42));
        assert!(is_fresh); // Just written → fresh

        // With TTL=0s, same data should be stale but still returned (< 48h hard TTL)
        let stale_cache = Cache {
            ttl_secs: 0,
            dir: cache.dir.clone(),
        };
        let (data, is_fresh) = stale_cache.get_with_freshness::<u32>("fresh_key");
        assert_eq!(data, Some(42)); // Still returned (within hard TTL)
        assert!(!is_fresh); // But marked stale

        cache.clear();
    }

    #[test]
    fn test_custom_ttl() {
        let cache = test_cache(10); // default TTL = 10s
        cache.set("custom_key", &"data").unwrap();

        // Custom TTL = 3600s → should be fresh
        let (data, is_fresh) = cache.get_with_custom_ttl::<String>("custom_key", 3600);
        assert_eq!(data.as_deref(), Some("data"));
        assert!(is_fresh);

        // Custom TTL = 0s → should be stale
        let (data, is_fresh) = cache.get_with_custom_ttl::<String>("custom_key", 0);
        assert_eq!(data.as_deref(), Some("data"));
        assert!(!is_fresh);

        cache.clear();
    }

    #[test]
    fn test_invalidate() {
        let cache = test_cache(300);
        cache.set("del_key", &"value").unwrap();

        // Verify exists
        let (data, _) = cache.get_with_freshness::<String>("del_key");
        assert!(data.is_some());

        // Invalidate
        cache.invalidate("del_key");
        let (data, _) = cache.get_with_freshness::<String>("del_key");
        assert!(data.is_none());

        cache.clear();
    }

    #[test]
    fn test_get_nonexistent_key() {
        let cache = test_cache(300);
        let (data, is_fresh) = cache.get_with_freshness::<String>("no_such_key");
        assert!(data.is_none());
        assert!(!is_fresh);
    }
}
