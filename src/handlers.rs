use actix_web::{get, web, HttpResponse, HttpRequest, http::header};
use md5;
use crate::url_utils::normalize_url;
use crate::models::IconResponse;
use crate::favicon::{get_page_icons, find_best_icon_for_size, select_user_agent_for_icon};
use crate::validation::{validate_icons, validate_image_content, is_html_content};
use crate::cache::IconCache;
use std::env;
use std::sync::Arc;
// Remove unused Duration import
use bytes::Bytes;
use std::collections::HashMap;
use log::{warn, debug, error};

/// Home page handler with documentation
#[get("/")]
pub async fn home() -> HttpResponse {
    debug!("Serving home page");
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

/// Extract important headers to forward to target sites
fn extract_headers_to_forward(req: &HttpRequest) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    
    // Extract User-Agent
    if let Some(user_agent) = req.headers().get(header::USER_AGENT) {
        if let Ok(value) = user_agent.to_str() {
            headers.insert("User-Agent".to_string(), value.to_string());
        }
    }
    
    // Extract Accept header
    if let Some(accept) = req.headers().get(header::ACCEPT) {
        if let Ok(value) = accept.to_str() {
            headers.insert("Accept".to_string(), value.to_string());
        }
    }
    
    // Extract Accept-Language header
    if let Some(accept_lang) = req.headers().get(header::ACCEPT_LANGUAGE) {
        if let Ok(value) = accept_lang.to_str() {
            headers.insert("Accept-Language".to_string(), value.to_string());
        }
    }
    
    // Extract Sec-Ch-Ua headers
    for header_name in &["Sec-Ch-Ua", "Sec-Ch-Ua-Mobile", "Sec-Ch-Ua-Platform"] {
        if let Some(header_value) = req.headers().get(*header_name) {
            if let Ok(value) = header_value.to_str() {
                headers.insert(header_name.to_string(), value.to_string());
            }
        }
    }
    
    headers
}

// Use the validate_icons function from the validation module

