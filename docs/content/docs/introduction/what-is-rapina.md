+++
title = "What is Rapina?"
weight = 1
date = 2025-02-13
+++

Rapina is a modern, high-performance web framework for building APIs with Rust, inspired by FastAPI's developer experience. Built on predictable patterns and strong conventions.

## Why Rapina?

- **Fast**: Native Rust performance. No garbage collector, no runtime overhead.
- **Type-safe**: Typed extractors, typed errors, everything checked at compile time. If it compiles, it works.
- **Opinionated**: Convention over configuration. 90% of apps need 10% of decisions. We made them for you.
- **AI-friendly**: Predictable patterns that humans and LLMs understand equally well.
- **Secure**: Protected by default. All routes require authentication unless explicitly marked public.
- **Batteries-included**: Standardized errors with trace_id, JWT auth, and OpenAPI generation out of the box.

## Who is it for?

You know Rust and you're tired of boilerplate. You want to ship APIs fast without sacrificing type safety.

If you're new to Rust, start with [The Rust Book](https://doc.rust-lang.org/book/) — then come back. HTTP and REST basics help, but Rapina handles the ceremony so you can focus on building.

## Built on solid foundations

Rapina is built directly on battle-tested crates:

- **Hyper** for HTTP handling
- **Tokio** for async runtime
- **SeaORM** for database operations (optional)

No layers of abstraction. Maximum control, maximum performance.
