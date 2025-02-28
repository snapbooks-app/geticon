use url::Url;

/// Normalizes a URL string to a consistent format
pub fn normalize_url_string(url: &str) -> Option<String> {
    let input = url.trim();
    
    // Handle URLs with port numbers first
    if url.contains(':') && !url.starts_with("http") {
        if let Some(port_str) = url.split(':').nth(1) {
            if let Some(port) = port_str.split('/').next() {
                if port.chars().all(|c| c.is_digit(10)) {
                    // Try parsing with the port
                    let with_port = format!("https://{}:{}", url.split(':').next().unwrap_or(url), port);
                    if let Ok(parsed) = Url::parse(&with_port) {
                        let mut normalized = parsed.host_str()?.to_string();
                        normalized.push(':');
                        normalized.push_str(port);
                        return Some(normalized);
                    }
                }
            }
        }
    }

    // Try parsing as is
    let parsed = Url::parse(input).or_else(|_| {
        // Try with https://
        Url::parse(&format!("https://{}", input))
    }).ok()?;
    
    // Normalize: remove trailing slash, query params, and fragments
    let mut normalized = parsed.host_str()?.to_string();
    if let Some(port) = parsed.port() {
        normalized.push(':');
        normalized.push_str(&port.to_string());
    }
    if let Some(path) = parsed.path_segments() {
        let path: Vec<_> = path.collect();
        if !path.is_empty() && path != [""] {
            normalized.push('/');
            normalized.push_str(&path.join("/"));
        }
    }
    
    Some(normalized)
}

/// Normalizes a URL string and returns a Url object
pub async fn normalize_url(input: &str) -> Option<Url> {
    let normalized = normalize_url_string(input)?;
    Url::parse(&format!("https://{}", normalized)).ok()
}
