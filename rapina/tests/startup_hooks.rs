//! Integration tests for `Rapina::on_startup()` lifecycle hooks.
//!
//! These are separate-crate tests, so they exercise the public API only.
//! `TestClient` deliberately does not run startup hooks, so every case here
//! goes through a real `listen()` call. A failing hook must return before the
//! TCP listener is bound, which means all of these tests finish without ever
//! accepting traffic — `127.0.0.1:0` is only ever resolved, never bound.
//!
//! Every `listen()` is wrapped in a timeout so a mis-wired implementation
//! fails the test instead of hanging CI forever.

use std::sync::{Arc, Mutex};
use std::time::Duration;

// Imported by name rather than glob: `rapina::prelude::*` also exports
// `rapina::Result`, which would shadow `std::result::Result` in this file.
use rapina::prelude::Rapina;

/// Generous upper bound: startup hooks are a handful of bookkeeping steps, so
/// anything approaching this means `listen()` never observed the hook result.
const LISTEN_TIMEOUT: Duration = Duration::from_secs(10);

/// A hook that panics synchronously, before it even produces a future.
fn panicking_sync_hook() -> std::future::Ready<Result<(), std::io::Error>> {
    panic!("sync boom")
}

/// A hook whose future panics after being polled at least once.
async fn panicking_async_hook() -> Result<(), std::io::Error> {
    tokio::task::yield_now().await;
    panic!("async boom")
}

/// A hook that panics with a payload that is not a `&str`/`String`.
async fn non_string_panic_hook() -> Result<(), std::io::Error> {
    std::panic::panic_any(42u32)
}

/// Runs `listen()` on `addr` under a timeout and requires it to fail, returning
/// the error so callers can inspect its message.
async fn listen_expect_err_at(app: Rapina, addr: &str) -> std::io::Error {
    let result = tokio::time::timeout(LISTEN_TIMEOUT, app.listen(addr))
        .await
        .expect(
            "listen() did not return within 10s; startup hooks are not being awaited before bind",
        );
    result.expect_err("listen() must fail when a startup hook fails")
}

/// Runs `listen()` on an ephemeral port under a timeout and requires it to fail.
async fn listen_expect_err(app: Rapina) -> std::io::Error {
    listen_expect_err_at(app, "127.0.0.1:0").await
}

/// Binds an ephemeral port, records its address, and releases it again so a
/// hook can prove the server has not bound it yet.
fn free_port() -> std::net::SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("failed to bind a free port");
    let addr = listener
        .local_addr()
        .expect("failed to read the ephemeral address");
    drop(listener);
    addr
}

