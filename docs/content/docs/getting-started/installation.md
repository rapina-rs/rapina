+++
title = "Installation"
description = "Install Rapina and create your first API"
weight = 1
date = 2025-02-13
+++

## Prerequisites

Rapina is a Rust framework, so you'll need Rust installed on your system. If you're new to Rust, don't worry — installation takes about a minute.

### Installing Rust

The recommended way to install Rust is through [rustup](https://rustup.rs/), the official Rust toolchain installer.

**macOS / Linux:**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

After installation, restart your terminal or run:

```bash
source $HOME/.cargo/env
```

**Windows:**

Download and run [rustup-init.exe](https://win.rustup.rs/x86_64) from the official website. Follow the on-screen instructions.

Alternatively, if you use [winget](https://learn.microsoft.com/en-us/windows/package-manager/winget/):

```powershell
winget install Rustlang.Rustup
```

### Verify Installation

Confirm Rust is installed correctly:

```bash
rustc --version
cargo --version
```

You should see version numbers for both. Rapina requires Rust 1.75 or later.

### Platform Notes

**macOS:** Works out of the box. Xcode Command Line Tools will be installed automatically if needed.

**Linux:** You may need to install build essentials. On Ubuntu/Debian:

```bash
sudo apt install build-essential pkg-config libssl-dev
```

On Fedora:

```bash
sudo dnf install gcc pkg-config openssl-devel
```

**Windows:** Visual Studio Build Tools are required. The rustup installer will guide you through this. Make sure to select "Desktop development with C++" workload.

## Using the CLI (Recommended)

The fastest way to get started is with the Rapina CLI:

```bash
# Install the CLI
cargo install rapina-cli

# Create a new project
rapina new my-app
cd my-app

# Start the development server
rapina dev
```

Your API is now running at `http://127.0.0.1:3000`.

## Manual Setup

Add Rapina to your `Cargo.toml`:

```toml
[dependencies]
rapina = "0.5.0"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
```

## Your First API

Create a simple API with a few endpoints:

```rust
use rapina::prelude::*;

#[get("/")]
async fn hello() -> &'static str {
    "Hello, Rapina!"
}

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Result<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "id": id.into_inner(),
        "name": "Alice"
    })))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router = Router::new()
        .get("/", hello)
        .get("/users/:id", get_user);

    Rapina::new()
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
```
