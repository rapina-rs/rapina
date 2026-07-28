+++
title = "Metrics"
description = "Prometheus metrics with the metrics feature flag"
weight = 6
date = 2025-02-18
+++

Rapina can expose a `/metrics` endpoint in [Prometheus](https://prometheus.io/) text format. Enable it with the `metrics` feature flag.

## Setup

Add the feature to your `Cargo.toml`:

```toml
[dependencies]
rapina = { version = "0.13.0", features = ["metrics"] }
```

Enable the endpoint in your application:

```rust
use rapina::prelude::*;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .enable_metrics()
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
```

That's all. A `GET /metrics` route is registered automatically and returns the collected metrics in Prometheus text format.

## Dynamic configuration

When the value comes from a config struct or environment variable, use `with_metrics(bool)` to keep the builder chain intact:

```rust
let cfg = Config::from_env();

Rapina::new()
    .with_metrics(cfg.metrics_enabled)
    .router(router)
    .listen("127.0.0.1:3000")
    .await
```

Both forms are equivalent, `enable_metrics()` and `disable_metrics()` are convenience wrappers around `with_metrics(true/false)`.

## Collected Metrics

| Metric                          | Type      | Labels                     | Description                             |
| ------------------------------- | --------- | -------------------------- | --------------------------------------- |
| `http_requests_total`           | Counter   | `method`, `path`, `status` | Total number of HTTP requests completed |
| `http_request_duration_seconds` | Histogram | `method`, `path`           | Request duration in seconds             |
| `http_requests_in_flight`       | Gauge     | —                          | Requests currently being processed      |

Example output:

```
# HELP http_requests_total Total number of HTTP requests
# TYPE http_requests_total counter
http_requests_total{method="GET",path="/users",status="200"} 42
http_requests_total{method="POST",path="/users",status="201"} 7
http_requests_total{method="GET",path="/users/:id",status="404"} 3

# HELP http_request_duration_seconds HTTP request duration in seconds
# TYPE http_request_duration_seconds histogram
http_request_duration_seconds_bucket{method="GET",path="/users",le="0.005"} 38
http_request_duration_seconds_sum{method="GET",path="/users"} 0.312
http_request_duration_seconds_count{method="GET",path="/users"} 42

# HELP http_requests_in_flight Number of HTTP requests currently being processed
# TYPE http_requests_in_flight gauge
http_requests_in_flight 2
```

## Path Normalisation

The `path` label is the route pattern that matched, not the raw URL. Cardinality stays
bounded by the number of routes you registered, so a path param never inflates the label
set, whether it's a number, a UUID, or any other string:

| Route definition | Raw request path                              | Label value  |
| ---------------- | --------------------------------------------- | ------------ |
| `/users/:id`     | `/users/42`                                   | `/users/:id` |
| `/users/:id`     | `/users/e58ed763-928c-4155-bee9-fdbaaadc15f6` | `/users/:id` |
| `/users/:id`     | `/users/whatever`                             | `/users/:id` |

The label keeps the parameter name from the route definition, so `/orders/:order_id` shows
up as `/orders/:order_id` and matches what `rapina routes` prints.

Requests that match no route (404s) are labelled `<unmatched>`. They share a single time
series, so a client probing random URLs can't explode the metric. The full request path is
still recorded on the OpenTelemetry span (`url.path`) when tracing is enabled, which is where
high-cardinality data belongs.

## Custom Metrics

Register your own Prometheus collectors alongside the built-in HTTP metrics using `add_metric()`:

```rust
use rapina::prelude::*;
use rapina::prometheus::{IntCounterVec, Opts};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let orders_total = IntCounterVec::new(
        Opts::new("orders_total", "Total number of orders placed"),
        &["status"],
    )
    .unwrap();

    // Clone before passing so you can increment it from your handlers.
    let orders_counter = orders_total.clone();

    Rapina::new()
        .enable_metrics()
        .add_metric(orders_total)
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
```

All types that implement `prometheus::core::Collector` are accepted — `IntCounter`, `IntCounterVec`, `Gauge`, `Histogram`, `HistogramVec`, and any custom collector.

> **Name collisions:** Rapina panics at startup if a custom metric name clashes with a built-in metric (`http_requests_total`, `http_request_duration_seconds`, `http_requests_in_flight`) or with another previously registered custom collector. Use unique names to avoid this.

## Auto-discovery

Instead of wiring collectors through the builder, mark a module-level `static` with `#[metric]` and let `.discover()` register it, the same hands-free flow route handlers get:

```rust
use std::sync::LazyLock;

use rapina::metric;
use rapina::prelude::*;
use rapina::prometheus::IntCounter;

#[metric]
static ORDERS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    IntCounter::new("orders_total", "Total orders placed").unwrap()
});

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Handlers increment ORDERS_TOTAL directly; .discover() finds and registers it.
    Rapina::new()
        .enable_metrics()
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

Discovery requires both calls. With `.enable_metrics()` but no `.discover()`, or `.discover()` but no metrics, the collector stays out of `/metrics` and Rapina logs a warning at startup naming the missing call. With both off, nothing happens.

The collector type must be `Clone` (all built-in prometheus types are; clones share the same underlying values, which is why incrementing the static shows up in `/metrics`). Wrap the collector in `std::sync::LazyLock` or `once_cell::sync::Lazy`; no built-in prometheus type can be constructed in a const context, so a bare static won't compile. `OnceLock`-style cells are not supported, and the static must live at module scope. Non-`Clone` custom collectors keep using `add_metric()`.

> **Name collisions apply here too:** discovered collectors go through the same registry, so two `#[metric]` statics sharing a metric name anywhere in your binary (including dependencies), or a `#[metric]` static plus an `add_metric()` call with the same name, panic at startup. The panic message names the colliding metric.

## Scraping with Prometheus

Point Prometheus at the `/metrics` endpoint in your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: my-rapina-api
    static_configs:
      - targets: ["localhost:3000"]
```
