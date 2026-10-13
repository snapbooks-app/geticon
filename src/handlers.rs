use actix_web::{get, web, HttpResponse, HttpRequest, http::{header, StatusCode}};
use md5;
use crate::url_utils::normalize_url;
use crate::models::{Icon, IconResponse};
use crate::favicon::{get_page_icons, find_best_icon_for_size, select_user_agent_for_icon};
use crate::validation::{validate_image_content, is_html_content, read_limited};
use crate::cache::{CacheEntry, CachedImage, Failure, IconCache};
use std::env;
use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;
use log::{warn, debug, error};
use url::Url;

/// Upper bound on icon discovery for one request, however many candidate URLs it probes
const DISCOVERY_DEADLINE: Duration = Duration::from_secs(15);

/// Largest icon we download and serve
const MAX_ICON_BYTES: usize = 2 * 1024 * 1024;

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

/// Logs why a lookup failed, reports it to Sentry if enabled, and returns it for caching
fn fail(status: StatusCode, message: &str, details: String) -> Failure {
    let level = if status.is_server_error() {
        error!("{}", details);
        sentry::Level::Error
    } else {
        warn!("{}", details);
        sentry::Level::Warning
    };
    if env::var("SENTRY_DSN").is_ok() {
        sentry::capture_message(&details, level);
    }
    Failure { status, message: message.to_string() }
}

/// The response every request for a failed lookup gets until it is retried
fn failure_response(failure: &Failure) -> HttpResponse {
    HttpResponse::build(failure.status).body(failure.message.clone())
}

/// Stale entries are being refreshed in the background, so clients should check back sooner
fn cache_control_for(is_stale: bool) -> &'static str {
    if is_stale { "public, max-age=600" } else { "public, max-age=7200" }
}

/// Finds and validates a page's icons, giving up after DISCOVERY_DEADLINE
async fn discover(
    client: reqwest::Client,
    url: Url,
    forwarded_headers: HashMap<String, String>,
) -> Result<Vec<Icon>, Failure> {
    let discovery = get_page_icons(&client, &url, Some(&forwarded_headers), None);
    match actix_web::rt::time::timeout(DISCOVERY_DEADLINE, discovery).await {
        Ok(icons) if !icons.is_empty() => Ok(icons),
        Ok(_) => Err(fail(
            StatusCode::NOT_FOUND,
            "No icons found",
            format!("Failed to find icons for URL: {}", url),
        )),
        Err(_) => Err(fail(
            StatusCode::GATEWAY_TIMEOUT,
            "Timed out looking for icons",
            format!("Icon discovery timed out after {}s for URL: {}", DISCOVERY_DEADLINE.as_secs(), url),
        )),
    }
}

/// A page's validated icons and whether they are stale; concurrent misses share one discovery
async fn load_icons(
    cache: &IconCache,
    client: &reqwest::Client,
    url: &Url,
    forwarded_headers: &HashMap<String, String>,
) -> (Arc<CacheEntry<Vec<Icon>>>, bool) {
    let (client, page_url, headers) = (client.clone(), url.clone(), forwarded_headers.clone());
    cache.discovery.get_or_load(url.as_str(), move || discover(client, page_url, headers)).await
}

