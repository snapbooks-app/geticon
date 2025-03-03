use actix_web::{get, web, HttpResponse, HttpRequest, http::header};
use md5;
use crate::url_utils::normalize_url;
use crate::models::IconResponse;
use crate::favicon::{get_page_icons, find_best_icon_for_size};
use crate::cache::IconCache;
use std::env;
use std::sync::Arc;
use bytes::Bytes;

/// Home page handler with documentation
#[get("/")]
pub async fn home() -> HttpResponse {
    let html = r#"<!DOCTYPE html>
<html>
<head>
    <title>GetIcon - Favicon Fetcher</title>
    <style>
        body { font-family: sans-serif; max-width: 800px; margin: 40px auto; padding: 0 20px; line-height: 1.6; }
        pre { background: #f4f4f4; padding: 15px; border-radius: 5px; }
        code { background: #f4f4f4; padding: 2px 4px; border-radius: 3px; }
    </style>
</head>
<body>
    <h1>GetIcon - Favicon Fetcher</h1>
    <p>Advanced API to fetch the best favicons from websites.</p>
    
    <h2>Usage</h2>
    <h3>Get favicon as image:</h3>
    <pre>/img?url=https://google.com</pre>
    <p>Optional: specify size with <code>size</code> parameter:</p>
    <pre>/img?url=https://google.com&size=192</pre>
    
    <h3>Get favicon information as JSON:</h3>
    <pre>/json?url=https://google.com</pre>
    
    <h3>Health check endpoint:</h3>
    <pre>/health</pre>

    <h2>Features</h2>
    <ul>
        <li>Supports full and partial URLs</li>
        <li>Detects multiple icon types including web app manifests</li>
        <li>Smart icon selection based on quality scoring</li>
        <li>Returns image dimensions and purpose information</li>
        <li>Server-side caching for consistent results</li>
        <li>ETag support for efficient client-side caching</li>
    </ul>
    
    <h2>Icon Detection</h2>
    <p>GetIcon searches for icons in multiple locations:</p>
    <ul>
        <li>Standard favicon.ico in site root</li>
        <li>HTML link tags with rel="icon", "shortcut icon", etc.</li>
        <li>Apple Touch icons</li>
        <li>Web App Manifest icons</li>
        <li>Microsoft Tile images</li>
        <li>Open Graph images (as fallback)</li>
    </ul>
    
    <h2>Icon Scoring</h2>
    <p>Icons are scored based on:</p>
    <ul>
        <li>Format quality (SVG > PNG > ICO > GIF)</li>
        <li>Size (larger icons score higher for high-DPI displays)</li>
        <li>Purpose (maskable icons for Android score higher)</li>
    </ul>
</body>
</html>"#;

    HttpResponse::Ok()
        .content_type("text/html")
        .body(html)
}

/// Handler for /img endpoint - returns the best favicon as an image
#[get("/img")]
pub async fn get_favicon_img(
    url: web::Query<std::collections::HashMap<String, String>>,
    req: HttpRequest,
    client: web::Data<reqwest::Client>,
    cache: web::Data<Arc<IconCache>>
) -> HttpResponse {
    
    // Get and validate URL
    let url_str = match url.get("url") {
        Some(u) => u,
        None => return HttpResponse::BadRequest().body("Missing url parameter"),
    };
    
    let normalized_url = match normalize_url(url_str).await {
        Some(u) => u,
        None => return HttpResponse::BadRequest().body("Invalid URL"),
    };
    
    // Get size parameter if provided
    let requested_size = url.get("size").and_then(|s| s.parse::<u32>().ok());
    
    // Create a cache key that includes the size parameter if provided
    let cache_key = match requested_size {
        Some(size) => format!("{}:{}", normalized_url, size),
        None => normalized_url.to_string(),
    };
    
    // Check if the icon is in the cache
    if let Some(cached_entry) = cache.get(&cache_key).await {
        // Check if the client has the same version (ETag)
        if let Some(if_none_match) = req.headers().get(header::IF_NONE_MATCH) {
            if if_none_match.to_str().unwrap_or("") == cached_entry.etag {
                return HttpResponse::NotModified().finish();
            }
        }
        
        // Return the cached icon
        return HttpResponse::Ok()
            .content_type(cached_entry.content_type.as_str())
            .append_header((header::CACHE_CONTROL, "public, max-age=3600"))
            .append_header((header::ETAG, cached_entry.etag.clone()))
            .body(cached_entry.content.clone());
    }
    
    // If not in cache, fetch icons from the website
    let icons = match get_page_icons(client.as_ref(), &normalized_url).await {
        icons if !icons.is_empty() => icons,
        _ => return HttpResponse::NotFound().body("No icons found")
    };
    
    // Select the best icon based on requested size or highest score
    let best_icon = match find_best_icon_for_size(&icons, requested_size) {
        Some(icon) => icon,
        None => return HttpResponse::NotFound().body("No suitable icon found"),
    };
    
    // Fetch the icon
    match client.get(&best_icon.url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.bytes().await {
                    Ok(bytes) => {
                        let etag = format!("\"{:x}\"", md5::compute(&bytes));
                        
                        // Check if the client has the same version
                        if let Some(if_none_match) = req.headers().get(header::IF_NONE_MATCH) {
                            if if_none_match.to_str().unwrap_or("") == etag {
                                return HttpResponse::NotModified().finish();
                            }
                        }
                        
                        // Store in cache
                        cache.insert(
                            cache_key,
                            bytes.clone(),
                            best_icon.content_type.clone(),
                            etag.clone()
                        ).await;

                        HttpResponse::Ok()
                            .content_type(best_icon.content_type.as_str())
                            .append_header((header::CACHE_CONTROL, "public, max-age=3600"))
                            .append_header((header::ETAG, etag))
                            .body(bytes)
                    },
                    Err(err) => {
                        // Capture error with Sentry if enabled
                        if env::var("SENTRY_DSN").is_ok() {
                            sentry::capture_message(
                                &format!("Failed to read icon content: {}", err),
                                sentry::Level::Error
                            );
                        }
                        
                        HttpResponse::InternalServerError()
                            .body("Failed to read icon content")
                    }
                }
            } else {
                let status = response.status();
                
                // Capture error with Sentry if enabled
                if env::var("SENTRY_DSN").is_ok() {
                    sentry::capture_message(
                        &format!("Icon not found. Status: {}", status),
                        sentry::Level::Warning
                    );
                }
                
                HttpResponse::NotFound()
                    .body(format!("Icon not found. Status: {}", status))
            }
        }
        Err(err) => {
            // Capture error with Sentry if enabled
            if env::var("SENTRY_DSN").is_ok() {
                sentry::capture_message(
                    &format!("Failed to fetch icon: {}", err),
                    sentry::Level::Error
                );
            }
            
            // Determine appropriate status code based on error type
            if err.is_timeout() {
                HttpResponse::GatewayTimeout()
                    .body(format!("Request timed out while fetching icon: {}", err))
            } else if err.is_connect() {
                HttpResponse::BadGateway()
                    .body(format!("Connection error while fetching icon: {}", err))
            } else {
                HttpResponse::InternalServerError()
                    .body(format!("Failed to fetch icon: {}", err))
            }
        }
    }
}

