use reqwest;
use scraper::{Html, Selector};
use std::collections::{HashSet, HashMap};
use url::Url;
use crate::models::Icon;
use crate::validation;
// Duration not used, removing it
use log::{info, warn, debug}; // Remove unused imports
use futures::future::join_all;

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

/// Most of a page we read; the <head> almost always fits, and longer pages are cut off
const MAX_PAGE_BYTES: usize = 1024 * 1024;
/// Largest web app manifest or browserconfig.xml we accept
const MAX_MANIFEST_BYTES: usize = 256 * 1024;

/// The part of a page up to its </head>, which is where every tag we look for lives
pub fn html_head(page: &str) -> &str {
    match page.as_bytes().windows(7).position(|window| window.eq_ignore_ascii_case(b"</head>")) {
        Some(end) => &page[..end + 7],
        None => page,
    }
}

/// An icon URL a page, manifest or browserconfig.xml declares, resolved against that document.
/// Like browsers, an http:// icon on an https:// document is fetched over https.
pub fn resolve_icon_url(document_url: &Url, href: &str) -> Option<Url> {
    let mut icon_url = document_url.join(href).ok()?;
    if document_url.scheme() == "https" && icon_url.scheme() == "http" {
        icon_url.set_scheme("https").ok()?;
    }
    Some(icon_url)
}

/// Try additional common icon locations that might not be explicitly referenced
async fn try_additional_icon_sources(
    client: &reqwest::Client,
    url: &Url,
    forwarded_headers: Option<&HashMap<String, String>>
) -> Vec<Icon> {
    debug!("Trying additional icon sources in parallel for URL: {}", url);
    
    // Common icon paths to try, most useful first so the search can stop early
    let common_paths = [
        "/apple-icon-180x180.png",
        "/android-chrome-512x512.png",
        "/android-chrome-192x192.png",
        "/android-icon-192x192.png",
        "/favicon-196x196.png",
        "/apple-icon.png",
        "/apple-icon-152x152.png",
        "/apple-icon-144x144.png",
        "/mstile-310x310.png",
        "/mstile-150x150.png",
        "/mstile-144x144.png",
        "/apple-icon-120x120.png",
        "/apple-icon-114x114.png",
        "/favicon-128.png",
        "/favicon-96x96.png",
        "/favicon.png",
        "/favicon-32x32.png",
    ];
    
    // Convert paths to possible icon objects
    let mut potential_icons = Vec::new();
    for path in &common_paths {
        if let Ok(icon_url) = url.join(path) {
            let icon_str = icon_url.to_string();
            
            // Skip duplicate URLs
            if potential_icons.iter().any(|i: &Icon| i.url == icon_str) {
                continue;
            }
            
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
            
            // Create icon and add to potential icons
            let icon = Icon::new(
                icon_str,
                content_type,
                width,
                height,
            );
            
            potential_icons.push(icon);
        }
    }
    
    // Probe a batch at a time and stop at the first batch that finds anything
    let headers = forwarded_headers.cloned().unwrap_or_default();
    for batch in potential_icons.chunks(5) {
        let found = validation::validate_icons(client, batch, &headers).await;
        if !found.is_empty() {
            debug!("Found {} valid additional icons", found.len());
            return found;
        }
    }
    Vec::new()
}

