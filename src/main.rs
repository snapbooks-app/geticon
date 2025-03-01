use actix_web::{web::Data, App, HttpServer};
use geticon::handlers::{home, get_favicon_img, get_favicon_json, health_check};
use geticon::cache::create_default_icon_cache;
use std::env;
use std::sync::Arc;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Check if Sentry DSN is provided
    let sentry_enabled = env::var("SENTRY_DSN").is_ok();
    
    // Initialize Sentry if DSN is provided
    let _guard = if sentry_enabled {
        let dsn = env::var("SENTRY_DSN").unwrap();
        println!("Initializing Sentry with release: {}", env!("CARGO_PKG_VERSION"));
        
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
        println!("Sentry DSN not found, error monitoring disabled");
        None
    };

    println!("GetIcon server running at http://0.0.0.0:8080");
    
    let client = reqwest::Client::new();
    
    // Create icon cache
    let icon_cache = Arc::new(create_default_icon_cache());
    println!("Initialized icon cache with 1-hour TTL");
    
    // Run server with or without Sentry based on environment variable
    if sentry_enabled {
        println!("Running with Sentry middleware enabled");
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
        println!("Running without Sentry middleware");
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
