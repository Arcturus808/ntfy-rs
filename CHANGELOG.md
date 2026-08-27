# Changelog

All notable changes to ntfy-rs are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- TOML config examples in README now use kebab-case (matching `serde(rename_all = "kebab-case")` in `src/config.rs`)
- iOS lock screen notifications now show "New message" instead of bare "ntfy / notification" — achieved by sending a JSON poll request body with a fallback message, matching the Go ntfy server format
- Fixed pre-existing clippy warnings: `field_reassign_with_default` in `src/config.rs` tests, `useless_conversion` in `src/handlers/ws.rs`, `infallible_destructuring_match` in `src/main.rs`

### Changed
- Clarified that `Tags` header expects emoji shortcodes (e.g. `white_check_mark`), not raw emoji characters (e.g. `✅`)
- Added lock screen appearance note to iOS upstream poll-forward docs section
- Declared MSRV 1.85 in `Cargo.toml` (driven by `lettre 0.11.22` and `clap 4.6.1`)
- Added Linux build prerequisites (C compiler, `pkg-config`, OpenSSL dev libs) to README
- Added `AGENTS.md` with development rules: CI credit conservation, branching strategy, pre-commit checks, MSRV policy

## [v0.1.0-beta.7] - 2026-08-22

### Changed
- Bump version to v0.1.0-beta.7

## [v0.1.0-beta.6] - 2026-08-22

### Added
- Multi-platform release builds (Windows, Linux x86_64/arm64, macOS x86_64/arm64)
- `workflow_dispatch` trigger for manual release workflow runs

### Changed
- Rename crate to avoid collision on crates.io
- Reorganize feature flags section in README for clarity
- Remove DE-5000 reference from README — ntfy-rs audience is general

## [v0.1.0-beta.5] - 2026-08-21

### Added
- Cargo feature flags for optional email, metrics, TLS, webpush, auth, unix-socket, config-file

## [v0.1.0-beta.4] - 2026-08-21

### Added
- `--version` flag in CLI command description

## [v0.1.0-beta.3] - 2026-08-21

### Fixed
- Prometheus recorder installation is now non-fatal on server restart
- Use `build_recorder().handle()` for Prometheus restart fallback

## [v0.1.0-beta.2] - 2026-08-20

### Fixed
- Only embed Windows resources when building binary, not library

## [v0.1.0-beta.1] - 2026-08-20

### Added
- Rust trademark disclaimer
- Disclaimer about no affiliation with original ntfy Go project
- Codecov coverage badge and CI coverage workflow (77 total tests)
- GitHub Sponsors badge

### Fixed
- Refined Content-Disposition filename sanitization with production security notes

## [v0.1.0-alpha.6] - 2026-08-19

### Added
- License, CI, and release badges to README

## [v0.1.0-alpha.5] - 2026-08-19

### Added
- Custom icon embedded in Windows executable via winres build script
- Icon set (favicon, apple-touch-icon, ICO) and icon-gen tool

### Fixed
- icon-gen now loads system fonts so SVG text elements render correctly

## [v0.1.0-alpha.4] - 2026-08-18

### Added
- Logo and curl examples for attachments/actions
- Action button iOS limitation note

### Changed
- Updated logo image

## [v0.1.0-alpha.3] - 2026-08-17

### Fixed
- Removed `ring` from dependency tree to eliminate `SystemFunction036` import and AV false positive

## [v0.1.0-alpha.2] - 2026-08-17

### Fixed
- Release workflow permissions (403 error) and opt into Node.js 24
- Deduplicate web push subscriptions per topic+endpoint (schema v5)
- Prevent SSRF in web push endpoint validation (GHSA-w9hq-5jg7-q4j7)

## [v0.1.0-alpha.1] - 2026-08-16

### Added
- SMTP STARTTLS config option, optional auth, Opportunistic TLS fallback
- TLS/cert documentation, X-Attach support, attachment improvements
- iOS/APNs upstream poll-forward documentation
- Emoji shortcode resolution for tags

### Fixed
- Set maximum body limit for requests in router

### Changed
- Use lowercase `cgo` per Go community convention