/// Handler for /img endpoint - returns the best favicon as an image
#[get("/img")]
pub async fn get_favicon_img(
    url: web::Query<std::collections::HashMap<String, String>>,
    req: HttpRequest,
    client: web::Data<reqwest::Client>,
    cache: web::Data<Arc<IconCache>>
) -> HttpResponse {
    debug!("Image favicon request received");
    
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
    
    // Check if the icon is in the cache (either main or expired)
    match cache.get(&cache_key).await {
        Some((cached_entry, needs_refresh)) => {
            // Check if the client has the same version (ETag)
            if let Some(if_none_match) = req.headers().get(header::IF_NONE_MATCH) {
                if if_none_match.to_str().unwrap_or("") == cached_entry.etag {
                    debug!("Client already has latest version (ETag match): {}", cache_key);
                    return HttpResponse::NotModified()
                        .append_header((header::CACHE_CONTROL, "public, max-age=7200"))
                        .finish();
                }
            }
            
            // If from expired cache, trigger background refresh and return shorter TTL
            if needs_refresh {
                debug!("Serving from expired cache while refreshing: {}", cache_key);
                
                // Extract headers to forward for the background task
                let forwarded_headers = extract_headers_to_forward(&req);
                
                // Clone variables for background task
                let cache_clone = cache.clone();
                let cache_key_clone = cache_key.clone();
                let client_clone = client.clone();
                let normalized_url_clone = normalized_url.clone();
                let forwarded_headers_clone = forwarded_headers.clone();
                let requested_size_clone = requested_size;
                
                // Launch background task to refresh the entry
                actix_web::rt::spawn(async move {
                    debug!("Background refresh task started for: {}", cache_key_clone);
                    
                    // Fetch icons from website
                    let icons = match get_page_icons(
                        client_clone.as_ref(), 
                        &normalized_url_clone, 
                        Some(&forwarded_headers_clone), 
                        None
                    ).await {
                        icons if !icons.is_empty() => icons,
                        _ => {
                            debug!("Background refresh: no icons found");
                            return;
                        }
                    };
                    
                    // Validate icons
                    let validated_icons = validate_icons(
                        client_clone.as_ref(), 
                        &icons, 
                        &forwarded_headers_clone
                    ).await;
                    
                    if validated_icons.is_empty() {
                        debug!("Background refresh: no valid icons found");
                        return;
                    }
                    
                    // Select best icon
                    let best_icon = match find_best_icon_for_size(&validated_icons, requested_size_clone) {
                        Some(icon) => icon,
                        None => {
                            debug!("Background refresh: no suitable icon found");
                            return;
                        }
                    };
                    
                    // Create a copy of forwarded headers that we can modify
                    let mut headers = forwarded_headers_clone.clone();
                    
                    // Override the User-Agent with our selected one based on icon type
                    headers.insert("User-Agent".to_string(), select_user_agent_for_icon(best_icon).to_string());
                    
                    // Fetch the icon
                    let mut request_builder = client_clone.get(&best_icon.url);
                    
                    // Apply headers
                    for (name, value) in &headers {
                        request_builder = request_builder.header(name, value);
                    }
                    
                    // Send the request
                    match request_builder.send().await {
                        Ok(response) => {
                            // Verify response is valid
                            if !response.status().is_success() {
                                debug!("Background refresh: icon request failed with status {}", response.status());
                                return;
                            }
                            
                            match response.bytes().await {
                                Ok(bytes) => {
                                    // Validate content
                                    if bytes.is_empty() || is_html_content(&bytes) {
                                        debug!("Background refresh: invalid icon content");
                                        return;
                                    }
                                    
                                    let is_valid_image = validate_image_content(&bytes, &best_icon.content_type);
                                    if !is_valid_image {
                                        debug!("Background refresh: invalid image content");
                                        return;
                                    }
                                    
                                    let etag = format!("\"{:x}\"", md5::compute(&bytes));
                                    
                                    // Update main cache with the new content
                                    cache_clone.insert(
                                        cache_key_clone.clone(), // Clone since we need to use key again
                                        bytes,
                                        best_icon.content_type.clone(),
                                        etag
                                    ).await;
                                    
                                    // Remove from expired cache since it's now in main cache
                                    cache_clone.remove_from_expired(&cache_key_clone).await;
                                    
                                    debug!("Background refresh completed successfully");
                                },
                                Err(err) => {
                                    debug!("Background refresh: Failed to read icon content: {}", err);
                                }
                            }
                        },
                        Err(err) => {
                            debug!("Background refresh: Failed to fetch icon: {}", err);
                        }
                    }
                });
                
                // Return the expired cached icon with a shorter cache duration (10 minutes)
                return HttpResponse::Ok()
                    .content_type(cached_entry.content_type.as_str())
                    .append_header((header::CACHE_CONTROL, "public, max-age=600")) // 10 minutes
                    .append_header((header::ETAG, cached_entry.etag.clone()))
                    .body(cached_entry.content.clone());
            }
            
            // If from main cache, return normal TTL
            debug!("Serving from main cache: {}", cache_key);
            return HttpResponse::Ok()
                .content_type(cached_entry.content_type.as_str())
                .append_header((header::CACHE_CONTROL, "public, max-age=7200"))
                .append_header((header::ETAG, cached_entry.etag.clone()))
                .body(cached_entry.content.clone());
        },
        None => {
            // No cache hit in either main or expired cache
        }
    }
    
    // Check if this URL is in the negative cache (previously failed)
    if cache.is_negative(&cache_key).await {
        debug!("URL in negative cache, returning 404: {}", cache_key);
        return HttpResponse::NotFound().body("Icon not found (cached negative result)");
    }
    
    // Extract headers to forward
    let forwarded_headers = extract_headers_to_forward(&req);
    
    // If not in cache, fetch icons from the website
    let icons = match get_page_icons(client.as_ref(), &normalized_url, Some(&forwarded_headers), None).await {
        icons if !icons.is_empty() => icons,
        _ => {
            // Log the failure with more details
            warn!("Failed to find icons for URL: {}", normalized_url);
            
            // Also send to Sentry if enabled
            if env::var("SENTRY_DSN").is_ok() {
                sentry::capture_message(
                    &format!("Failed to find icons for URL: {}", normalized_url),
                    sentry::Level::Warning
                );
            }
            return HttpResponse::NotFound().body("No icons found")
        }
    };
    
    // Validate icons
    let validated_icons = validate_icons(client.as_ref(), &icons, &forwarded_headers).await;
    
    // If no icons passed validation, add to negative cache and return a 404
    if validated_icons.is_empty() {
        // Add to negative cache to avoid repeated validation attempts
        let cache_key_for_log = cache_key.clone(); // Clone for logging
        cache.insert_negative(cache_key).await;
        debug!("No valid icons found, added to negative cache: {}", cache_key_for_log);
        return HttpResponse::NotFound().body("No valid icons found");
    }
    
    // Select the best icon based on requested size or highest score from validated icons
    let best_icon = match find_best_icon_for_size(&validated_icons, requested_size) {
        Some(icon) => icon,
        None => return HttpResponse::NotFound().body("No suitable icon found"),
    };
    
    // Create a copy of forwarded headers that we can modify
    let mut headers = forwarded_headers.clone();
    
    // Override the User-Agent with our selected one based on icon type
    headers.insert("User-Agent".to_string(), select_user_agent_for_icon(best_icon).to_string());
    
    // Fetch the icon with the appropriate User-Agent
    let mut request_builder = client.get(&best_icon.url);
    
    // Apply headers
    for (name, value) in &headers {
        request_builder = request_builder.header(name, value);
    }
    
    // Send the request
    match request_builder.send().await {
        Ok(response) => {
            // Check if the response was redirected to a non-image resource
            let final_url = response.url().to_string();
            if final_url != best_icon.url {
                // The request was redirected, check if the final URL is still an image
                if let Some(content_type) = response.headers().get(header::CONTENT_TYPE) {
                    if let Ok(content_type_str) = content_type.to_str() {
                        // If redirected to a non-image resource (like HTML), reject it
                        if !content_type_str.starts_with("image/") {
                            // Log the redirect
                            warn!("Icon redirected to non-image resource: {} -> {} (Content-Type: {})", 
                                best_icon.url, final_url, content_type_str);
                            
                            // Also send to Sentry if enabled
                            if env::var("SENTRY_DSN").is_ok() {
                                sentry::capture_message(
                                    &format!("Icon redirected to non-image resource: {} -> {} (Content-Type: {})", 
                                        best_icon.url, final_url, content_type_str),
                                    sentry::Level::Warning
                                );
                            }
                            
                            return HttpResponse::NotFound()
                                .body(format!("Icon redirected to non-image resource: {}", final_url));
                        }
                    }
                }
            }
            
            // Check content type header to ensure it's an image
            if let Some(content_type) = response.headers().get(header::CONTENT_TYPE) {
                if let Ok(content_type_str) = content_type.to_str() {
                    if !content_type_str.starts_with("image/") {
                        // Log the invalid content type
                        warn!("Invalid content type for icon: {} (Content-Type: {})", 
                            best_icon.url, content_type_str);
                        
                        // Also send to Sentry if enabled
                        if env::var("SENTRY_DSN").is_ok() {
                            sentry::capture_message(
                                &format!("Invalid content type for icon: {} (Content-Type: {})", 
                                    best_icon.url, content_type_str),
                                sentry::Level::Warning
                            );
                        }
                        
                        return HttpResponse::NotFound()
                            .body(format!("Invalid content type for icon: {}", content_type_str));
                    }
                }
            }
            
            if response.status().is_success() {
                match response.bytes().await {
                    Ok(bytes) => {
                        // Validate content size
                        if bytes.is_empty() {
                            // Log the zero-size icon
                            warn!("Zero-size icon detected for URL: {} from icon URL: {}", 
                                normalized_url, best_icon.url);
                            
                            // Also send to Sentry if enabled
                            if env::var("SENTRY_DSN").is_ok() {
                                sentry::capture_message(
                                    &format!("Zero-size icon detected for URL: {} from icon URL: {}", 
                                        normalized_url, best_icon.url),
                                    sentry::Level::Warning
                                );
                            }
                            
                            return HttpResponse::NotFound()
                                .body("Icon found but has zero size");
                        }
                        
                        // Check for HTML content disguised as an image
                        if is_html_content(&bytes) {
                            // Log the HTML content disguised as an image
                            warn!("HTML content disguised as an image for URL: {} from icon URL: {}", 
                                normalized_url, best_icon.url);
                            
                            // Also send to Sentry if enabled
                            if env::var("SENTRY_DSN").is_ok() {
                                sentry::capture_message(
                                    &format!("HTML content disguised as an image for URL: {} from icon URL: {}", 
                                        normalized_url, best_icon.url),
                                    sentry::Level::Warning
                                );
                            }
                            
                            return HttpResponse::NotFound()
                                .body("Icon found but content is HTML, not an image");
                        }
                        
                        // Validate image content using our validation function
                        let is_valid_image = validate_image_content(&bytes, &best_icon.content_type);
                        
                        if !is_valid_image {
                            // Log the invalid image
                            warn!("Invalid image content for URL: {} from icon URL: {}", 
                                normalized_url, best_icon.url);
                            
                            // Also send to Sentry if enabled
                            if env::var("SENTRY_DSN").is_ok() {
                                sentry::capture_message(
                                    &format!("Invalid image content for URL: {} from icon URL: {}", 
                                        normalized_url, best_icon.url),
                                    sentry::Level::Warning
                                );
                            }
                            
                            return HttpResponse::NotFound()
                                .body("Icon found but content is not a valid image");
                        }
                        
                        let etag = format!("\"{:x}\"", md5::compute(&bytes));
                        
                        // Check if the client has the same version
                        if let Some(if_none_match) = req.headers().get(header::IF_NONE_MATCH) {
                            if if_none_match.to_str().unwrap_or("") == etag {
                                return HttpResponse::NotModified().finish();
                            }
                        }
                        
                        // Store in main cache, and if it was in expired cache, remove it from there
                        cache.insert(
                            cache_key.clone(), // Clone since we need the key again
                            bytes.clone(),
                            best_icon.content_type.clone(),
                            etag.clone()
                        ).await;
                        
                        // Also check if we should remove it from expired cache
                        cache.remove_from_expired(&cache_key).await;

                        HttpResponse::Ok()
                            .content_type(best_icon.content_type.as_str())
                            .append_header((header::CACHE_CONTROL, "public, max-age=3600"))
                            .append_header((header::ETAG, etag))
                            .body(bytes)
                    },
                    Err(err) => {
                        // Log the error
                        error!("Failed to read icon content: {}", err);
                        
                        // Also send to Sentry if enabled
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
                
                // Log the error
                warn!("Icon not found. Status: {}", status);
                
                // Also send to Sentry if enabled
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
            // Log the error
            error!("Failed to fetch icon: {}", err);
            
            // Also send to Sentry if enabled
            if env::var("SENTRY_DSN").is_ok() {
                sentry::capture_message(
                    &format!("Failed to fetch icon: {}", err),
                    sentry::Level::Error
                );
            }
            
            // Determine appropriate status code based on error type
            if err.is_timeout() {
                warn!("Request timed out while fetching icon: {}", err);
                HttpResponse::GatewayTimeout()
                    .body(format!("Request timed out while fetching icon: {}", err))
            } else if err.is_connect() {
                warn!("Connection error while fetching icon: {}", err);
                HttpResponse::BadGateway()
                    .body(format!("Connection error while fetching icon: {}", err))
            } else {
                error!("Failed to fetch icon: {}", err);
                HttpResponse::InternalServerError()
                    .body(format!("Failed to fetch icon: {}", err))
            }
        }
    }
}

/// Health check endpoint
#[get("/health")]
    pub async fn health_check(cache: web::Data<Arc<IconCache>>) -> HttpResponse {
        debug!("Health check requested");
        
        // Get cache statistics for monitoring
        let (main_count, expired_count, negative_count) = cache.stats().await;
        
        HttpResponse::Ok()
            .content_type("application/json")
            .body(format!(
                r#"{{
                    "status":"ok",
                    "service":"geticon",
                    "cache_stats":{{
                        "main_cache":{},
                        "expired_cache":{},
                        "negative_cache":{}
                    }}
                }}"#,
                main_count, expired_count, negative_count
            ))
    }

/// Handler for /json endpoint - returns favicon information as JSON
#[get("/json")]
pub async fn get_favicon_json(
    url: web::Query<std::collections::HashMap<String, String>>,
    req: HttpRequest,
    client: web::Data<reqwest::Client>,
    cache: web::Data<Arc<IconCache>>
) -> HttpResponse {
    debug!("JSON favicon request received");
    
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
    if let Some((cached_entry, needs_refresh)) = cache.get(&cache_key).await {
        // For JSON endpoint, we'll use the same approach as for images
        // If from expired cache, return a shorter TTL
        let max_age = if needs_refresh { "600" } else { "3600" };
        
        // Return the cached JSON response
        return HttpResponse::Ok()
            .content_type(cached_entry.content_type.as_str())
            .append_header((header::CACHE_CONTROL, format!("public, max-age={}", max_age)))
            .append_header((header::ETAG, cached_entry.etag.clone()))
            .body(cached_entry.content.clone());
    }
    
    // Extract headers to forward
    let forwarded_headers = extract_headers_to_forward(&req);
    
    // If not in cache, fetch icons from the website
    let icons = match get_page_icons(client.as_ref(), &normalized_url, Some(&forwarded_headers), None).await {
        icons if !icons.is_empty() => icons,
        _ => {
            warn!("Failed to find icons for URL: {}", normalized_url);
            return HttpResponse::NotFound().body("No icons found");
        }
    };
    
    // Select best icon based on requested size or highest score
    let _best_icon = find_best_icon_for_size(&icons, requested_size)
        .cloned();
    
    // Validate icons
    let final_icons = validate_icons(client.as_ref(), &icons, &forwarded_headers).await;
    
    // If no icons passed validation, return a 404
    if final_icons.is_empty() {
        warn!("No valid icons found for URL: {}", normalized_url);
        return HttpResponse::NotFound().body("No valid icons found");
    }
    
    // Recalculate the best icon based on the validated icons
    let best_icon = if !final_icons.is_empty() {
        find_best_icon_for_size(&final_icons, requested_size).cloned()
    } else {
        None
    };
    
    // Create response
    let response = IconResponse {
        url: normalized_url.host_str().unwrap_or(url_str).to_string(),
        icons: final_icons,
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
            // Log the error
            error!("Failed to serialize JSON response: {}", err);
            
            // Also send to Sentry if enabled
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
