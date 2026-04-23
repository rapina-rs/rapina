//! llms.txt tools.

use colored::Colorize;
use std::fs;
use std::process::Command;

/// Export llms.txt to stdout or file.
pub fn export(output: Option<String>, host: &str, port: u16) -> Result<(), String> {
    let content = fetch_llms_txt(host, port)?;

    match output {
        Some(path) => {
            fs::write(&path, &content).map_err(|e| format!("Failed to write file: {}", e))?;
            println!("  {} llms.txt exported to {}", "✓".green(), path.cyan());
        }
        None => {
            print!("{}", content);
        }
    }

    Ok(())
}

/// Fetch llms.txt from the running application.
fn fetch_llms_txt(host: &str, port: u16) -> Result<String, String> {
    let url = crate::common::urls::build_llms_url(host, port);
    let output = Command::new("curl")
        .args(["-s", "-f", &url])
        .output()
        .map_err(|e| format!("Failed to run curl: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to fetch llms.txt. Is the server running on {}?",
            url
        ));
    }

    String::from_utf8(output.stdout).map_err(|e| format!("Invalid UTF-8 response: {}", e))
}
