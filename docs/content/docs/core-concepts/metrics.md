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
rapina = { version = "0.5.0", features = ["metrics"] }
```

Enable the endpoint in your application:

```rust
use rapina::prelude::*;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .with_metrics(true)
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
```

That's all. A `GET /metrics` route is registered automatically and returns the collected metrics in Prometheus text format.

## Collected Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `http_requests_total` | Counter | `method`, `path`, `status` | Total number of HTTP requests completed |
| `http_request_duration_seconds` | Histogram | `method`, `path` | Request duration in seconds |
| `http_requests_in_flight` | Gauge | — | Requests currently being processed |

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

To prevent label cardinality explosion, pure-numeric path segments are automatically replaced with `:id`:

| Raw request path | Label value |
|------------------|-------------|
| `/users/42` | `/users/:id` |
| `/users/123/posts/456` | `/users/:id/posts/:id` |
| `/users/profile` | `/users/profile` |

This means `/users/1`, `/users/2`, and `/users/999` all map to the same label set and are counted together.

## Scraping with Prometheus

Point Prometheus at the `/metrics` endpoint in your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: my-rapina-api
    static_configs:
      - targets: ["localhost:3000"]
```
