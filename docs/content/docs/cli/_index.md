+++
title = "CLI"
weight = 4
sort_by = "weight"
+++

The Rapina CLI provides tools for development, project scaffolding, and API management.

## Installation

```bash
cargo install rapina-cli
```

## Commands

| Command | Description |
|---------|-------------|
| `rapina new <name>` | Create a new project |
| `rapina add resource <name> <fields...>` | Scaffold a CRUD resource |
| `rapina dev` | Start development server with hot reload |
| `rapina test` | Run tests with pretty output |
| `rapina routes` | List all registered routes |
| `rapina doctor` | Run API health checks |
| `rapina migrate new <name>` | Generate a new migration file |
| `rapina openapi export` | Export OpenAPI spec |
| `rapina openapi check` | Verify spec is up to date |
| `rapina openapi diff` | Detect breaking changes |
