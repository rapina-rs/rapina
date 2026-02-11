//! Implementation of the `rapina test` command.

use colored::Colorize;
use notify_debouncer_mini::{DebounceEventResult, new_debouncer, notify::RecursiveMode};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// Configuration for the test command.
#[derive(Default)]
pub struct TestConfig {
    pub coverage: bool,
    pub watch: bool,
    pub filter: Option<String>,
}

/// Test results summary.
#[derive(Default)]
struct TestSummary {
    passed: u32,
    failed: u32,
    ignored: u32,
}

/// Execute the `test` command.
pub fn execute(config: TestConfig) -> Result<(), String> {
    verify_rapina_project()?;

    if config.coverage {
        check_coverage_tool()?;
    }

    if config.watch {
        run_watch_mode(&config)
    } else {
        run_tests(&config)
    }
}

/// Check if cargo-llvm-cov is installed.
fn check_coverage_tool() -> Result<(), String> {
    let output = Command::new("cargo")
        .args(["llvm-cov", "--version"])
        .output();

    match output {
        Ok(o) if o.status.success() => Ok(()),
        _ => {
            Err("cargo-llvm-cov not found. Install with: cargo install cargo-llvm-cov".to_string())
        }
    }
}

/// Verify that we're in a valid Rapina project directory.
fn verify_rapina_project() -> Result<(), String> {
    let cargo_toml = Path::new("Cargo.toml");
    if !cargo_toml.exists() {
        return Err("No Cargo.toml found. Are you in a Rust project directory?".to_string());
    }

    let content = std::fs::read_to_string(cargo_toml)
        .map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;

    let parsed: toml::Value = content
        .parse()
        .map_err(|e| format!("Failed to parse Cargo.toml: {}", e))?;

    let has_rapina = parsed
        .get("dependencies")
        .and_then(|deps| deps.get("rapina"))
        .is_some();

    if !has_rapina {
        return Err(
            "This doesn't appear to be a Rapina project (no rapina dependency found)".to_string(),
        );
    }

    Ok(())
}

/// Run tests once.
fn run_tests(config: &TestConfig) -> Result<(), String> {
    println!();
    println!(
        "{} Running tests...",
        "INFO".custom_color(colors::blue()).bold()
    );
    println!();

    let (cmd, args) = build_test_command(config);

    let mut child = Command::new(&cmd)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run tests: {}", e))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut summary = TestSummary::default();

    // Process stdout
    let stdout_reader = BufReader::new(stdout);
    for line in stdout_reader.lines().map_while(Result::ok) {
        process_test_line(&line, &mut summary);
    }

    // Process stderr (compilation errors, etc.)
    let stderr_reader = BufReader::new(stderr);
    for line in stderr_reader.lines().map_while(Result::ok) {
        eprintln!("{}", line);
    }

    let status = child
        .wait()
        .map_err(|e| format!("Failed to wait for tests: {}", e))?;

    println!();
    print_summary(&summary, status.success());

    if status.success() {
        Ok(())
    } else {
        Err("Tests failed".to_string())
    }
}

