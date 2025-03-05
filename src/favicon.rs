use reqwest;
use scraper::{Html, Selector};
use std::collections::{HashSet, HashMap};
use url::Url;
use crate::models::Icon;

/// Gets all available icons from a webpage with enhanced detection
pub async fn get_page_icons(
    client: &reqwest::Client, 
    url: &Url,
    forwarded_headers: Option<&HashMap<String, String>>
) -> Vec<Icon> {
    let mut icons = HashSet::new();
    
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
    let mut request_builder = client.get(url.as_str());
    
    // Apply forwarded headers if provided
    if let Some(headers) = forwarded_headers {
        for (name, value) in headers {
            request_builder = request_builder.header(name, value);
        }
    }
    
    if let Ok(response) = request_builder.send().await {
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
    for manifest_url in manifest_urls {
        let mut manifest_req = client.get(manifest_url.as_str());
        
        // Apply forwarded headers to manifest requests too
        if let Some(headers) = forwarded_headers {
            for (name, value) in headers {
                manifest_req = manifest_req.header(name, value);
            }
        }
        
        if let Ok(manifest_response) = manifest_req.send().await {
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
    
    // Calculate scores and sort icons
    let mut icon_vec: Vec<Icon> = icons.into_iter().collect();
    for icon in &mut icon_vec {
        icon.calculate_score();
    }
    
    // Sort by score (highest first)
    icon_vec.sort_by(|a, b| b.score.cmp(&a.score));
    
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
