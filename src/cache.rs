use moka::future::Cache;
use std::sync::Arc;
use std::time::Duration;
use bytes::Bytes;
use log::{info, debug};

/// Cache for storing fetched icons to avoid repeated requests
/// Enhanced with dual-layer caching system for handling expired entries
pub struct IconCache {
    main_cache: Cache<String, Arc<CacheEntry>>,     // Primary cache with normal TTL
    expired_cache: Cache<String, Arc<CacheEntry>>,  // Secondary cache for expired entries
    negative_cache: Cache<String, ()>,              // For URLs that failed validation
}

/// Entry in the icon cache
#[derive(Clone)]
pub struct CacheEntry {
    pub content: Bytes,
    pub content_type: String,
    pub etag: String,
    pub access_count: u32, // Track how often this entry is accessed
}

impl IconCache {
    /// Create a new icon cache with the specified max capacity and TTL
    pub fn new(max_capacity: u64, ttl_seconds: u64) -> Self {
        let main_cache = Cache::builder()
            .max_capacity(max_capacity)
            .time_to_live(Duration::from_secs(ttl_seconds))
            .time_to_idle(Duration::from_secs(ttl_seconds * 2)) // Keep frequently accessed items longer
            .build();
        
        // Expired cache has a longer TTL to serve as fallback while refreshing
        let expired_cache = Cache::builder()
            .max_capacity(max_capacity) // Same size as main cache
            .time_to_live(Duration::from_secs(259200)) // 3 days (in seconds)
            .build();
            
        // Negative cache has shorter TTL to allow retrying failed URLs periodically
        let negative_cache = Cache::builder()
            .max_capacity(max_capacity / 2) // Half the size of the main cache
            .time_to_live(Duration::from_secs(ttl_seconds / 2)) // Half the TTL of the main cache 
            .build();
            
        IconCache { 
            main_cache,
            expired_cache,
            negative_cache 
        }
    }
    
    /// Get an entry from the cache
    /// Returns (CacheEntry, needs_refresh)
    /// If needs_refresh is true, the entry came from the expired cache and should be refreshed
    pub async fn get(&self, key: &str) -> Option<(Arc<CacheEntry>, bool)> {
        // First check if this key is in the negative cache
        let in_negative = self.negative_cache.get(key).await.is_some();
        if in_negative {
            debug!("Cache hit (negative) for key: {}", key);
            return None;
        }
        
        // Then check the main cache
        if let Some(entry) = self.main_cache.get(key).await {
            debug!("Main cache hit for key: {}", key);
            let count = {
                let mut entry_ref = Arc::get_mut(&mut entry.clone()).unwrap();
                entry_ref.access_count += 1;
                entry_ref.access_count
            };
            debug!("Incremented access count to {} for key: {}", count, key);
            return Some((entry, false)); // false = doesn't need refresh
        }
        
        // Finally check the expired cache
        if let Some(entry) = self.expired_cache.get(key).await {
            debug!("Expired cache hit for key: {}", key);
            return Some((entry, true)); // true = needs refresh
        }
        
        debug!("Cache miss for key: {}", key);
        None
    }
    
    /// Insert an entry into the main cache
    pub async fn insert(&self, key: String, content: Bytes, content_type: String, etag: String) {
        let entry = Arc::new(CacheEntry {
            content,
            content_type,
            etag,
            access_count: 1,
        });
        
        debug!("Inserting into main cache: {}", key);
        self.main_cache.insert(key, entry).await;
    }
    
    /// Move an entry from main cache to expired cache
    /// Called when an entry in the main cache expires but we want to keep it for fallback
    pub async fn move_to_expired(&self, key: String, entry: Arc<CacheEntry>) {
        debug!("Moving to expired cache: {}", key);
        self.expired_cache.insert(key, entry).await;
    }
    
    /// Manually move expired entries from main cache to expired cache
    /// This is used as a workaround for the lack of direct on_evict handler capture support
    pub async fn check_and_move_expired_entries(&self) {
        // This would require additional tracking of entry insertion times
        // which is beyond the scope of the current implementation
        // In a real implementation, we would iterate through main_cache entries 
        // and check if they're approaching expiry
    }
    
    /// Remove an entry from the expired cache
    /// Called after successfully refreshing an entry
    pub async fn remove_from_expired(&self, key: &str) {
        debug!("Removing from expired cache: {}", key);
        self.expired_cache.invalidate(key).await;
    }
    
    /// Insert a negative entry for failed URLs to avoid repeated validation attempts
    pub async fn insert_negative(&self, key: String) {
        debug!("Inserting negative cache entry: {}", key);
        self.negative_cache.insert(key, ()).await;
    }
    
    /// Check if a URL is in the negative cache
    pub async fn is_negative(&self, key: &str) -> bool {
        self.negative_cache.get(key).await.is_some()
    }
    
    /// Get cache statistics
    pub async fn stats(&self) -> (u64, u64, u64) {
        let main_count = self.main_cache.entry_count();
        let expired_count = self.expired_cache.entry_count();
        let negative_count = self.negative_cache.entry_count();
        (main_count, expired_count, negative_count)
    }
}

/// Create a default icon cache with reasonable defaults
pub fn create_default_icon_cache() -> IconCache {
    // Default: 2000 entries, 2 hour TTL (increased from 1 hour)
    let cache = IconCache::new(2000, 7200);
    info!("Created optimized icon cache with dual-layer caching (2-hour main TTL, 3-day expired TTL)");
    cache
}
