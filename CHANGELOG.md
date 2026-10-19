# Changelog

All notable changes to the GetIcon project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- Keep the query string of icon URLs: it was cut off, so icons such as aub.no's `og:image` (`thumbnail.php?h=1400&img=…`) got a 400 and were skipped. `http://` icons are switched to `https://` only on `https://` pages, as browsers do
- Guess an undeclared icon type from the URL path instead of the end of the `href`, so eg.no's `favicon.svg?auto=format&h=32&w=32` is checked as an SVG instead of failing as an ICO
- Unescape `&amp;` in browserconfig.xml tile URLs

## [0.7.1] - 2026-10-13

### Fixed
- Put `curl` back in the Docker image: the deploy's container health check runs `curl -f http://localhost:8080/health`, so 0.7.0 containers never turned healthy and the deploy waited on them indefinitely

### Security
- Only use dependency versions published at least 14 days ago: 30 locked crates published in the last 14 days are pinned back to older releases (for example actix-web 4.16.0 to 4.15.0 and tokio 1.53.2 to 1.53.1), and the older compression-codecs moves brotli from 9.0.0 back to 8.0.4
- Pin the Docker base images to digests (`rust:1.98.1-slim-trixie` and `debian:trixie-20260918-slim`) instead of the moving `rust:1-slim-trixie` and `debian:trixie-slim` tags
- Pin the workflow's GitHub Actions to commit SHAs

## [0.7.0] - 2026-10-13

### Added
- Serve icons for up to 3 days after they go stale while one background task per icon refreshes them; a failed refresh keeps serving the last good icon
- Concurrent requests for the same uncached page share one lookup (10 simultaneous cold requests: 8 upstream requests instead of 80)

### Changed
- Validate each icon once per request instead of twice; a cold `/img` now makes 8 upstream requests instead of 11
- Stop icon discovery as soon as a site can't be reached, with a 3-second connect timeout and a 15-second cap on discovery (an unresponsive host fails in 3 seconds instead of 65)
- Negative-cache every failed lookup, including `/json` and fetch errors on `/img`
- Cache each page's icon lookup once and share it between `/img` (every size) and `/json`; cache downloaded images per icon URL
- Cached failures keep their original status and message, and are retried on the next request once their 1-hour window has passed
- `/json` sends the same Cache-Control as `/img`: 2 hours, or 10 minutes while a stale entry is being refreshed
- The health check reports `discovery_cache` and `image_cache` counts instead of `main_cache`, `expired_cache` and `negative_cache`
- Parse only a page's `<head>`, and only once (a 1.5 MB page went from two full parses, about 80 ms of CPU, to 0.04 ms)
- Check icons by reading the image header instead of decoding the whole image (a 1200×630 JPEG: 16.6 ms to 0.02 ms)
- Request gzip, brotli and deflate compression from sites
- Cap every download, counted after decompression: pages are read up to 1 MB, manifests and browserconfig.xml up to 256 KB, and icons over 2 MB are rejected with "Icon is too large"
- Bound the image cache at 64 MB instead of 2000 entries of any size
- Fall back to a ranged GET when a server rejects HEAD (400, 403, 405 or 501); such sites now get their icon instead of a 404, in 13 upstream requests instead of 34
- Check the icons a page declares before guessed locations such as `/favicon.ico` and `/apple-touch-icon.png`
- Fetch web app manifests and browserconfig.xml in parallel
- Probe 17 common icon paths instead of 24, best first, and stop at the first batch that finds one (a page with no declared icons: 12 upstream requests instead of 31)
- Docker image is now a multi-stage build on Debian 13 (trixie): it ships only the binary and its runtime libraries (143 MB, where the old image was based on the 1.19 GB Rust toolchain image), and dependencies build in their own cached layer
- Commit `Cargo.lock` and build the Docker image with `--locked`
- README: building from source needs Rust 1.89 or later (required by the locked dependencies)

