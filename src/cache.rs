use moka::future::Cache;
use std::sync::Arc;
use std::time::Duration;
use bytes::Bytes;

/// Cache for storing fetched icons to avoid repeated requests
pub struct IconCache {
    cache: Cache<String, Arc<CacheEntry>>,
}

/// Entry in the icon cache
#[derive(Clone)]
pub struct CacheEntry {
    pub content: Bytes,
    pub content_type: String,
    pub etag: String,
}

impl IconCache {
    /// Create a new icon cache with the specified max capacity and TTL
    pub fn new(max_capacity: u64, ttl_seconds: u64) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_capacity)
            .time_to_live(Duration::from_secs(ttl_seconds))
            .build();
            
        IconCache { cache }
    }
    
    /// Get an entry from the cache
    pub async fn get(&self, key: &str) -> Option<Arc<CacheEntry>> {
        self.cache.get(key).await
    }
    
    /// Insert an entry into the cache
    pub async fn insert(&self, key: String, content: Bytes, content_type: String, etag: String) {
        let entry = Arc::new(CacheEntry {
            content,
            content_type,
            etag,
        });
        
        self.cache.insert(key, entry).await;
    }
}

/// Create a default icon cache with reasonable defaults
pub fn create_default_icon_cache() -> IconCache {
    // Default: 1000 entries, 1 hour TTL
    IconCache::new(1000, 3600)
}
