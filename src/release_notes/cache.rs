//! Work item title caching for release notes.
//!
//! Caches work item titles locally to avoid repeated API calls.
//! Cache entries expire after 7 days by default.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Default cache expiration time in days.
const DEFAULT_CACHE_EXPIRY_DAYS: i64 = 7;

/// Cached work item entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedWorkItem {
    pub id: i32,
    pub title: String,
    pub cached_at: DateTime<Utc>,
}

/// Work item title cache.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WorkItemCache {
    entries: HashMap<i32, CachedWorkItem>,
    #[serde(default)]
    version: u32,
}

impl WorkItemCache {
    /// Current cache format version (reserved for future compatibility checks).
    #[allow(dead_code)]
    const CURRENT_VERSION: u32 = 1;

    /// Load cache from disk.
    ///
    /// Returns an empty cache if the file doesn't exist or is invalid.
    pub fn load() -> Result<Self> {
        let cache_path = Self::get_cache_path()?;

        if !cache_path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&cache_path)
            .with_context(|| format!("Failed to read cache file: {}", cache_path.display()))?;

        let mut cache: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse cache file: {}", cache_path.display()))?;

        // Prune expired entries on load
        cache.prune_expired();

        Ok(cache)
    }

    /// Save cache to disk.
    pub fn save(&self) -> Result<()> {
        let cache_path = Self::get_cache_path()?;

        // Ensure directory exists
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create cache directory: {}", parent.display())
            })?;
        }

        let content = serde_json::to_string_pretty(self).context("Failed to serialize cache")?;

        fs::write(&cache_path, content)
            .with_context(|| format!("Failed to write cache file: {}", cache_path.display()))?;

        Ok(())
    }

    /// Get a cached work item title if it exists and is not expired.
    pub fn get(&self, id: i32) -> Option<&str> {
        self.entries.get(&id).and_then(|entry| {
            if Self::is_expired(&entry.cached_at) {
                None
            } else {
                Some(entry.title.as_str())
            }
        })
    }

    /// Get multiple cached titles at once.
    ///
    /// Returns a map of ID to title for cached (non-expired) entries.
    pub fn get_many(&self, ids: &[i32]) -> HashMap<i32, String> {
        ids.iter()
            .filter_map(|&id| self.get(id).map(|title| (id, title.to_string())))
            .collect()
    }

    /// Set a cached work item title.
    pub fn set(&mut self, id: i32, title: &str) {
        self.entries.insert(
            id,
            CachedWorkItem {
                id,
                title: title.to_string(),
                cached_at: Utc::now(),
            },
        );
    }

    /// Set multiple cached work item titles.
    pub fn set_many(&mut self, items: &[(i32, String)]) {
        for (id, title) in items {
            self.set(*id, title);
        }
    }

    /// Get IDs that are not in the cache or are expired.
    pub fn get_uncached_ids(&self, ids: &[i32]) -> Vec<i32> {
        ids.iter()
            .filter(|&&id| self.get(id).is_none())
            .copied()
            .collect()
    }

    /// Check if cache contains a valid (non-expired) entry for an ID.
    pub fn contains(&self, id: i32) -> bool {
        self.get(id).is_some()
    }

    /// Remove expired entries from the cache.
    pub fn prune_expired(&mut self) {
        self.entries
            .retain(|_, entry| !Self::is_expired(&entry.cached_at));
    }

    /// Clear all cache entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Get the number of entries in the cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Check if a cached entry is expired.
    fn is_expired(cached_at: &DateTime<Utc>) -> bool {
        let expiry_duration = Duration::days(DEFAULT_CACHE_EXPIRY_DAYS);
        Utc::now() - *cached_at > expiry_duration
    }

    /// Get the cache file path.
    fn get_cache_path() -> Result<PathBuf> {
        // Use XDG_CACHE_HOME if set, otherwise ~/.cache
        let cache_dir = std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .expect("Could not determine home directory")
                    .join(".cache")
            });

        Ok(cache_dir.join("mergers").join("work_items.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_temp_cache() -> (TempDir, WorkItemCache) {
        let temp_dir = TempDir::new().unwrap();
        // SAFETY: We're in a test context and modifying env vars is acceptable
        unsafe {
            std::env::set_var("XDG_CACHE_HOME", temp_dir.path());
        }
        (temp_dir, WorkItemCache::default())
    }

    #[test]
    fn test_cache_set_and_get() {
        let (_temp, mut cache) = setup_temp_cache();

        cache.set(123, "Test title");
        assert_eq!(cache.get(123), Some("Test title"));
        assert_eq!(cache.get(456), None);
    }

    #[test]
    fn test_cache_get_many() {
        let (_temp, mut cache) = setup_temp_cache();

        cache.set(100, "Title 100");
        cache.set(200, "Title 200");

        let results = cache.get_many(&[100, 200, 300]);
        assert_eq!(results.len(), 2);
        assert_eq!(results.get(&100), Some(&"Title 100".to_string()));
        assert_eq!(results.get(&200), Some(&"Title 200".to_string()));
        assert_eq!(results.get(&300), None);
    }

    #[test]
    fn test_cache_get_uncached_ids() {
        let (_temp, mut cache) = setup_temp_cache();

        cache.set(100, "Title 100");
        cache.set(200, "Title 200");

        let uncached = cache.get_uncached_ids(&[100, 200, 300, 400]);
        assert_eq!(uncached, vec![300, 400]);
    }

    #[test]
    fn test_cache_save_and_load() {
        let (temp_dir, mut cache) = setup_temp_cache();

        cache.set(123, "Saved title");
        cache.save().unwrap();

        // Create a new cache and load from disk
        // SAFETY: We're in a test context and modifying env vars is acceptable
        unsafe {
            std::env::set_var("XDG_CACHE_HOME", temp_dir.path());
        }
        let loaded_cache = WorkItemCache::load().unwrap();
        assert_eq!(loaded_cache.get(123), Some("Saved title"));
    }

    #[test]
    fn test_cache_prune_expired() {
        let (_temp, mut cache) = setup_temp_cache();

        // Add an entry with an old timestamp
        cache.entries.insert(
            999,
            CachedWorkItem {
                id: 999,
                title: "Old entry".to_string(),
                cached_at: Utc::now() - Duration::days(30),
            },
        );

        // Add a fresh entry
        cache.set(123, "Fresh entry");

        assert!(cache.entries.contains_key(&999));
        cache.prune_expired();
        assert!(!cache.entries.contains_key(&999));
        assert!(cache.entries.contains_key(&123));
    }

    #[test]
    fn test_cache_clear() {
        let (_temp, mut cache) = setup_temp_cache();

        cache.set(100, "Title 100");
        cache.set(200, "Title 200");
        assert_eq!(cache.len(), 2);

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_expired_entry_not_returned() {
        let (_temp, mut cache) = setup_temp_cache();

        // Add an entry with an old timestamp
        cache.entries.insert(
            999,
            CachedWorkItem {
                id: 999,
                title: "Expired entry".to_string(),
                cached_at: Utc::now() - Duration::days(30),
            },
        );

        // The entry exists in storage but should not be returned
        assert_eq!(cache.get(999), None);
        assert!(!cache.contains(999));
    }
}