/// Run tests in watch mode.
fn run_watch_mode(config: &TestConfig) -> Result<(), String> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .map_err(|e| format!("Failed to set Ctrl+C handler: {}", e))?;

    println!();
    println!(
        "{} Watch mode enabled. Press Ctrl+C to stop.",
        "INFO".custom_color(colors::blue()).bold()
    );

    // Initial run
    let _ = run_tests(config);

    // Set up file watcher
    let (tx, rx) = mpsc::channel();

    let mut debouncer = new_debouncer(
        Duration::from_millis(300),
        move |res: DebounceEventResult| {
            if let Ok(events) = res {
                for event in events {
                    if event.path.extension().is_some_and(|ext| ext == "rs") {
                        let _ = tx.send(());
                        break;
                    }
                }
            }
        },
    )
    .map_err(|e| format!("Failed to create file watcher: {}", e))?;

    // Watch src and tests directories
    if Path::new("src").exists() {
        debouncer
            .watcher()
            .watch(Path::new("src"), RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch src directory: {}", e))?;
    }

    if Path::new("tests").exists() {
        debouncer
            .watcher()
            .watch(Path::new("tests"), RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch tests directory: {}", e))?;
    }

    println!(
        "{} Watching for changes...",
        "INFO".custom_color(colors::blue()).bold()
    );
    println!();

    while running.load(Ordering::SeqCst) {
        if rx.recv_timeout(Duration::from_millis(100)).is_ok() {
            println!();
            println!(
                "{} Change detected, re-running tests...",
                "INFO".custom_color(colors::yellow()).bold()
            );

            let _ = run_tests(config);

            println!(
                "{} Watching for changes...",
                "INFO".custom_color(colors::blue()).bold()
            );
            println!();
        }
    }

    println!();
    println!(
        "{} Stopped watching.",
        "INFO".custom_color(colors::blue()).bold()
    );

    Ok(())
}

/// Build the test command based on config.
fn build_test_command(config: &TestConfig) -> (String, Vec<String>) {
    let mut args = Vec::new();

    if config.coverage {
        args.push("llvm-cov".to_string());
        args.push("--".to_string());
    } else {
        args.push("test".to_string());
    }

    if let Some(ref filter) = config.filter {
        args.push(filter.clone());
    }

    // Add color output
    args.push("--color=always".to_string());

    ("cargo".to_string(), args)
}

/// Process a line of test output.
fn process_test_line(line: &str, summary: &mut TestSummary) {
    // Parse test result lines
    if line.contains("test result:") {
        // Already captured in summary parsing
    } else if line.contains(" ... ok") {
        summary.passed += 1;
        println!(
            "  {} {}",
            "✓".custom_color(colors::green()),
            extract_test_name(line).custom_color(colors::subtext())
        );
    } else if line.contains(" ... FAILED") {
        summary.failed += 1;
        println!(
            "  {} {}",
            "✗".custom_color(colors::red()),
            extract_test_name(line).custom_color(colors::red())
        );
    } else if line.contains(" ... ignored") {
        summary.ignored += 1;
        println!(
            "  {} {}",
            "○".custom_color(colors::yellow()),
            extract_test_name(line).custom_color(colors::subtext())
        );
    } else if line.starts_with("running ")
        || line.contains("Compiling")
        || line.contains("Finished")
    {
        println!("{}", line.custom_color(colors::subtext()));
    } else if !line.trim().is_empty() && !line.starts_with("test ") {
        // Print other relevant output (doc tests header, etc.)
        println!("{}", line);
    }
}

/// Extract test name from a test output line.
fn extract_test_name(line: &str) -> &str {
    line.strip_prefix("test ")
        .and_then(|s| s.split(" ...").next())
        .unwrap_or(line)
        .trim()
}

/// Print the test summary.
fn print_summary(summary: &TestSummary, success: bool) {
    let total = summary.passed + summary.failed + summary.ignored;

    println!("{}", "─".repeat(50).custom_color(colors::subtext()));

    if success {
        println!(
            "{} {} passed, {} failed, {} ignored",
            "PASS".custom_color(colors::green()).bold(),
            summary.passed.to_string().custom_color(colors::green()),
            summary.failed.to_string().custom_color(colors::subtext()),
            summary.ignored.to_string().custom_color(colors::yellow()),
        );
    } else {
        println!(
            "{} {} passed, {} failed, {} ignored",
            "FAIL".custom_color(colors::red()).bold(),
            summary.passed.to_string().custom_color(colors::green()),
            summary.failed.to_string().custom_color(colors::red()),
            summary.ignored.to_string().custom_color(colors::yellow()),
        );
    }

    if total > 0 {
        let bar_width = 40;
        let passed_width = (summary.passed as f64 / total as f64 * bar_width as f64) as usize;
        let failed_width = (summary.failed as f64 / total as f64 * bar_width as f64) as usize;
        let ignored_width = bar_width - passed_width - failed_width;

        let bar = format!(
            "{}{}{}",
            "█".repeat(passed_width).custom_color(colors::green()),
            "█".repeat(failed_width).custom_color(colors::red()),
            "░".repeat(ignored_width).custom_color(colors::subtext()),
        );
        println!("{}", bar);
    }

    println!();
}

/// Catppuccin Mocha color palette.
mod colors {
    use colored::CustomColor;

    pub fn subtext() -> CustomColor {
        CustomColor::new(166, 173, 200)
    }

    pub fn green() -> CustomColor {
        CustomColor::new(166, 227, 161)
    }

    pub fn yellow() -> CustomColor {
        CustomColor::new(249, 226, 175)
    }

    pub fn red() -> CustomColor {
        CustomColor::new(243, 139, 168)
    }

    pub fn blue() -> CustomColor {
        CustomColor::new(137, 180, 250)
    }
}
