use reqwest;
use scraper::{Html, Selector};
use std::collections::{HashSet, HashMap};
use url::Url;
use crate::models::Icon;
use crate::validation;
use std::time::Duration;
use log::{info, warn, debug, error, trace};

/// Selects an appropriate User-Agent string based on icon type
/// User-Agents sourced from https://www.useragents.me (last updated: March 2025)
pub fn select_user_agent_for_icon(icon: &Icon) -> &'static str {
    // Check for Apple icons
    if icon.url.contains("apple-touch-icon") || 
       (icon.purpose.as_ref().map_or(false, |p| p.contains("apple-touch-icon"))) {
        // iOS/Safari User-Agent
        "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1.1 Mobile/15E148 Safari/604.1"
    } 
    // Check for Android/maskable icons
    else if icon.purpose.as_ref().map_or(false, |p| p.contains("maskable")) {
        // Android/Chrome User-Agent
        "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Mobile Safari/537.36"
    }
    // Check for Microsoft icons
    else if icon.purpose.as_ref().map_or(false, |p| p.contains("msapplication")) {
        // Windows/Chrome User-Agent
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36"
    }
    // Default to Windows/Chrome for other icons
    else {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36"
    }
}

// Use the validation functions from the validation module
use crate::validation::validate_icon;

/// Try additional common icon locations that might not be explicitly referenced
async fn try_additional_icon_sources(
    client: &reqwest::Client,
    url: &Url,
    forwarded_headers: Option<&HashMap<String, String>>
) -> Vec<Icon> {
    debug!("Trying additional icon sources for URL: {}", url);
    let mut additional_icons = Vec::new();
    
    // Common icon paths to try
    let common_paths = [
        // Root favicon variations
        "/favicon.png",
        "/favicon-32x32.png",
        "/favicon-16x16.png",
        "/favicon-96x96.png",
        "/favicon-128.png",
        "/favicon-196x196.png",
        
        // Apple icon variations
        "/apple-icon.png",
        "/apple-icon-57x57.png",
        "/apple-icon-60x60.png",
        "/apple-icon-72x72.png",
        "/apple-icon-76x76.png",
        "/apple-icon-114x114.png",
        "/apple-icon-120x120.png",
        "/apple-icon-144x144.png",
        "/apple-icon-152x152.png",
        "/apple-icon-180x180.png",
        
        // Android icon variations
        "/android-icon-192x192.png",
        "/android-chrome-192x192.png",
        "/android-chrome-512x512.png",
        
        // Microsoft icon variations
        "/mstile-70x70.png",
        "/mstile-144x144.png",
        "/mstile-150x150.png",
        "/mstile-310x150.png",
        "/mstile-310x310.png",
    ];
    
    for path in &common_paths {
        if let Ok(icon_url) = url.join(path) {
            let icon_str = icon_url.to_string();
            
            // Skip if we've already tried this URL
            if additional_icons.iter().any(|i: &Icon| i.url == icon_str) {
                continue;
            }
            
            debug!("Trying additional icon path: {}", icon_str);
            
            // Determine content type and size from path
            let (content_type, width, height) = if path.ends_with(".png") {
                let size = path.split('-').last()
                    .and_then(|s| s.split('.').next())
                    .and_then(|s| s.split('x').next())
                    .and_then(|s| s.parse::<u32>().ok());
                
                ("image/png".to_string(), size, size)
            } else if path.ends_with(".ico") {
                ("image/x-icon".to_string(), Some(16), Some(16))
            } else if path.ends_with(".svg") {
                ("image/svg+xml".to_string(), None, None)
            } else {
                ("image/png".to_string(), None, None)
            };
            
            // Create icon and validate it
            let icon = Icon::new(
                icon_str,
                content_type,
                width,
                height,
            );
            
            if validate_icon(client, &icon, forwarded_headers).await {
                additional_icons.push(icon);
            }
        }
    }
    
    additional_icons
}

