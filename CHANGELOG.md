# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2025-01-24

### Added
- **Authentication**: JWT authentication with "protected by default" approach
  - `#[public]` attribute for public routes
  - `CurrentUser` extractor for accessing authenticated user
  - `AuthConfig` for JWT configuration from environment
  - `TokenResponse` helper for login endpoints
- **Configuration**: Type-safe config with `#[derive(Config)]` macro
  - `#[env = "VAR_NAME"]` for environment variable binding
  - `#[default = "value"]` for default values
  - `load_dotenv()` helper for .env files
  - Fail-fast validation with clear error messages
- **Documentation**: Full docs site at userapina.com
  - Getting started guide
  - CLI reference
  - Philosophy section
- **CLI**: New commands
  - `rapina doctor` for health checks
  - `rapina routes` for route introspection

### Changed
- All routes now require authentication by default (use `#[public]` to opt-out)
- Improved error messages for missing configuration

## [0.1.0-alpha.3] - 2025-01-15

### Added
- OpenAPI 3.0 automatic generation
- CLI tools: `rapina openapi export`, `rapina openapi check`, `rapina openapi diff`
- Breaking change detection for API contracts
- Validation with `Validated<T>` extractor
- Observability with structured logging and tracing

## [0.1.0-alpha.2] - 2025-01-10

### Added
- Route introspection endpoint (`/__rapina/routes`)
- Test client for integration testing
- Middleware system (`Timeout`, `BodyLimit`, `TraceId`)

## [0.1.0-alpha.1] - 2025-01-05

### Added
- Initial release
- Basic router with path parameters
- Typed extractors (`Json`, `Path`, `Query`, `Form`, `Headers`, `State`)
- Standardized error handling with `trace_id`
- CLI (`rapina new`, `rapina dev`)

[Unreleased]: https://github.com/arferreira/rapina/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/arferreira/rapina/compare/v0.1.0-alpha.3...v0.2.0
[0.1.0-alpha.3]: https://github.com/arferreira/rapina/compare/v0.1.0-alpha.2...v0.1.0-alpha.3
[0.1.0-alpha.2]: https://github.com/arferreira/rapina/compare/v0.1.0-alpha.1...v0.1.0-alpha.2
[0.1.0-alpha.1]: https://github.com/arferreira/rapina/releases/tag/v0.1.0-alpha.1
