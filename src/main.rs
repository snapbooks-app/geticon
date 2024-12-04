use actix_web::{get, web, App, HttpServer, HttpResponse, http::header};

#[get("/")]
async fn home() -> HttpResponse {
    let html = r#"<!DOCTYPE html>
<html>
<head>
    <title>GetIcon - Favicon Fetcher</title>
    <style>
        body { font-family: sans-serif; max-width: 800px; margin: 40px auto; padding: 0 20px; line-height: 1.6; }
        pre { background: #f4f4f4; padding: 15px; border-radius: 5px; }
    </style>
</head>
<body>
    <h1>GetIcon - Favicon Fetcher</h1>
    <p>Simple API to fetch favicons from websites.</p>
    
    <h2>Usage</h2>
    <p>Make a GET request to:</p>
    <pre>/url/{website-url}</pre>
    
    <h2>Example</h2>
    <p>To get GitHub's favicon:</p>
    <pre>/url/github.com</pre>
</body>
</html>"#;

    HttpResponse::Ok()
        .content_type("text/html")
        .body(html)
}

#[get("/url/{url}")]
async fn get_favicon(url: web::Path<String>) -> HttpResponse {
    let client = reqwest::Client::new();
    let url_str = format!("https://{}/favicon.ico", url.into_inner());
    
    match client.get(&url_str).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.bytes().await {
                    Ok(bytes) => {
                        HttpResponse::Ok()
                            .content_type("image/x-icon")
                            .append_header((header::CACHE_CONTROL, "public, max-age=3600"))
                            .body(bytes)
                    },
                    Err(_) => HttpResponse::InternalServerError()
                        .body("Failed to read favicon content")
                }
            } else {
                HttpResponse::NotFound()
                    .body("Favicon not found")
            }
        }
        Err(_) => HttpResponse::InternalServerError()
            .body("Failed to fetch favicon")
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("Server running at http://localhost:8080");
    
    HttpServer::new(|| {
        App::new()
            .service(home)
            .service(get_favicon)
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
