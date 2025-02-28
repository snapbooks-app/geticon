# Changelog

All notable changes to the GetIcon project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
