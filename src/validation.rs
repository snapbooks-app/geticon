use bytes::Bytes;
use reqwest;
use std::collections::HashMap;
use crate::models::Icon;
use std::time::Duration;
use log::{info, warn, debug, error, trace};

/// Checks if a content type header indicates an image
pub fn is_image_content_type(content_type: &str) -> bool {
    content_type.starts_with("image/")
}

/// Checks if content bytes represent HTML rather than an image
pub fn is_html_content(bytes: &[u8]) -> bool {
    bytes.starts_with(b"<!DOCTYPE") || 
    bytes.starts_with(b"<html") || 
    bytes.starts_with(b"<HTML") ||
    bytes.windows(7).any(|window| window == b"<script") ||
    bytes.windows(5).any(|window| window == b"<body") ||
    bytes.windows(5).any(|window| window == b"<head")
}

/// Checks if content bytes represent a valid image based on file signatures
pub fn has_valid_image_signature(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG") || // PNG
    bytes.starts_with(b"GIF8") || // GIF
    bytes.starts_with(b"\xFF\xD8\xFF") || // JPEG
    bytes.starts_with(b"<svg") || // SVG
    bytes.starts_with(b"<?xml") || // XML (possibly SVG)
    bytes.starts_with(b"RIFF") || // WEBP
    bytes.starts_with(b"\x00\x00\x01\x00") // ICO
}

/// Validates an icon by making a HEAD request to check if it exists and has content
pub async fn validate_icon(
    client: &reqwest::Client, 
    icon: &Icon, 
    forwarded_headers: Option<&HashMap<String, String>>
) -> bool {
    debug!("Validating icon: {}", icon.url);
    
    // Create a copy of forwarded headers that we can modify
    let mut headers = match forwarded_headers {
        Some(h) => h.clone(),
        None => HashMap::new(),
    };
    
    // Override the User-Agent with our selected one based on icon type
    let user_agent = crate::favicon::select_user_agent_for_icon(icon);
    headers.insert("User-Agent".to_string(), user_agent.to_string());
    
    let mut request_builder = client.head(&icon.url)
        .timeout(Duration::from_secs(5));
    
    // Apply headers
    for (name, value) in &headers {
        request_builder = request_builder.header(name, value);
    }
    
    match request_builder.send().await {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                debug!("Icon validation failed - HTTP status: {} for URL: {}", status, icon.url);
                return false;
            }
            
            // Check if the response was redirected to a different URL
            let final_url = response.url().to_string();
            if final_url != icon.url {
                debug!("Icon was redirected: {} -> {}", icon.url, final_url);
                
                // The request was redirected, check if the final URL is still an image
                if let Some(content_type) = response.headers().get("content-type") {
                    if let Ok(content_type_str) = content_type.to_str() {
                        // If redirected to a non-image resource (like HTML), reject it
                        if !is_image_content_type(content_type_str) {
                            debug!("Icon validation failed - Redirected to non-image content type: {}", content_type_str);
                            return false;
                        }
                    }
                }
                
                // For redirects, do a small GET request to peek at the content
                // This helps detect cookie consent pages and other non-image content
                debug!("Peeking at content for redirected URL: {}", final_url);
                if !peek_content_is_valid_image(client, &final_url, &headers).await {
                    debug!("Icon validation failed - Peeked content is not a valid image");
                    return false;
                }
            }
            
            // Check content type header to ensure it's an image
            if let Some(content_type) = response.headers().get("content-type") {
                if let Ok(content_type_str) = content_type.to_str() {
                    if !is_image_content_type(content_type_str) {
                        debug!("Icon validation failed - Non-image content type: {}", content_type_str);
                        return false;
                    }
                    debug!("Icon content type: {}", content_type_str);
                }
            }
            
            // Check content length if available
            if let Some(length) = response.headers().get("content-length") {
                if let Ok(size) = length.to_str().unwrap_or("0").parse::<u64>() {
                    if size == 0 {
                        debug!("Icon validation failed - Zero content length");
                        return false;
                    }
                    debug!("Icon content length: {} bytes", size);
                }
            }
            
            // If no content-length header, assume it's valid if we've passed other checks
            debug!("Icon validation successful: {}", icon.url);
            true
        },
        Err(err) => {
            debug!("Icon validation failed - Request error: {} for URL: {}", err, icon.url);
            false
        }
    }
}

