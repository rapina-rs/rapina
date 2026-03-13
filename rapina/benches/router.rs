use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use http::Method;
use rapina::prelude::*;

/// Resources used to generate realistic REST routes.
const RESOURCES: &[&str] = &[
    "users",
    "posts",
    "comments",
    "tags",
    "categories",
    "products",
    "orders",
    "invoices",
    "payments",
    "sessions",
    "notifications",
    "messages",
    "teams",
    "projects",
    "tasks",
    "events",
    "tickets",
    "reviews",
    "subscriptions",
    "reports",
    "analytics",
    "settings",
    "profiles",
    "addresses",
    "shipments",
    "coupons",
    "campaigns",
    "contacts",
    "leads",
    "deals",
    "pipelines",
    "workflows",
    "templates",
    "assets",
    "files",
    "folders",
    "permissions",
    "roles",
    "groups",
    "organizations",
];

/// Dummy async handler that satisfies the Router's type signature.
async fn noop(
    _req: http::Request<hyper::body::Incoming>,
    _params: rapina::extract::PathParams,
    _state: std::sync::Arc<rapina::state::AppState>,
) -> http::StatusCode {
    http::StatusCode::OK
}

/// Number of resources needed for a given total route count.
/// Each resource produces 5 routes (GET/POST collection + GET/PUT/DELETE individual).
fn resources_for(route_count: usize) -> usize {
    route_count / 5
}

/// Build a frozen router with `n` total routes (must be divisible by 5).
fn build_frozen_router(n: usize) -> Router {
    let count = resources_for(n);
    let mut router = Router::new();
    for &resource in &RESOURCES[..count] {
        let collection = format!("/v1/{resource}");
        let individual = format!("/v1/{resource}/:id");
        router = router
            .route(Method::GET, &collection, noop)
            .route(Method::POST, &collection, noop)
            .route(Method::GET, &individual, noop)
            .route(Method::PUT, &individual, noop)
            .route(Method::DELETE, &individual, noop);
    }
    router.prepare_bench();
    router
}

/// Build a router with `n` total routes but WITHOUT freezing (no static map, no trie).
/// Used for linear scan baseline comparison.
fn build_unfrozen_router(n: usize) -> Router {
    let count = resources_for(n);
    let mut router = Router::new();
    for &resource in &RESOURCES[..count] {
        let collection = format!("/v1/{resource}");
        let individual = format!("/v1/{resource}/:id");
        router = router
            .route(Method::GET, &collection, noop)
            .route(Method::POST, &collection, noop)
            .route(Method::GET, &individual, noop)
            .route(Method::PUT, &individual, noop)
            .route(Method::DELETE, &individual, noop);
    }
    router
}

const ROUTE_COUNTS: &[usize] = &[10, 50, 200];

fn static_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("static_lookup");
    for &n in ROUTE_COUNTS {
        let router = build_frozen_router(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| router.resolve(black_box(&Method::GET), black_box("/v1/users")));
        });
    }
    group.finish();
}

fn dynamic_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("dynamic_lookup");
    for &n in ROUTE_COUNTS {
        let router = build_frozen_router(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| router.resolve(black_box(&Method::GET), black_box("/v1/users/42")));
        });
    }
    group.finish();
}

fn not_found(c: &mut Criterion) {
    let mut group = c.benchmark_group("not_found");
    for &n in ROUTE_COUNTS {
        let router = build_frozen_router(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| router.resolve(black_box(&Method::GET), black_box("/v1/nonexistent/path")));
        });
    }
    group.finish();
}

fn mixed_traffic(c: &mut Criterion) {
    let mut group = c.benchmark_group("mixed_traffic");
    for &n in ROUTE_COUNTS {
        let router = build_frozen_router(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                // Static hit
                router.resolve(black_box(&Method::GET), black_box("/v1/users"));
                // Dynamic hit
                router.resolve(black_box(&Method::GET), black_box("/v1/users/42"));
                // 404
                router.resolve(black_box(&Method::GET), black_box("/v1/nonexistent/path"));
            });
        });
    }
    group.finish();
}

fn linear_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("linear_scan");
    for &n in ROUTE_COUNTS {
        let router = build_unfrozen_router(n);
        group.bench_with_input(BenchmarkId::new("static", n), &n, |b, _| {
            b.iter(|| router.resolve_linear(black_box(&Method::GET), black_box("/v1/users")));
        });
        group.bench_with_input(BenchmarkId::new("dynamic", n), &n, |b, _| {
            b.iter(|| router.resolve_linear(black_box(&Method::GET), black_box("/v1/users/42")));
        });
        group.bench_with_input(BenchmarkId::new("not_found", n), &n, |b, _| {
            b.iter(|| {
                router.resolve_linear(black_box(&Method::GET), black_box("/v1/nonexistent/path"))
            });
        });
        group.bench_with_input(BenchmarkId::new("mixed", n), &n, |b, _| {
            b.iter(|| {
                router.resolve_linear(black_box(&Method::GET), black_box("/v1/users"));
                router.resolve_linear(black_box(&Method::GET), black_box("/v1/users/42"));
                router.resolve_linear(black_box(&Method::GET), black_box("/v1/nonexistent/path"));
            });
        });
    }
    group.finish();
}

/// Build a matchit router with the same route patterns for apples-to-apples comparison.
///
/// matchit only does path matching (no method dispatch), so each unique path is
/// registered once. The value stored is a dummy index matching the order our
/// Router would assign. This isolates the radix trie comparison without
/// conflating method lookup overhead.
fn build_matchit_router(n: usize) -> matchit::Router<usize> {
    let count = resources_for(n);
    let mut router = matchit::Router::new();
    for (i, &resource) in RESOURCES[..count].iter().enumerate() {
        let collection = format!("/v1/{resource}");
        let individual = format!("/v1/{resource}/{{id}}");
        router.insert(&collection, i * 2).unwrap();
        router.insert(&individual, i * 2 + 1).unwrap();
    }
    router
}

fn matchit_baseline(c: &mut Criterion) {
    let mut group = c.benchmark_group("matchit");
    for &n in ROUTE_COUNTS {
        let router = build_matchit_router(n);
        group.bench_with_input(BenchmarkId::new("static", n), &n, |b, _| {
            b.iter(|| router.at(black_box("/v1/users")));
        });
        group.bench_with_input(BenchmarkId::new("dynamic", n), &n, |b, _| {
            b.iter(|| router.at(black_box("/v1/users/42")));
        });
        group.bench_with_input(BenchmarkId::new("not_found", n), &n, |b, _| {
            b.iter(|| router.at(black_box("/v1/nonexistent/path")));
        });
        group.bench_with_input(BenchmarkId::new("mixed", n), &n, |b, _| {
            b.iter(|| {
                let _ = router.at(black_box("/v1/users"));
                let _ = router.at(black_box("/v1/users/42"));
                let _ = router.at(black_box("/v1/nonexistent/path"));
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    static_lookup,
    dynamic_lookup,
    not_found,
    mixed_traffic,
    linear_scan,
    matchit_baseline
);
criterion_main!(benches);