/// Health check endpoint
#[get("/health")]
pub async fn health_check() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("application/json")
        .body(r#"{"status":"ok","service":"geticon"}"#)
}

/// Handler for /json endpoint - returns favicon information as JSON
#[get("/json")]
pub async fn get_favicon_json(
    url: web::Query<std::collections::HashMap<String, String>>,
    client: web::Data<reqwest::Client>,
    cache: web::Data<Arc<IconCache>>
) -> HttpResponse {
    
    // Get and validate URL
    let url_str = match url.get("url") {
        Some(u) => u,
        None => return HttpResponse::BadRequest().body("Missing url parameter"),
    };
    
    let normalized_url = match normalize_url(url_str).await {
        Some(u) => u,
        None => return HttpResponse::BadRequest().body("Invalid URL"),
    };
    
    // Get size parameter if provided
    let requested_size = url.get("size").and_then(|s| s.parse::<u32>().ok());
    
    // Create a cache key that includes the size parameter if provided
    let cache_key = match requested_size {
        Some(size) => format!("{}:json:{}", normalized_url, size),
        None => format!("{}:json", normalized_url),
    };
    
    // Check if the response is in the cache
    if let Some(cached_entry) = cache.get(&cache_key).await {
        // Return the cached JSON response
        return HttpResponse::Ok()
            .content_type(cached_entry.content_type.as_str())
            .append_header((header::CACHE_CONTROL, "public, max-age=3600"))
            .append_header((header::ETAG, cached_entry.etag.clone()))
            .body(cached_entry.content.clone());
    }
    
    // If not in cache, fetch icons from the website
    let icons = match get_page_icons(client.as_ref(), &normalized_url).await {
        icons if !icons.is_empty() => icons,
        _ => return HttpResponse::NotFound().body("No icons found")
    };
    
    // Select best icon based on requested size or highest score
    let best_icon = find_best_icon_for_size(&icons, requested_size)
        .cloned();
    
    // Create response
    let response = IconResponse {
        url: normalized_url.host_str().unwrap_or(url_str).to_string(),
        icons: icons.clone(),
        best_icon,
    };
    
    match serde_json::to_string(&response) {
        Ok(json) => {
            // Generate ETag for the JSON response
            let etag = format!("\"{:x}\"", md5::compute(json.as_bytes()));
            
            // Store in cache (as bytes for consistency with the image endpoint)
            cache.insert(
                cache_key,
                Bytes::from(json.clone()),
                "application/json".to_string(),
                etag.clone()
            ).await;
            
            HttpResponse::Ok()
                .content_type("application/json")
                .append_header((header::CACHE_CONTROL, "public, max-age=3600"))
                .append_header((header::ETAG, etag))
                .body(json)
        },
        Err(err) => {
            // Capture error with Sentry if enabled
            if env::var("SENTRY_DSN").is_ok() {
                sentry::capture_message(
                    &format!("Failed to serialize JSON response: {}", err),
                    sentry::Level::Error
                );
            }
            
            HttpResponse::InternalServerError()
                .body(format!("Failed to generate JSON response: {}", err))
        }
    }
}