#[tokio::test]
#[serial_test::serial]
async fn startup_hook_error_makes_listen_fail() {
    let app = Rapina::new()
        .disable_introspection()
        .on_startup(|| async { Err::<(), std::io::Error>(std::io::Error::other("boom")) });

    let err = listen_expect_err(app).await;
    let message = err.to_string();

    assert!(
        message.contains("startup hook 1 failed"),
        "error should name the failing hook, got: {message}"
    );
    assert!(
        message.contains("boom"),
        "error should carry the hook's error, got: {message}"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn startup_hooks_run_in_registration_order() {
    let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

    let first = Arc::clone(&log);
    let second = Arc::clone(&log);
    let third = Arc::clone(&log);

    let app = Rapina::new()
        .disable_introspection()
        .on_startup(move || {
            let log = Arc::clone(&first);
            async move {
                log.lock().unwrap().push("first");
                Ok::<(), std::io::Error>(())
            }
        })
        .on_startup(move || {
            let log = Arc::clone(&second);
            async move {
                log.lock().unwrap().push("second");
                Ok::<(), std::io::Error>(())
            }
        })
        .on_startup(move || {
            let log = Arc::clone(&third);
            async move {
                log.lock().unwrap().push("third");
                Err::<(), std::io::Error>(std::io::Error::other("third failed"))
            }
        });

    let err = listen_expect_err(app).await;
    assert!(
        err.to_string().contains("startup hook 3 failed"),
        "the third hook is the one that failed, got: {err}"
    );

    let recorded = log.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec!["first", "second", "third"],
        "hooks must run in registration order"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn startup_hook_failure_skips_remaining_hooks() {
    let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

    let first = Arc::clone(&log);
    let third = Arc::clone(&log);

    let app = Rapina::new()
        .disable_introspection()
        .on_startup(move || {
            let log = Arc::clone(&first);
            async move {
                log.lock().unwrap().push("one");
                Ok::<(), std::io::Error>(())
            }
        })
        .on_startup(|| async { Err::<(), std::io::Error>(std::io::Error::other("second failed")) })
        .on_startup(move || {
            let log = Arc::clone(&third);
            async move {
                log.lock().unwrap().push("three");
                Ok::<(), std::io::Error>(())
            }
        });

    let err = listen_expect_err(app).await;
    assert!(
        err.to_string().contains("startup hook 2 failed"),
        "error should point at the second hook, got: {err}"
    );

    let recorded = log.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec!["one"],
        "hooks after the first failure must not run"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn synchronous_panic_in_hook_is_caught() {
    let app = Rapina::new()
        .disable_introspection()
        .on_startup(panicking_sync_hook);

    let err = listen_expect_err(app).await;
    let message = err.to_string();

    assert!(
        message.contains("startup hook 1 panicked"),
        "a synchronous panic must be caught and reported, got: {message}"
    );
    assert!(
        message.contains("sync boom"),
        "the panic payload must be preserved, got: {message}"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn async_panic_in_hook_is_caught() {
    let app = Rapina::new()
        .disable_introspection()
        .on_startup(panicking_async_hook);

    let err = listen_expect_err(app).await;
    let message = err.to_string();

    assert!(
        message.contains("startup hook 1 panicked"),
        "a panic while polling the hook future must be caught, got: {message}"
    );
    assert!(
        message.contains("async boom"),
        "the panic payload must be preserved, got: {message}"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn panic_in_second_hook_skips_third() {
    let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

    let first = Arc::clone(&log);
    let third = Arc::clone(&log);

    let app = Rapina::new()
        .disable_introspection()
        .on_startup(move || {
            let log = Arc::clone(&first);
            async move {
                log.lock().unwrap().push("one");
                Ok::<(), std::io::Error>(())
            }
        })
        .on_startup(panicking_async_hook)
        .on_startup(move || {
            let log = Arc::clone(&third);
            async move {
                log.lock().unwrap().push("three");
                Ok::<(), std::io::Error>(())
            }
        });

    let err = listen_expect_err(app).await;
    assert!(
        err.to_string().contains("startup hook 2 panicked"),
        "the second hook is the one that panicked, got: {err}"
    );

    let recorded = log.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec!["one"],
        "hooks after a panicking hook must not run"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn startup_hooks_run_before_listener_is_bound() {
    // The probe address must be the one `listen()` would bind: if startup hooks
    // ever ran after the listener bound, this hook's bind would fail with
    // EADDRINUSE and the assertion below would catch it.
    let addr = free_port();
    let was_free: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));

    let slot = Arc::clone(&was_free);
    let app = Rapina::new().disable_introspection().on_startup(move || {
        let slot = Arc::clone(&slot);
        async move {
            let bound = std::net::TcpListener::bind(addr).is_ok();
            *slot.lock().unwrap() = Some(bound);
            // Stop here so `listen()` returns without ever binding.
            Err::<(), std::io::Error>(std::io::Error::other("stop before bind"))
        }
    });

    let err = listen_expect_err_at(app, &addr.to_string()).await;
    assert!(
        err.to_string().contains("startup hook 1 failed"),
        "the hook must fail, not panic, to keep the assertion honest, got: {err}"
    );

    let bound = *was_free.lock().unwrap();
    assert_eq!(
        bound,
        Some(true),
        "the listener must not be bound while startup hooks run"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn non_string_panic_payload_is_caught() {
    let app = Rapina::new()
        .disable_introspection()
        .on_startup(non_string_panic_hook);

    let err = listen_expect_err(app).await;
    assert!(
        err.to_string().contains("non-string panic payload"),
        "a non-string payload must still be reported, got: {err}"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn error_type_from_boxed_dyn_error() {
    let app = Rapina::new().disable_introspection().on_startup(|| async {
        let err: Box<dyn std::error::Error + Send + Sync> = "boxed boom".into();
        Err::<(), _>(err)
    });

    let err = listen_expect_err(app).await;
    assert!(
        err.to_string().contains("boxed boom"),
        "an already-boxed trait object must be accepted as the hook error, got: {err}"
    );
}
