+++
title = "Installation"
description = "Install Rapina and create your first API"
weight = 1
date = 2025-02-13
+++

## Start Here

Four commands from zero to running API:

```bash
cargo install rapina-cli
rapina new my-app
cd my-app
rapina dev
```

Your API is now running at `http://127.0.0.1:3000`.

## Try It

Hit the default endpoints:

```bash
curl http://127.0.0.1:3000/
```

```json
{"message": "Hello from Rapina!"}
```

```bash
curl http://127.0.0.1:3000/__rapina/health
```

```json
{"status": "ok"}
```

The health endpoint is enabled by `.with_health_check(true)` in `main.rs`. See [Health Checks](@/docs/core-concepts/state.md#health-checks) for database and custom checks.

Check what routes are available:

```bash
rapina routes
```

```
GET    /         [public]
```

## What the CLI Created

The `rapina new` command scaffolded a complete project for you. See [Project Structure](@/docs/getting-started/project-structure.md) for a full walkthrough of what each file does.

## Prerequisites

Rapina is a Rust framework. If you don't have Rust installed yet, it takes about a minute.

### Installing Rust

Install through [rustup](https://rustup.rs/), the official Rust toolchain installer:

**macOS / Linux:**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

After installation, restart your terminal or run:

```bash
source $HOME/.cargo/env
```

**Windows:**

Download and run [rustup-init.exe](https://win.rustup.rs/x86_64) from the official website, or use [winget](https://learn.microsoft.com/en-us/windows/package-manager/winget/):

```powershell
winget install Rustlang.Rustup
```

### Verify Installation

```bash
rustc --version
cargo --version
```

You should see version numbers for both. Rapina requires Rust 1.75 or later.

### Platform Notes

**macOS:** Works out of the box. Xcode Command Line Tools will be installed automatically if needed.

**Linux:** You may need build essentials. On Ubuntu/Debian:

```bash
sudo apt install build-essential pkg-config libssl-dev
```

On Fedora:

```bash
sudo dnf install gcc pkg-config openssl-devel
```

**Windows:** Visual Studio Build Tools are required. The rustup installer will guide you through this — select the "Desktop development with C++" workload.

## Manual Setup

If you prefer not to use the CLI, add Rapina to an existing project:

```toml
[dependencies]
rapina = "0.10.0"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
```

Then write your entry point:

```rust
use rapina::prelude::*;

#[derive(Serialize, JsonSchema)]
struct MessageResponse {
    message: String,
}

#[public]
#[get("/")]
async fn hello() -> Json<MessageResponse> {
    Json(MessageResponse {
        message: "Hello from Rapina!".into(),
    })
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```
