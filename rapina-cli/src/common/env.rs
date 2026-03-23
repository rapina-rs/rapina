/// Propagate legacy and generic port env vars into `RAPINA_PORT` so that
/// clap's `env = "RAPINA_PORT"` picks them up uniformly.
///
/// Fallback chain (first match wins):
///   1. `RAPINA_PORT` already set → do nothing
///   2. `SERVER_PORT` set → copy to `RAPINA_PORT` (backwards compat)
///   3. `PORT` set → copy to `RAPINA_PORT` (generic convention)
///
/// # Safety
/// Must be called before any threads are spawned (i.e. at the top of `main`).
pub fn propagate_port_env() {
    if std::env::var("RAPINA_PORT").is_ok() {
        return;
    }

    if let Ok(port) = std::env::var("SERVER_PORT") {
        // SAFETY: called at the start of main, before any threads are spawned.
        unsafe { std::env::set_var("RAPINA_PORT", &port) };
    } else if let Ok(port) = std::env::var("PORT") {
        // SAFETY: called at the start of main, before any threads are spawned.
        unsafe { std::env::set_var("RAPINA_PORT", &port) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env-var tests mutate global state, so we serialize them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_port_vars() {
        // SAFETY: ENV_LOCK serializes all env-mutating tests in this module,
        // ensuring no two tests race on these vars simultaneously.
        unsafe {
            std::env::remove_var("RAPINA_PORT");
            std::env::remove_var("SERVER_PORT");
            std::env::remove_var("PORT");
        }
    }

    #[test]
    fn rapina_port_already_set_is_not_overwritten() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_port_vars();

        // SAFETY: test-only, serialized by ENV_LOCK.
        unsafe {
            std::env::set_var("RAPINA_PORT", "4000");
            std::env::set_var("SERVER_PORT", "5000");
            std::env::set_var("PORT", "6000");
        }

        propagate_port_env();

        assert_eq!(std::env::var("RAPINA_PORT").unwrap(), "4000");
        clear_port_vars();
    }

    #[test]
    fn server_port_fallback() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_port_vars();

        // SAFETY: test-only, serialized by ENV_LOCK.
        unsafe {
            std::env::set_var("SERVER_PORT", "5000");
            std::env::set_var("PORT", "6000");
        }

        propagate_port_env();

        assert_eq!(std::env::var("RAPINA_PORT").unwrap(), "5000");
        clear_port_vars();
    }

    #[test]
    fn port_fallback() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_port_vars();

        // SAFETY: test-only, serialized by ENV_LOCK.
        unsafe {
            std::env::set_var("PORT", "6000");
        }

        propagate_port_env();

        assert_eq!(std::env::var("RAPINA_PORT").unwrap(), "6000");
        clear_port_vars();
    }

    #[test]
    fn no_env_vars_does_nothing() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_port_vars();

        propagate_port_env();

        assert!(std::env::var("RAPINA_PORT").is_err());
        clear_port_vars();
    }
}
