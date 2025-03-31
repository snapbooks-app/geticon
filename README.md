# GetIcon

A fast and efficient favicon fetching service built in Rust. GetIcon provides a simple HTTP API to retrieve favicons from any website, with built-in caching and performance optimizations.

## Features

- 🚀 Simple REST API endpoints for both image and JSON responses
- 📦 Server-side caching for consistent results
- 🔄 ETag support for efficient client-side caching
- ⚡ HTTP cache headers for improved performance
- 🌐 Support for any website's favicon.ico
- 📄 Built-in HTML documentation page
- 🔍 Smart icon selection with size parameter support
- 📱 Detection of multiple icon types (favicon.ico, Apple Touch, Web App Manifest)
- 🔄 Docker support for easy deployment
- 📊 Sentry integration for error monitoring (optional)

## Installation

### Option 1: Using Rust

1. Ensure you have Rust installed (1.75.0 or later)
2. Clone the repository:
```bash
git clone https://github.com/yourusername/geticon.git
cd geticon
```
3. Build and run:
```bash
cargo run
```

The server will start at `http://localhost:8080`

### Option 2: Using Docker

1. Pull the Docker image:
```bash
docker pull ghcr.io/snapbooks-app/geticon:latest
```

2. Run the container:
```bash
docker run -p 8080:8080 ghcr.io/snapbooks-app/geticon:latest
```

Or use Docker Compose:

```yaml
# docker-compose.yml
version: '3'
services:
  geticon:
    image: ghcr.io/snapbooks-app/geticon:latest
    ports:
      - "8080:8080"
    environment:
      - RUST_LOG=info  # Controls log level
      - SENTRY_DSN=your_sentry_dsn  # Optional
      - SENTRY_ENVIRONMENT=production  # Optional
```

Then run:
```bash
docker-compose up -d
```

## Usage

GetIcon is in production use at [snapbooks.no](https://snapbooks.no).

### API Endpoints

#### Get Favicon as Image

```
GET /img?url={website-url}
```

Optional: Specify size with the `size` parameter:
```
GET /img?url={website-url}&size={size}
```

#### Get Favicon Information as JSON

```
GET /json?url={website-url}
```

Optional: Specify size with the `size` parameter:
```
GET /json?url={website-url}&size={size}
```

#### Health Check

```
GET /health
```

Returns a JSON response with service status information.

### Examples

To fetch GitHub's favicon as an image:
```
GET http://localhost:8080/img?url=github.com
```

To fetch Microsoft's favicon at 192px size:
```
GET http://localhost:8080/img?url=microsoft.com&size=192
```

To get JSON information about Google's favicon:
```
GET http://localhost:8080/json?url=google.com
```

### Responses

#### Image Endpoint
- Success: Returns the favicon with appropriate content type (image/png, image/x-icon, etc.)
- Not Found: Returns 404 if favicon doesn't exist
- Not Modified: Returns 304 if favicon hasn't changed (when using ETag)

#### JSON Endpoint
Returns a JSON object with:
- `url`: The normalized URL
- `icons`: Array of all detected icons with their properties
- `best_icon`: The selected best icon based on scoring algorithm

## Cache Support

GetIcon implements efficient caching through:
- Server-side in-memory cache with 1-hour TTL
- Consistent icon selection for the same URL and size
- ETag headers for client-side caching
- Cache-Control headers with a 1-hour max age
- 304 Not Modified responses when content hasn't changed

## Icon Detection

GetIcon searches for icons in multiple locations:
- Standard favicon.ico in site root
- HTML link tags with rel="icon", "shortcut icon", etc.
- Apple Touch icons
- Web App Manifest icons
- Microsoft Tile images
- Open Graph images (as fallback)

## User-Agent Handling

GetIcon uses device-specific User-Agent strings to improve compatibility with websites that implement strict security measures:

- Apple Touch Icons: iOS/Safari User-Agent
- Android/Maskable Icons: Android/Chrome User-Agent
- Microsoft Tile Images: Windows/Edge User-Agent
- Standard Icons: Windows/Chrome User-Agent

The User-Agent strings are periodically updated from [useragents.me](https://www.useragents.me) to ensure they remain current and effective.

### Maintenance Note

To keep the service working optimally with all websites, the User-Agent strings should be updated periodically (recommended: every 3-6 months) by checking the latest common User-Agents at [useragents.me](https://www.useragents.me).

## Environment Variables

The following environment variables can be configured:

| Variable | Description | Default |
|----------|-------------|---------|
| RUST_LOG | Controls log filtering (e.g., `info`, `geticon=debug`, `debug`) | info |
| SENTRY_DSN | Sentry DSN for error monitoring | (none) |
| SENTRY_ENVIRONMENT | Environment name for Sentry | production |

## Development

Built with:
- Rust
- Actix-web framework for the HTTP server
- reqwest for HTTP requests
- scraper for HTML parsing
- serde for JSON serialization
- md5 for ETag generation
- moka for high-performance in-memory caching
- log and env_logger for structured logging
- sentry for error monitoring

## License

See [LICENSE](LICENSE) file for details.