/// Icons listed in a web app manifest
async fn fetch_manifest_icons(
    client: &reqwest::Client,
    manifest_url: &Url,
    forwarded_headers: Option<&HashMap<String, String>>,
) -> Vec<Icon> {
    let mut found = Vec::new();
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
        if let Ok((manifest_body, false)) = validation::read_limited(manifest_response, MAX_MANIFEST_BYTES).await {
            if let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&manifest_body) {
                if let Some(manifest_icons) = manifest.get("icons").and_then(|i| i.as_array()) {
                    for icon in manifest_icons {
                        if let (Some(src), Some(sizes)) = (
                            icon.get("src").and_then(|s| s.as_str()),
                            icon.get("sizes").and_then(|s| s.as_str()),
                        ) {
                            if let Some(icon_url) = resolve_icon_url(manifest_url, src) {
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

                                // Get content type from extension, ignoring any query string
                                let path = icon_url.path();
                                let content_type = if path.ends_with(".png") {
                                    "image/png".to_string()
                                } else if path.ends_with(".svg") {
                                    "image/svg+xml".to_string()
                                } else if path.ends_with(".webp") {
                                    "image/webp".to_string()
                                } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
                                    "image/jpeg".to_string()
                                } else {
                                    "image/png".to_string() // Default to PNG
                                };

                                // Get purpose if available
                                let purpose = icon.get("purpose")
                                    .and_then(|p| p.as_str())
                                    .map(|p| p.to_string());

                                found.push(Icon::new(
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
    found
}

/// The tile image a browserconfig.xml names, resolved against the page URL
async fn fetch_browserconfig_icon(client: &reqwest::Client, config_url: &Url, page_url: &Url) -> Option<Icon> {
    let config_response = client.get(config_url.as_str()).send().await.ok()?;
    let (config_body, _) = validation::read_limited(config_response, MAX_MANIFEST_BYTES).await.ok()?;
    let config_text = String::from_utf8_lossy(&config_body);
    // Very basic parsing of browserconfig.xml
    let tile_image = config_text.lines()
        .find(|line| line.contains("<square"))
        .and_then(|line| {
            let start = line.find("src=\"")?;
            let end = line[start + 5..].find("\"")?;
            Some(&line[start + 5..start + 5 + end])
        })?;
    // The src is XML, so a query string's & arrives as &amp;
    let icon_url = resolve_icon_url(page_url, &tile_image.replace("&amp;", "&"))?;
    Some(Icon::new(
        icon_url.to_string(),
        "image/png".to_string(),
        Some(144),
        Some(144),
    ).with_purpose(Some("msapplication-tile".to_string())))
}

/// Gets all available icons from a webpage with enhanced detection and validation
/// Returns only icons that passed validation, best first (empty if none did)
pub async fn get_page_icons(
    client: &reqwest::Client, 
    url: &Url,
    forwarded_headers: Option<&HashMap<String, String>>,
    redirect_count: Option<u8> // New parameter for tracking redirects
) -> Vec<Icon> {
    const MAX_REDIRECTS: u8 = 5;
    
    // Use a mutable URL that we can update if we encounter redirects
    let mut current_url = url.clone();
    let mut current_redirect_count = redirect_count.unwrap_or(0);
    
    // Loop to handle meta refresh redirects
    loop {
        // Check if we've exceeded the redirect limit
        if current_redirect_count > MAX_REDIRECTS {
            warn!("Too many meta refresh redirects for URL: {}", current_url);
            return Vec::new();
        }
        
        info!("Fetching icons for URL: {} (redirect count: {})", current_url, current_redirect_count);
        let mut icons = HashSet::new();
        let mut validated_icons: Vec<Icon> = Vec::new();
        
        // Try direct favicon.ico
        let favicon_url = current_url.join("/favicon.ico").ok();
        if let Some(favicon_url) = favicon_url {
            icons.insert(Icon::new(
                favicon_url.to_string(),
                "image/x-icon".to_string(),
                Some(16),
                Some(16),
            ).speculative());
        }
        
        // Try apple-touch-icon.png and apple-touch-icon-precomposed.png
        for apple_icon in &["/apple-touch-icon.png", "/apple-touch-icon-precomposed.png"] {
            if let Ok(apple_url) = current_url.join(apple_icon) {
                icons.insert(Icon::new(
                    apple_url.to_string(),
                    "image/png".to_string(),
                    Some(180),
                    Some(180),
                ).with_purpose(Some("apple-touch-icon".to_string())).speculative());
            }
        }
        
        let mut manifest_urls = Vec::new();
        let mut config_urls = Vec::new();
        
        // Try fetching HTML and parsing link tags
        // Create a copy of forwarded headers that we can modify
        let mut headers = match forwarded_headers {
            Some(h) => h.clone(),
            None => HashMap::new(),
        };
        
        // Use a default desktop User-Agent for the initial HTML request
        headers.insert("User-Agent".to_string(), 
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36".to_string());
        
        debug!("Fetching HTML from URL: {}", current_url);
        let mut request_builder = client.get(current_url.as_str());
        
        // Apply headers
        for (name, value) in &headers {
            request_builder = request_builder.header(name, value);
        }
        
        let html_response = request_builder.send().await;
        if let Err(err) = &html_response {
            // Every other candidate lives on this same host, so probing them would only time out too
            if err.is_connect() {
                warn!("Could not connect to {}: {}", current_url, err);
                return Vec::new();
            }
        }

        if let Ok(response) = html_response {
            // Update current_url to the final URL after following any HTTP redirects
            current_url = response.url().clone();
            debug!("Successfully fetched HTML from URL: {}, status: {}", current_url, response.status());
            if let Ok((body, _)) = validation::read_limited(response, MAX_PAGE_BYTES).await {
                let text = String::from_utf8_lossy(&body);
                // Icon, manifest and refresh tags all live in <head>, so the body isn't parsed
                let document = Html::parse_document(html_head(&text));

                // Check for meta refresh redirect
                if let Some(redirect_url) = validation::extract_meta_refresh_url(&document) {
                    debug!("Detected meta refresh redirect to: {}", redirect_url);
                    
                    // Parse the redirect URL
                    let parsed_redirect_url = if let Ok(absolute_url) = Url::parse(&redirect_url) {
                        absolute_url
                    } else if let Ok(relative_url) = current_url.join(&redirect_url) {
                        // Handle relative URLs
                        relative_url
                    } else {
                        warn!("Failed to parse redirect URL: {}", redirect_url);
                        return Vec::new();
                    };
                    
                    // Update the current URL and increment the redirect counter
                    current_url = parsed_redirect_url;
                    current_redirect_count += 1;
                    
                    // Continue to the next iteration of the loop
                    continue;
                }

                // Look for all icon-related link tags
                let selector = Selector::parse("link[rel~='icon'], link[rel~='shortcut icon'], link[rel~='apple-touch-icon'], link[rel~='apple-touch-icon-precomposed'], link[rel~='mask-icon'], meta[name='msapplication-TileImage']").unwrap();
                
                for element in document.select(&selector) {
                    let tag_name = element.value().name();
                    
                    if tag_name == "link" {
                        if let Some(href) = element.value().attr("href") {
                            if let Some(icon_url) = resolve_icon_url(&current_url, href) {
                                let mut content_type = element.value().attr("type")
                                    .unwrap_or("image/x-icon")
                                    .to_string();
                                    
                                // Infer type from extension if not specified, ignoring any query string
                                if content_type == "image/x-icon" {
                                    let path = icon_url.path();
                                    if path.ends_with(".png") {
                                        content_type = "image/png".to_string();
                                    } else if path.ends_with(".svg") {
                                        content_type = "image/svg+xml".to_string();
                                    } else if path.ends_with(".webp") {
                                        content_type = "image/webp".to_string();
                                    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
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
                            if let Some(icon_url) = resolve_icon_url(&current_url, content) {
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
                        if let Ok(manifest_url) = current_url.join(href) {
                            manifest_urls.push(manifest_url);
                        }
                    }
                }
                
                // Look for browserconfig.xml
                let browserconfig_selector = Selector::parse("meta[name='msapplication-config']").unwrap();
                for element in document.select(&browserconfig_selector) {
                    if let Some(content) = element.value().attr("content") {
                        if let Ok(config_url) = current_url.join(content) {
                            config_urls.push(config_url);
                        }
                    }
                }
                
                // Look for Open Graph image as fallback
                let og_selector = Selector::parse("meta[property='og:image']").unwrap();
                for element in document.select(&og_selector) {
                    if let Some(content) = element.value().attr("content") {
                        if let Some(og_url) = resolve_icon_url(&current_url, content) {
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
            if let Ok(default_manifest) = current_url.join("/manifest.json") {
                manifest_urls.push(default_manifest);
            }
            if let Ok(default_manifest) = current_url.join("/site.webmanifest") {
                manifest_urls.push(default_manifest);
            }
        }
        
        // Fetch every manifest and browserconfig.xml at once rather than one after another
        let manifest_fetches = join_all(manifest_urls.iter().map(|manifest_url| {
            fetch_manifest_icons(client, manifest_url, forwarded_headers)
        }));
        let config_fetches = join_all(config_urls.iter().map(|config_url| {
            fetch_browserconfig_icon(client, config_url, &current_url)
        }));
        let (manifest_icons, config_icons) = futures::join!(manifest_fetches, config_fetches);
        icons.extend(manifest_icons.into_iter().flatten());
        icons.extend(config_icons.into_iter().flatten());
        
        // Validate all collected icons
        let mut icon_vec: Vec<Icon> = icons.into_iter().collect();
        
        // Calculate scores for all icons
        for icon in &mut icon_vec {
            icon.calculate_score();
        }
        
        // Icons the page declares come before guessed locations, highest score first within each
        icon_vec.sort_by(|a, b| a.speculative.cmp(&b.speculative).then(b.score.cmp(&a.score)));
        
        // Validate the top icons (up to 5) in parallel to avoid excessive sequential requests
        debug!("Validating top {} icons in parallel from URL: {}", icon_vec.len().min(5), current_url);
        let icons_to_validate: Vec<Icon> = icon_vec.iter().take(5).cloned().collect();

        // Use the parallel validate_icons function for significant performance improvement
        let headers = forwarded_headers
            .cloned()
            .unwrap_or_else(HashMap::new);

        validated_icons = validation::validate_icons(client, &icons_to_validate, &headers).await;
        
        // If we found valid icons, return them
        if !validated_icons.is_empty() {
            // Sort validated icons by score
            validated_icons.sort_by(|a, b| b.score.cmp(&a.score));
            info!("Found {} valid icons for URL: {}", validated_icons.len(), current_url);
            debug!("Best icon: {} (type: {}, size: {}x{})", 
                validated_icons[0].url, 
                validated_icons[0].content_type,
                validated_icons[0].width.unwrap_or(0),
                validated_icons[0].height.unwrap_or(0));
            return validated_icons;
        }
        
        // If no valid icons found, try additional sources
        debug!("No valid icons found in primary sources, trying additional sources for URL: {}", current_url);
        let additional_icons = try_additional_icon_sources(client, &current_url, forwarded_headers).await;
        if !additional_icons.is_empty() {
            let mut result = additional_icons;
            // Calculate scores for additional icons
            for icon in &mut result {
                icon.calculate_score();
            }
            // Sort by score
            result.sort_by(|a, b| b.score.cmp(&a.score));
            info!("Found {} valid icons from additional sources for URL: {}", result.len(), current_url);
            debug!("Best additional icon: {} (type: {}, size: {}x{})", 
                result[0].url, 
                result[0].content_type,
                result[0].width.unwrap_or(0),
                result[0].height.unwrap_or(0));
            return result;
        }
        
        // Last resort: the lower-scored icons that weren't in the first batch
        let remaining: Vec<Icon> = icon_vec.into_iter().skip(5).collect();
        let mut result = validation::validate_icons(client, &remaining, &headers).await;
        if result.is_empty() {
            warn!("No valid icons found for URL: {}", current_url);
        }
        result.sort_by(|a, b| b.score.cmp(&a.score));
        return result;
    }
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
