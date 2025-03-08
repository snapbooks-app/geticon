# Changelog

All notable changes to the GetIcon project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

## [Unreleased]

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
