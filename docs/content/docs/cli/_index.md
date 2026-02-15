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
| `rapina dev` | Start development server with hot reload |
| `rapina routes` | List all registered routes |
| `rapina doctor` | Run API health checks |
| `rapina openapi export` | Export OpenAPI spec |
| `rapina openapi check` | Verify spec is up to date |
| `rapina openapi diff` | Detect breaking changes |
