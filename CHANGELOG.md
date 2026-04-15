# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Tower compatibility layer**: `tower` feature flag with `TowerLayerMiddleware` (tower Layer → rapina Middleware adapter), `RapinaService` (rapina stack → tower Service adapter), and `.layer()` builder method
- **NextService Clone support**: Tower layers requiring `Clone` on the inner service (e.g. tower-resilience, retry, circuit breaker) now work out of the box

## [0.10.0] - 2026-03-16

### Added
- **Serde-based Path extraction**: `Path<T>` now uses a custom serde deserializer, supporting `Path<u64>`, `Path<(u64, String)>` tuples, and `Path<MyStruct>` structs from a single implementation
- **Database seeding**: `rapina seed load`, `rapina seed dump`, and `rapina seed generate` commands behind `seed-*` feature flags
- **Snapshot testing**: `response.assert_snapshot("name")` with automatic UUID/timestamp redaction, `--bless` mode for updating golden files
- **RFC 7807 Problem Details**: Standardized error responses with configurable `ErrorConfig`, per-request scoping via `task_local!`
- **Three-layer router**: Static route map for O(1) parameterless lookup, hot cache, and frozen radix trie
- **Router benchmarks**: Criterion benchmarks for router resolution performance
- **Configurable request logging**: Verbose mode with header/query/body-size logging, header redaction for sensitive values
- **`--force` flag for `import database`**: Re-import over existing generated files
- **Irregular plurals in codegen**: Handles words like `status`, `address`, `child` correctly in singularize/pluralize
- **UUID primary key support** in `schema!` macro
- **`put_named` and `delete_named`** convenience methods on Router
- **URL shortener example**: Full CRUD example with database, migrations, and tests

### Changed
- **`State<T>` wrapped in `Arc<T>`**: Removes the `Clone` bound on state types, `into_inner()` returns `Arc<T>`
- **Positional extractor convention**: Last handler argument uses `FromRequest` (consumes body), all others use `FromRequestParts` — replaces string-based classification
- **`PathParams` backed by `SmallVec`**: Stack-allocated for up to 4 parameters, zero heap allocation for typical routes
- **Compression gated behind feature flag**: `compression` feature (enabled by default)
- **Macro preserves `mut` on handler arguments**: Enables mutable extractors like `mut form: Multipart`

## [0.6.0] - 2026-02-22

### Added
- **Route Auto Discovery**: Routes are automatically registered via `inventory` — no more manual wiring in `main.rs`
- `toml` upgraded to 1.0 (TOML spec 1.1 support)

### Changed
- Updated `jsonwebtoken` to 10.3.0
- Updated `ctrlc` to 3.5.2
- GitHub Actions: auto-labeler for PRs, welcome message for first-time contributors
- Consolidated Discord links across documentation

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

[Unreleased]: https://github.com/rapina-rs/rapina/compare/v0.10.0...HEAD
[0.10.0]: https://github.com/rapina-rs/rapina/compare/v0.9.0...v0.10.0
[0.6.0]: https://github.com/rapina-rs/rapina/compare/v0.5.0...v0.6.0
[0.2.0]: https://github.com/rapina-rs/rapina/compare/v0.1.0-alpha.3...v0.2.0
[0.1.0-alpha.3]: https://github.com/rapina-rs/rapina/compare/v0.1.0-alpha.2...v0.1.0-alpha.3
[0.1.0-alpha.2]: https://github.com/rapina-rs/rapina/compare/v0.1.0-alpha.1...v0.1.0-alpha.2
[0.1.0-alpha.1]: https://github.com/rapina-rs/rapina/releases/tag/v0.1.0-alpha.1
