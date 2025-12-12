use serde::Serialize;
use crate::url_utils::normalize_url_string;

#[derive(Serialize, Hash, Eq, PartialEq, Debug, Clone)]
pub struct Icon {
    pub url: String,
    #[serde(rename = "type")]
    pub content_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(skip)]
    pub score: u32,
}

impl Icon {
    pub fn new(url: String, content_type: String, width: Option<u32>, height: Option<u32>) -> Self {
        // Normalize URL for storage and comparison
        let normalized_url = normalize_url_string(&url)
            .map(|u| format!("https://{}", u))
            .unwrap_or(url);
            
        Icon {
            url: normalized_url,
            content_type,
            width,
            height,
            purpose: None,
            score: 0,
        }
    }
    
    pub fn with_purpose(mut self, purpose: Option<String>) -> Self {
        self.purpose = purpose;
        self
    }
    
    pub fn calculate_score(&mut self) {
        let mut score = 0;
        
        // Score based on format quality
        match self.content_type.as_str() {
            "image/svg+xml" => score += 50, // SVG is best for scaling
            "image/png" => score += 40,     // PNG is good quality
            "image/webp" => score += 35,    // WEBP is good but less supported
            "image/jpeg" | "image/jpg" => score += 30,
            "image/x-icon" | "image/vnd.microsoft.icon" => score += 20,
            "image/gif" => score += 10,
            _ => score += 5,
        }
        
        // Score based on size (larger is better for high-DPI displays)
        if let (Some(width), Some(height)) = (self.width, self.height) {
            let size = width.max(height);
            if size >= 512 { score += 30; }
            else if size >= 256 { score += 25; }
            else if size >= 192 { score += 20; }
            else if size >= 128 { score += 15; }
            else if size >= 64 { score += 10; }
            else if size >= 32 { score += 5; }
            else { score += 2; }
        } else {
            // Unknown size, modest score
            score += 3;
        }
        
        // Score based on purpose
        if let Some(purpose) = &self.purpose {
            if purpose.contains("maskable") { score += 10; } // Good for Android adaptive icons
            if purpose.contains("apple-touch-icon") { score += 15; } // Apple icons are high quality, typically 180x180
            if purpose.contains("any") { score += 5; }
            if purpose.contains("og:image") { score -= 25; } // Penalize OG images - they're fallback only
        }
        
        self.score = score;
    }
}

#[derive(Serialize)]
pub struct IconResponse {
    pub url: String,
    pub icons: Vec<Icon>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_icon: Option<Icon>,
}
