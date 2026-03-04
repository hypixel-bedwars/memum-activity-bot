/// A generic time-based cache backed by `tokio::sync::RwLock`.
///
/// Entries expire after a configurable TTL (time-to-live). This is used by the
/// Hypixel client to avoid redundant API calls for the same player within a
/// short window, but the implementation is fully generic and can be reused by
/// any module that needs short-lived caching.
use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

/// A single cache entry storing the value and the instant it was inserted.
struct CacheEntry<V> {
    value: V,
    inserted_at: Instant,
}

/// A concurrent, TTL-based cache.
///
/// # Type Parameters
/// - `K` — key type (must be `Eq + Hash + Clone`).
/// - `V` — value type (must be `Clone` so callers get owned copies).
pub struct TimedCache<K, V> {
    /// The TTL after which entries are considered stale.
    ttl: Duration,
    /// Interior-mutable map protected by a tokio read-write lock.
    entries: RwLock<HashMap<K, CacheEntry<V>>>,
}

impl<K, V> TimedCache<K, V>
where
    K: Eq + Hash + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    /// Create a new cache with the given TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Retrieve a value if it exists and has not expired.
    ///
    /// Returns `None` if the key is missing or the entry is older than the TTL.
    pub async fn get(&self, key: &K) -> Option<V> {
        let entries = self.entries.read().await;
        if let Some(entry) = entries.get(key) {
            if entry.inserted_at.elapsed() < self.ttl {
                return Some(entry.value.clone());
            }
        }
        None
    }

    /// Insert or overwrite a value, resetting its TTL.
    pub async fn insert(&self, key: K, value: V) {
        let mut entries = self.entries.write().await;
        entries.insert(
            key,
            CacheEntry {
                value,
                inserted_at: Instant::now(),
            },
        );
    }

    /// Remove all entries that have exceeded their TTL.
    ///
    /// This is not called automatically — the caller (or a periodic task) should
    /// invoke it when appropriate to reclaim memory.
    pub async fn purge_expired(&self) {
        let mut entries = self.entries.write().await;
        entries.retain(|_, entry| entry.inserted_at.elapsed() < self.ttl);
    }
}
