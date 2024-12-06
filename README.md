# GetIcon

A fast and efficient favicon fetching service built in Rust. GetIcon provides a simple HTTP API to retrieve favicons from any website, with built-in caching and performance optimizations.

## Features

- 🚀 Simple REST API endpoint
- 📦 ETag support for efficient caching
- ⚡ HTTP cache headers for improved performance
- 🌐 Support for any website's favicon.ico
- 📄 Built-in HTML documentation page

## Installation

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

## Usage

### API Endpoint

```
GET /url/{website-url}
```

The API accepts a website URL (without protocol) and returns its favicon.

### Examples

To fetch GitHub's favicon:
```
GET http://localhost:8080/url/github.com
```

To fetch Microsoft's favicon:
```
GET http://localhost:8080/url/microsoft.com
```

### Response

- Success: Returns the favicon with `image/x-icon` content type
- Not Found: Returns 404 if favicon doesn't exist
- Not Modified: Returns 304 if favicon hasn't changed (when using ETag)

## Cache Support

GetIcon implements efficient caching through:
- ETag headers for client-side caching
- Cache-Control headers with a 1-hour max age
- 304 Not Modified responses when content hasn't changed

## Development

Built with:
- Rust
- Actix-web framework
- reqwest for HTTP requests
- md5 for ETag generation

## License

See [LICENSE](LICENSE) file for details.