### Fixed
- Fixed og:image penalty wrapping to a huge score (or panicking in debug builds) for icons with a low base score
- Stopped guessed apple-touch-icon URLs from outranking icons the site actually declares
- Cache reads no longer restart an entry's 2-hour TTL, so popular icons get refreshed instead of being served indefinitely
- A cache miss answered with 304 Not Modified now stores the icon in the server cache

## [0.6.2] - 2025-12-12

### Fixed
- Penalize og:image in icon scoring to prefer actual favicons over page thumbnails

## [0.6.1] - 2025-12-12

### Fixed
- Fixed icon resolution for sites with HTTP redirects to different domains

## [0.6.0] - 2025-03-13

### Changed
- Optimized HTTP client configuration with connection pooling and longer timeouts
- Improved cache mechanism with 2-hour TTL (up from 1 hour)
- Enhanced Cache-Control headers with longer max-age values

### Added
- Implemented dual-layer caching system with main and expired caches
- Added background refresh mechanism for expired icons
- Improved user experience by returning expired icons immediately (with 10-minute TTL)
- Parallel icon validation using futures::stream for improved performance
- Implemented negative caching to avoid repeated validation of failed URLs
- Parallelized fetching of additional icon sources
- Enhanced health check endpoint with detailed cache statistics

### Fixed
- Fixed performance bottlenecks in the /img endpoint
- Improved handling of expired cache entries

## [0.5.1] - 2025-03-10

### Added
- Support for handling HTML meta refresh redirects when fetching favicons

## [0.5.0] - 2025-03-08

### Changed
- Created dedicated validation module to improve code organization
- Refactored server initialization for better maintainability

### Added
- Structured logging system with `log` and `env_logger`
- Smart User-Agent selection based on icon type (iOS/Android/Windows)
- Robust icon validation to prevent returning invalid content
- Fallback mechanism for finding alternative icons when primary sources fail

### Fixed
- Fixed empty icon issues for sites like happybytes.no
- Fixed HTML page redirects for sites like burgerking.no
- Improved handling of PNG files with valid signatures but unsupported features


## [0.4.2] - 2025-03-05

### Fixed
- Fixed 404 errors when fetching favicons from sites with strict security measures by forwarding client headers
- Added header forwarding to improve compatibility with sites that implement bot detection
- Enhanced error logging for favicon fetching failures

## [0.4.1] - 2025-03-04

### Fixed
- Disabled certificate validation to fix issues with sites using certificates from unknown issuers
- Improved error handling with more specific HTTP status codes for different types of network errors
- Added more detailed error messages to help with debugging

## [0.4.0] - 2025-03-01

### Added
- Server-side caching for icons to ensure consistent results
- Cache keys include size parameter for size-specific caching
- Memory cache with 1-hour TTL for improved performance

## [0.3.2] - 2025-02-28
- Changed server binding from 127.0.0.1 to 0.0.0.0 for container networking

## [0.3.1] - 2025-02-28

### Added
- Added curl to the Docker container for easier API testing and health checks

## [0.3.0] - 2025-02-28

### Added
- Health check endpoint at `/health` that returns service status

### Changed
- Updated Sentry dependency from 0.31 to 0.36

## [0.2.0] - 2025-02-28

### Changed
- Updated documentation with comprehensive API details
- Improved installation instructions with Docker support
- Added environment variables documentation

## [0.1.0] - 2025-02-28

### Added
- Initial release of GetIcon
- REST API endpoints for both image and JSON responses
- ETag support for efficient caching
- HTTP cache headers with a 1-hour max age
- Support for any website's favicon.ico
- Built-in HTML documentation page
- Smart icon selection with size parameter support
- Detection of multiple icon types:
  - Standard favicon.ico in site root
  - HTML link tags with rel="icon", "shortcut icon", etc.
  - Apple Touch icons
  - Web App Manifest icons
  - Microsoft Tile images
  - Open Graph images (as fallback)
- Icon scoring algorithm based on format quality, size, and purpose
- Docker support for easy deployment
- GitHub Actions workflow for Docker image building and publishing
- Sentry integration for error monitoring (optional)
- URL normalization for consistent handling of different URL formats