/// Helper function to peek at content and validate it's an image
async fn peek_content_is_valid_image(
    client: &reqwest::Client,
    url: &str,
    headers: &HashMap<String, String>
) -> bool {
    debug!("Peeking at content for URL: {}", url);
    
    let mut peek_request = client.get(url)
        .timeout(Duration::from_secs(5));
    
    // Apply headers
    for (name, value) in headers {
        peek_request = peek_request.header(name, value);
    }
    
    // Set range header to only get the first 512 bytes
    peek_request = peek_request.header("Range", "bytes=0-511");
    
    match peek_request.send().await {
        Ok(peek_response) => {
            let status = peek_response.status();
            debug!("Peek response status: {} for URL: {}", status, url);
            
            if let Ok(bytes) = peek_response.bytes().await {
                if bytes.is_empty() {
                    debug!("Peek content is empty for URL: {}", url);
                    return false;
                }
                
                // Check for HTML content
                if is_html_content(&bytes) {
                    debug!("Peek content is HTML, not an image for URL: {}", url);
                    return false;
                }
                
                // Check for common image signatures
                let is_valid = has_valid_image_signature(&bytes);
                if is_valid {
                    debug!("Peek content has valid image signature for URL: {}", url);
                } else {
                    debug!("Peek content does not have valid image signature for URL: {}", url);
                }
                return is_valid;
            } else {
                debug!("Failed to read peek content bytes for URL: {}", url);
            }
            false
        },
        Err(err) => {
            debug!("Peek request failed: {} for URL: {}", err, url);
            false
        }
    }
}

/// Validates a list of icons by checking if they exist and are valid images
/// Returns a list of validated icons
pub async fn validate_icons(
    client: &reqwest::Client,
    icons: &[Icon],
    forwarded_headers: &HashMap<String, String>
) -> Vec<Icon> {
    debug!("Validating {} icons", icons.len());
    let mut validated_icons = Vec::new();
    
    for icon in icons {
        debug!("Validating icon: {} (type: {}, size: {}x{})", 
            icon.url, 
            icon.content_type,
            icon.width.unwrap_or(0),
            icon.height.unwrap_or(0));
            
        if validate_icon(client, icon, Some(forwarded_headers)).await {
            debug!("Icon validated successfully: {}", icon.url);
            validated_icons.push(icon.clone());
        } else {
            debug!("Icon validation failed: {}", icon.url);
        }
    }
    
    info!("Validated {}/{} icons successfully", validated_icons.len(), icons.len());
    validated_icons
}

/// Validates image content by checking file signatures and using the image crate
pub fn validate_image_content(bytes: &Bytes, content_type: &str) -> bool {
    debug!("Validating image content of type: {}, size: {} bytes", content_type, bytes.len());
    
    // Check for empty content
    if bytes.is_empty() {
        debug!("Image validation failed - Empty content");
        return false;
    }
    
    // Check for HTML content disguised as an image
    if is_html_content(bytes) {
        debug!("Image validation failed - Content is HTML, not an image");
        return false;
    }
    
    // First check file signatures
    if !has_valid_image_signature(bytes) {
        debug!("Image validation failed - Invalid image signature for content type: {}", content_type);
        return false;
    }
    
    // Then use the image crate for deeper validation
    let result = match content_type {
        "image/svg+xml" => {
            debug!("SVG validation passed (signature check only)");
            true // SVG validation is already done by signature check
        },
        "image/png" => {
            // Special handling for PNG files
            match image::load_from_memory(bytes) {
                Ok(_) => {
                    debug!("PNG validation passed");
                    true
                },
                Err(err) => {
                    // Log detailed error for PNG validation failures
                    debug!("PNG validation failed - Error: {:?}", err);
                    
                    // Check if the PNG signature is valid but the image crate can't parse it
                    // This is a fallback to allow serving PNGs that have valid signatures
                    // but might have features the image crate doesn't support
                    if bytes.starts_with(b"\x89PNG") {
                        debug!("PNG has valid signature but failed image crate validation - allowing as fallback");
                        true
                    } else {
                        false
                    }
                }
            }
        },
        _ => {
            let load_result = image::load_from_memory(bytes).is_ok();
            if load_result {
                debug!("Image validation passed for content type: {}", content_type);
            } else {
                debug!("Image validation failed - Could not load image of type: {}", content_type);
            }
            load_result
        }
    };
    
    result
}
