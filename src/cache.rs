use actix_web::http::StatusCode;
use moka::future::Cache;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use bytes::Bytes;
use log::{info, debug};
use crate::models::Icon;

/// Why a lookup produced no icon; cached like a result so repeat requests get the same answer
#[derive(Clone, Debug)]
pub struct Failure {
    pub status: StatusCode,
    pub message: String,
}

/// An icon image ready to serve from /img
#[derive(Clone)]
pub struct CachedImage {
    pub content: Bytes,
    pub content_type: String,
    pub etag: String,
}

/// A cached lookup outcome and the window in which it counts as fresh
pub struct CacheEntry<T> {
    pub value: Result<T, Failure>,
    fetched_at: Instant,
    fresh_until: Instant,
}

impl<T> CacheEntry<T> {
    fn new(value: Result<T, Failure>, fresh_for: Duration, failure_fresh_for: Duration) -> Self {
        let now = Instant::now();
        let window = if value.is_ok() { fresh_for } else { failure_fresh_for };
        CacheEntry { value, fetched_at: now, fresh_until: now + window }
    }
}

/// Stale-while-revalidate cache: fresh entries are served as-is, stale successes are served
/// while one background task refreshes them, and misses share a single load per key
pub struct SwrCache<T> {
    entries: Cache<String, Arc<CacheEntry<T>>>,
    // Keys with a background refresh in flight; the TTL frees a key even if its task dies
    refreshing: Cache<String, ()>,
    fresh_for: Duration,
    failure_fresh_for: Duration,
    max_age: Duration,
}

impl<T: Clone + Send + Sync + 'static> SwrCache<T> {
    /// `fresh_for` applies to successes and `failure_fresh_for` to failures;
    /// a success is served stale until `max_age` after it was fetched
    pub fn new(max_capacity: u64, fresh_for: Duration, failure_fresh_for: Duration, max_age: Duration) -> Self {
        SwrCache {
            entries: Cache::builder()
                .max_capacity(max_capacity)
                .time_to_live(max_age)
                .build(),
            refreshing: Cache::builder()
                .time_to_live(Duration::from_secs(60))
                .build(),
            fresh_for,
            failure_fresh_for,
            max_age,
        }
    }

    /// Returns the entry for `key` and whether it is stale, calling `load` only on a miss,
    /// on an expired failure, or (in the background) to refresh a stale success
    pub async fn get_or_load<F, Fut>(&self, key: &str, load: F) -> (Arc<CacheEntry<T>>, bool)
    where
        F: FnOnce() -> Fut + 'static,
        Fut: Future<Output = Result<T, Failure>> + 'static,
    {
        let now = Instant::now();
        if let Some(entry) = self.entries.get(key).await {
            if now < entry.fresh_until {
                debug!("Fresh cache hit for key: {}", key);
                return (entry, false);
            }
            // Failures are never served stale: a site that recovered should work on the next request
            if entry.value.is_ok() && now < entry.fetched_at + self.max_age {
                debug!("Stale cache hit for key: {}", key);
                self.refresh_in_background(key, entry.clone(), load).await;
                return (entry, true);
            }
        }

        debug!("Cache miss for key: {}", key);
        let (fresh_for, failure_fresh_for) = (self.fresh_for, self.failure_fresh_for);
        let entry = self.entries.entry(key.to_string())
            .or_insert_with_if(
                async move { Arc::new(CacheEntry::new(load().await, fresh_for, failure_fresh_for)) },
                |entry| Instant::now() >= entry.fresh_until,
            )
            .await
            .into_value();
        (entry, false)
    }

    async fn refresh_in_background<F, Fut>(&self, key: &str, stale: Arc<CacheEntry<T>>, load: F)
    where
        F: FnOnce() -> Fut + 'static,
        Fut: Future<Output = Result<T, Failure>> + 'static,
    {
        if !self.refreshing.entry(key.to_string()).or_insert(()).await.is_fresh() {
            debug!("Refresh already running for key: {}", key);
            return;
        }

        let entries = self.entries.clone();
        let refreshing = self.refreshing.clone();
        let key = key.to_string();
        let (fresh_for, failure_fresh_for) = (self.fresh_for, self.failure_fresh_for);
        actix_web::rt::spawn(async move {
            debug!("Background refresh started for key: {}", key);
            let entry = match load().await {
                Ok(value) => CacheEntry::new(Ok(value), fresh_for, failure_fresh_for),
                // Keep serving the last good value and retry once the failure window has passed
                Err(failure) => {
                    debug!("Background refresh failed for key {}: {}", key, failure.message);
                    CacheEntry {
                        value: stale.value.clone(),
                        fetched_at: stale.fetched_at,
                        fresh_until: Instant::now() + failure_fresh_for,
                    }
                }
            };
            entries.insert(key.clone(), Arc::new(entry)).await;
            refreshing.invalidate(&key).await;
        });
    }

    pub fn entry_count(&self) -> u64 {
        self.entries.entry_count()
    }
}

/// Icon caches shared by all workers
pub struct IconCache {
    /// Validated icons per page URL, shared by /img (every size) and /json
    pub discovery: SwrCache<Vec<Icon>>,
    /// Image bytes per icon URL
    pub images: SwrCache<CachedImage>,
}

/// Fresh for 2 hours (failures 1 hour), served stale for up to 3 days while refreshing
fn default_swr_cache<T: Clone + Send + Sync + 'static>() -> SwrCache<T> {
    SwrCache::new(
        2000,
        Duration::from_secs(2 * 60 * 60),
        Duration::from_secs(60 * 60),
        Duration::from_secs(3 * 24 * 60 * 60),
    )
}

/// Create the icon cache with default sizes and freshness windows
pub fn create_default_icon_cache() -> IconCache {
    info!("Created icon cache (2-hour freshness, served stale for up to 3 days while refreshing)");
    IconCache { discovery: default_swr_cache(), images: default_swr_cache() }
}
