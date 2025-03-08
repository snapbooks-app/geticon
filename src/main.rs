use actix_web::{web::Data, App, HttpServer};
use geticon::handlers::{home, get_favicon_img, get_favicon_json, health_check};
use geticon::cache::create_default_icon_cache;
use std::env;
use std::sync::Arc;
use log::{info, warn, debug};
use env_logger::Env;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize env_logger
    env_logger::init_from_env(Env::default().default_filter_or("info"));
    
    info!("Starting GetIcon v{}", env!("CARGO_PKG_VERSION"));
    
    // Check if Sentry DSN is provided
    let sentry_enabled = env::var("SENTRY_DSN").is_ok();
    
    // Initialize Sentry if DSN is provided
    let _guard = if sentry_enabled {
        let dsn = env::var("SENTRY_DSN").unwrap();
        info!("Initializing Sentry with release: {}", env!("CARGO_PKG_VERSION"));
        
        // Get optional environment variable
        let environment = env::var("SENTRY_ENVIRONMENT").unwrap_or_else(|_| "production".into());
        
        Some(sentry::init((
            dsn,
            sentry::ClientOptions {
                release: Some(env!("CARGO_PKG_VERSION").into()),
                environment: Some(environment.into()),
                ..Default::default()
            },
        )))
    } else {
        info!("Sentry DSN not found, error monitoring disabled");
        None
    };

    info!("GetIcon server running at http://0.0.0.0:8080");
    
    // Create a client with disabled certificate validation
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("Failed to build reqwest client");
    
    debug!("Created HTTP client with certificate validation disabled");
    
    // Create icon cache
    let icon_cache = Arc::new(create_default_icon_cache());
    debug!("Initialized icon cache with 1-hour TTL");
    
    // Log middleware status
    if sentry_enabled {
        info!("Running with Sentry middleware enabled");
    } else {
        info!("Running without Sentry middleware");
    }
    
    // Run server with or without Sentry middleware based on environment variable
    if sentry_enabled {
        // With Sentry middleware
        HttpServer::new(move || {
            App::new()
                .app_data(Data::new(client.clone()))
                .app_data(Data::new(icon_cache.clone()))
                .wrap(sentry_actix::Sentry::new())
                .service(home)
                .service(get_favicon_img)
                .service(get_favicon_json)
                .service(health_check)
        })
        .bind("0.0.0.0:8080")?
        .run()
        .await
    } else {
        // Without Sentry middleware
        HttpServer::new(move || {
            App::new()
                .app_data(Data::new(client.clone()))
                .app_data(Data::new(icon_cache.clone()))
                .service(home)
                .service(get_favicon_img)
                .service(get_favicon_json)
                .service(health_check)
        })
        .bind("0.0.0.0:8080")?
        .run()
        .await
    }
}

#[cfg(test)]
mod tests;