/// Gets all available icons from a webpage with enhanced detection and validation
pub async fn get_page_icons(
    client: &reqwest::Client, 
    url: &Url,
    forwarded_headers: Option<&HashMap<String, String>>
) -> Vec<Icon> {
    info!("Fetching icons for URL: {}", url);
    let mut icons = HashSet::new();
    let mut validated_icons: Vec<Icon> = Vec::new();
    
    // Try direct favicon.ico
    let favicon_url = url.join("/favicon.ico").ok();
    if let Some(favicon_url) = favicon_url {
        icons.insert(Icon::new(
            favicon_url.to_string(),
            "image/x-icon".to_string(),
            Some(16),
            Some(16),
        ));
    }
    
    // Try apple-touch-icon.png and apple-touch-icon-precomposed.png
    for apple_icon in &["/apple-touch-icon.png", "/apple-touch-icon-precomposed.png"] {
        if let Ok(apple_url) = url.join(apple_icon) {
            icons.insert(Icon::new(
                apple_url.to_string(),
                "image/png".to_string(),
                Some(180),
                Some(180),
            ).with_purpose(Some("apple-touch-icon".to_string())));
        }
    }
    
    let mut manifest_urls = Vec::new();
    
    // Try fetching HTML and parsing link tags
    // Create a copy of forwarded headers that we can modify
    let mut headers = match forwarded_headers {
        Some(h) => h.clone(),
        None => HashMap::new(),
    };
    
    // Use a default desktop User-Agent for the initial HTML request
    headers.insert("User-Agent".to_string(), 
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36".to_string());
    
    debug!("Fetching HTML from URL: {}", url);
    let mut request_builder = client.get(url.as_str());
    
    // Apply headers
    for (name, value) in &headers {
        request_builder = request_builder.header(name, value);
    }
    
    if let Ok(response) = request_builder.send().await {
        debug!("Successfully fetched HTML from URL: {}, status: {}", url, response.status());
        if let Ok(text) = response.text().await {
            let document = Html::parse_document(&text);
            
            // Look for all icon-related link tags
            let selector = Selector::parse("link[rel~='icon'], link[rel~='shortcut icon'], link[rel~='apple-touch-icon'], link[rel~='apple-touch-icon-precomposed'], link[rel~='mask-icon'], meta[name='msapplication-TileImage']").unwrap();
            
            for element in document.select(&selector) {
                let tag_name = element.value().name();
                
                if tag_name == "link" {
                    if let Some(href) = element.value().attr("href") {
                        if let Ok(icon_url) = url.join(href) {
                            let mut content_type = element.value().attr("type")
                                .unwrap_or("image/x-icon")
                                .to_string();
                                
                            // Infer type from extension if not specified
                            if content_type == "image/x-icon" {
                                if href.ends_with(".png") {
                                    content_type = "image/png".to_string();
                                } else if href.ends_with(".svg") {
                                    content_type = "image/svg+xml".to_string();
                                } else if href.ends_with(".webp") {
                                    content_type = "image/webp".to_string();
                                } else if href.ends_with(".jpg") || href.ends_with(".jpeg") {
                                    content_type = "image/jpeg".to_string();
                                }
                            }
                            
                            // Parse sizes attribute
                            let (width, height) = element.value().attr("sizes")
                                .and_then(|sizes| {
                                    let parts: Vec<&str> = sizes.split('x').collect();
                                    if parts.len() == 2 {
                                        let w = parts[0].parse().ok()?;
                                        let h = parts[1].parse().ok()?;
                                        Some((Some(w), Some(h)))
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or((None, None));
                            
                            // Get purpose from rel attribute
                            let purpose = if let Some(rel) = element.value().attr("rel") {
                                Some(rel.to_string())
                            } else {
                                None
                            };
                                
                            icons.insert(Icon::new(
                                icon_url.to_string(),
                                content_type,
                                width,
                                height,
                            ).with_purpose(purpose));
                        }
                    }
                } else if tag_name == "meta" && element.value().attr("name") == Some("msapplication-TileImage") {
                    // Handle Windows tile image
                    if let Some(content) = element.value().attr("content") {
                        if let Ok(icon_url) = url.join(content) {
                            icons.insert(Icon::new(
                                icon_url.to_string(),
                                "image/png".to_string(),
                                Some(144),
                                Some(144),
                            ).with_purpose(Some("msapplication-TileImage".to_string())));
                        }
                    }
                }
            }
            
            // Look for web app manifest
            let manifest_selector = Selector::parse("link[rel='manifest']").unwrap();
            for element in document.select(&manifest_selector) {
                if let Some(href) = element.value().attr("href") {
                    if let Ok(manifest_url) = url.join(href) {
                        manifest_urls.push(manifest_url);
                    }
                }
            }
            
            // Look for browserconfig.xml
            let browserconfig_selector = Selector::parse("meta[name='msapplication-config']").unwrap();
            for element in document.select(&browserconfig_selector) {
                if let Some(content) = element.value().attr("content") {
                    if let Ok(config_url) = url.join(content) {
                        // Try to fetch browserconfig.xml
                        if let Ok(config_response) = client.get(config_url).send().await {
                            if let Ok(config_text) = config_response.text().await {
                                // Very basic parsing of browserconfig.xml
                                if let Some(tile_image) = config_text.lines()
                                    .find(|line| line.contains("<square"))
                                    .and_then(|line| {
                                        let start = line.find("src=\"")?;
                                        let end = line[start + 5..].find("\"")?;
                                        Some(&line[start + 5..start + 5 + end])
                                    }) {
                                    if let Ok(icon_url) = url.join(tile_image) {
                                        icons.insert(Icon::new(
                                            icon_url.to_string(),
                                            "image/png".to_string(),
                                            Some(144),
                                            Some(144),
                                        ).with_purpose(Some("msapplication-tile".to_string())));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            
            // Look for Open Graph image as fallback
            let og_selector = Selector::parse("meta[property='og:image']").unwrap();
            for element in document.select(&og_selector) {
                if let Some(content) = element.value().attr("content") {
                    if let Ok(og_url) = url.join(content) {
                        icons.insert(Icon::new(
                            og_url.to_string(),
                            "image/jpeg".to_string(), // Assume JPEG, will be corrected if needed
                            None,
                            None,
                        ).with_purpose(Some("og:image".to_string())));
                    }
                }
            }
        }
    }
    
    // Try default manifest.json location if none found in HTML
    if manifest_urls.is_empty() {
        if let Ok(default_manifest) = url.join("/manifest.json") {
            manifest_urls.push(default_manifest);
        }
        if let Ok(default_manifest) = url.join("/site.webmanifest") {
            manifest_urls.push(default_manifest);
        }
    }
    
    // Process manifest files
    for manifest_url in &manifest_urls {
        debug!("Fetching web app manifest from URL: {}", manifest_url);
        
        // Create a copy of forwarded headers that we can modify
        let mut manifest_headers = match forwarded_headers {
            Some(h) => h.clone(),
            None => HashMap::new(),
        };
        
        // Use a Chrome/Android User-Agent for manifest requests as they're often used for PWAs
        manifest_headers.insert("User-Agent".to_string(), 
            "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Mobile Safari/537.36".to_string());
        
        let mut manifest_req = client.get(manifest_url.as_str());
        
        // Apply headers
        for (name, value) in &manifest_headers {
            manifest_req = manifest_req.header(name, value);
        }
        
        if let Ok(manifest_response) = manifest_req.send().await {
            debug!("Successfully fetched manifest from URL: {}, status: {}", manifest_url, manifest_response.status());
            if let Ok(manifest_text) = manifest_response.text().await {
                if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&manifest_text) {
                    if let Some(manifest_icons) = manifest.get("icons").and_then(|i| i.as_array()) {
                        for icon in manifest_icons {
                            if let (Some(src), Some(sizes)) = (
                                icon.get("src").and_then(|s| s.as_str()),
                                icon.get("sizes").and_then(|s| s.as_str()),
                            ) {
                                if let Ok(icon_url) = manifest_url.join(src) {
                                    // Parse size from manifest
                                    let (width, height) = if sizes.contains('x') {
                                        let parts: Vec<&str> = sizes.split('x').collect();
                                        if parts.len() == 2 {
                                            (parts[0].parse().ok(), parts[1].parse().ok())
                                        } else {
                                            (None, None)
                                        }
                                    } else if let Ok(size) = sizes.parse::<u32>() {
                                        // Some manifests use single number for square icons
                                        (Some(size), Some(size))
                                    } else {
                                        (None, None)
                                    };
                                    
                                    // Get content type from extension
                                    let content_type = if src.ends_with(".png") {
                                        "image/png".to_string()
                                    } else if src.ends_with(".svg") {
                                        "image/svg+xml".to_string()
                                    } else if src.ends_with(".webp") {
                                        "image/webp".to_string()
                                    } else if src.ends_with(".jpg") || src.ends_with(".jpeg") {
                                        "image/jpeg".to_string()
                                    } else {
                                        "image/png".to_string() // Default to PNG
                                    };
                                    
                                    // Get purpose if available
                                    let purpose = icon.get("purpose")
                                        .and_then(|p| p.as_str())
                                        .map(|p| p.to_string());
                                    
                                    icons.insert(Icon::new(
                                        icon_url.to_string(),
                                        content_type,
                                        width,
                                        height,
                                    ).with_purpose(purpose));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Validate all collected icons
    let mut icon_vec: Vec<Icon> = icons.into_iter().collect();
    
    // Calculate scores for all icons
    for icon in &mut icon_vec {
        icon.calculate_score();
    }
    
    // Sort by score (highest first)
    icon_vec.sort_by(|a, b| b.score.cmp(&a.score));
    
    // Validate the top icons (up to 5) to avoid excessive requests
    debug!("Validating top {} icons from URL: {}", icon_vec.len().min(5), url);
    for icon in icon_vec.iter().take(5) {
        debug!("Validating icon: {} (type: {}, size: {}x{})", 
            icon.url, 
            icon.content_type,
            icon.width.unwrap_or(0),
            icon.height.unwrap_or(0));
            
        if validate_icon(client, icon, forwarded_headers).await {
            debug!("Icon validated successfully: {}", icon.url);
            validated_icons.push(icon.clone());
        } else {
            debug!("Icon validation failed: {}", icon.url);
        }
    }
    
    // If we found valid icons, return them
    if !validated_icons.is_empty() {
        // Sort validated icons by score
        validated_icons.sort_by(|a, b| b.score.cmp(&a.score));
        info!("Found {} valid icons for URL: {}", validated_icons.len(), url);
        debug!("Best icon: {} (type: {}, size: {}x{})", 
            validated_icons[0].url, 
            validated_icons[0].content_type,
            validated_icons[0].width.unwrap_or(0),
            validated_icons[0].height.unwrap_or(0));
        return validated_icons;
    }
    
    // If no valid icons found, try additional sources
    debug!("No valid icons found in primary sources, trying additional sources for URL: {}", url);
    let additional_icons = try_additional_icon_sources(client, url, forwarded_headers).await;
    if !additional_icons.is_empty() {
        let mut result = additional_icons;
        // Calculate scores for additional icons
        for icon in &mut result {
            icon.calculate_score();
        }
        // Sort by score
        result.sort_by(|a, b| b.score.cmp(&a.score));
        info!("Found {} valid icons from additional sources for URL: {}", result.len(), url);
        debug!("Best additional icon: {} (type: {}, size: {}x{})", 
            result[0].url, 
            result[0].content_type,
            result[0].width.unwrap_or(0),
            result[0].height.unwrap_or(0));
        return result;
    }
    
    // If still no icons found, return the original list (which might have invalid icons)
    // This allows the handler to attempt to fetch them anyway as a last resort
    warn!("No valid icons found for URL: {}, returning unvalidated icons as last resort", url);
    icon_vec
}

/// Finds the best icon for a specific size requirement
pub fn find_best_icon_for_size(icons: &[Icon], requested_size: Option<u32>) -> Option<&Icon> {
    if icons.is_empty() {
        return None;
    }
    
    if let Some(size) = requested_size {
        // Find icon closest to requested size
        icons.iter()
            .filter(|icon| icon.width.is_some() && icon.height.is_some())
            .min_by_key(|icon| {
                let icon_size = icon.width.unwrap_or(0).max(icon.height.unwrap_or(0));
                if icon_size >= size {
                    icon_size - size // Prefer slightly larger than smaller
                } else {
                    size - icon_size
                }
            })
            .or(Some(&icons[0])) // Fallback to highest scored icon
    } else {
        // Use highest scored icon
        Some(&icons[0])
    }
}
