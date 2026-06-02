//! llms.txt tools.

use colored::Colorize;
use std::fs;

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
    let mut response = crate::common::AGENT.get(&url).call().map_err(|e| {
        format!(
            "Failed to fetch llms.txt. Is the server running on {}? ({})",
            url, e
        )
    })?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_llms_txt_success() {
        let body = "User-agent: *\r\nDisallow: /admin";
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0; 4096];
            use std::io::Read;
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            use std::io::Write;
            stream.write_all(response.as_bytes()).unwrap();
        });
        let result = fetch_llms_txt("127.0.0.1", port);
        handle.join().unwrap();
        assert_eq!(result.unwrap(), body);
    }

    #[test]
    fn test_fetch_llms_txt_connection_refused() {
        let result = fetch_llms_txt("127.0.0.1", 1);
        assert!(result.is_err());
    }
}