/// Downloads an icon and checks that it really is an image
async fn fetch_image(
    client: reqwest::Client,
    icon: Icon,
    forwarded_headers: HashMap<String, String>,
    page_url: Url,
) -> Result<CachedImage, Failure> {
    // Override the User-Agent with our selected one based on icon type
    let mut headers = forwarded_headers;
    headers.insert("User-Agent".to_string(), select_user_agent_for_icon(&icon).to_string());

    let mut request_builder = client.get(&icon.url);

    // Apply headers
    for (name, value) in &headers {
        request_builder = request_builder.header(name, value);
    }

    let response = match request_builder.send().await {
        Ok(response) => response,
        Err(err) => {
            let (status, message) = if err.is_timeout() {
                (StatusCode::GATEWAY_TIMEOUT, format!("Request timed out while fetching icon: {}", err))
            } else if err.is_connect() {
                (StatusCode::BAD_GATEWAY, format!("Connection error while fetching icon: {}", err))
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to fetch icon: {}", err))
            };
            return Err(fail(status, &message, format!("Failed to fetch icon: {}", err)));
        }
    };

    // Must be an image, including after a redirect (e.g. to a cookie consent page)
    if let Some(content_type) = response.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
        if !content_type.starts_with("image/") {
            let final_url = response.url().as_str();
            return Err(if final_url != icon.url {
                fail(
                    StatusCode::NOT_FOUND,
                    &format!("Icon redirected to non-image resource: {}", final_url),
                    format!("Icon redirected to non-image resource: {} -> {} (Content-Type: {})",
                        icon.url, final_url, content_type),
                )
            } else {
                fail(
                    StatusCode::NOT_FOUND,
                    &format!("Invalid content type for icon: {}", content_type),
                    format!("Invalid content type for icon: {} (Content-Type: {})", icon.url, content_type),
                )
            });
        }
    }

    let status = response.status();
    if !status.is_success() {
        return Err(fail(
            StatusCode::NOT_FOUND,
            &format!("Icon not found. Status: {}", status),
            format!("Icon not found. Status: {} for icon URL: {}", status, icon.url),
        ));
    }

    let bytes = match read_limited(response, MAX_ICON_BYTES).await {
        Ok((bytes, false)) => bytes,
        Ok((_, true)) => return Err(fail(
            StatusCode::NOT_FOUND,
            "Icon is too large",
            format!("Icon over {} bytes for URL: {} from icon URL: {}", MAX_ICON_BYTES, page_url, icon.url),
        )),
        Err(err) => return Err(fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to read icon content",
            format!("Failed to read icon content: {}", err),
        )),
    };

    if bytes.is_empty() {
        return Err(fail(
            StatusCode::NOT_FOUND,
            "Icon found but has zero size",
            format!("Zero-size icon detected for URL: {} from icon URL: {}", page_url, icon.url),
        ));
    }

    // Check for HTML content disguised as an image
    if is_html_content(&bytes) {
        return Err(fail(
            StatusCode::NOT_FOUND,
            "Icon found but content is HTML, not an image",
            format!("HTML content disguised as an image for URL: {} from icon URL: {}", page_url, icon.url),
        ));
    }

    if !validate_image_content(&bytes, &icon.content_type) {
        return Err(fail(
            StatusCode::NOT_FOUND,
            "Icon found but content is not a valid image",
            format!("Invalid image content for URL: {} from icon URL: {}", page_url, icon.url),
        ));
    }

    let etag = format!("\"{:x}\"", md5::compute(&bytes));
    Ok(CachedImage { content: bytes, content_type: icon.content_type, etag })
}

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
    
    // Extract headers to forward
    let forwarded_headers = extract_headers_to_forward(&req);

    let (discovery, discovery_stale) = load_icons(&cache, &client, &normalized_url, &forwarded_headers).await;
    let icons = match &discovery.value {
        Ok(icons) => icons,
        Err(failure) => return failure_response(failure),
    };

    // Select the best icon based on requested size or highest score
    let best_icon = match find_best_icon_for_size(icons, requested_size) {
        Some(icon) => icon.clone(),
        None => return HttpResponse::NotFound().body("No suitable icon found"),
    };
    
    // Images are cached per icon URL, so sizes that pick the same icon share one download
    let icon_url = best_icon.url.clone();
    let (client, page_url) = (client.get_ref().clone(), normalized_url.clone());
    let (image_entry, image_stale) = cache.images
        .get_or_load(&icon_url, move || fetch_image(client, best_icon, forwarded_headers, page_url))
        .await;
    let image = match &image_entry.value {
        Ok(image) => image,
        Err(failure) => return failure_response(failure),
    };
    let cache_control = cache_control_for(discovery_stale || image_stale);
    
    // Check if the client has the same version (ETag)
    if let Some(if_none_match) = req.headers().get(header::IF_NONE_MATCH) {
        if if_none_match.to_str().unwrap_or("") == image.etag {
            debug!("Client already has latest version (ETag match): {}", icon_url);
            return HttpResponse::NotModified()
                .append_header((header::CACHE_CONTROL, cache_control))
                .finish();
        }
    }

    HttpResponse::Ok()
        .content_type(image.content_type.as_str())
        .append_header((header::CACHE_CONTROL, cache_control))
        .append_header((header::ETAG, image.etag.clone()))
        .body(image.content.clone())
}

/// Health check endpoint
#[get("/health")]
    pub async fn health_check(cache: web::Data<Arc<IconCache>>) -> HttpResponse {
        debug!("Health check requested");
        
        // Get cache statistics for monitoring
        HttpResponse::Ok()
            .content_type("application/json")
            .body(format!(
                r#"{{
                    "status":"ok",
                    "service":"geticon",
                    "cache_stats":{{
                        "discovery_cache":{},
                        "image_cache":{}
                    }}
                }}"#,
                cache.discovery.entry_count(), cache.images.entry_count()
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
    
    // Extract headers to forward
    let forwarded_headers = extract_headers_to_forward(&req);

    // Shares the cached discovery with /img, whatever the size
    let (discovery, is_stale) = load_icons(&cache, &client, &normalized_url, &forwarded_headers).await;
    let icons = match &discovery.value {
        Ok(icons) => icons,
        Err(failure) => return failure_response(failure),
    };

    // Select best icon based on requested size or highest score
    let best_icon = find_best_icon_for_size(icons, requested_size).cloned();

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
            
            HttpResponse::Ok()
                .content_type("application/json")
                .append_header((header::CACHE_CONTROL, cache_control_for(is_stale)))
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
