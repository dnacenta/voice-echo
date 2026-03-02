use std::process::Command;

use super::ansi;

struct CheckResult {
    name: &'static str,
    found: bool,
    detail: String,
}

/// Run prerequisite checks and print results.
/// Returns true if all required tools are found.
pub fn run_checks() -> bool {
    let checks = vec![
        check_tool("rustc", &["--version"], "rustc"),
        check_tool("openssl", &["version"], "openssl"),
    ];

    println!("\n  {} Checking prerequisites", ansi::bold(">>"));

    let mut all_ok = true;
    for c in &checks {
        if c.found {
            println!(
                "  {} {}: {}",
                ansi::green("\u{2713}"),
                c.name,
                ansi::dim(&c.detail)
            );
        } else {
            println!(
                "  {} {}: {}",
                ansi::red("\u{2717}"),
                c.name,
                ansi::red(&c.detail)
            );
            all_ok = false;
        }
    }

    if !all_ok {
        println!(
            "\n  {} Missing prerequisites. Install them and re-run setup.",
            ansi::red("!")
        );
    }

    all_ok
}

fn check_tool(cmd: &'static str, args: &[&str], display_name: &'static str) -> CheckResult {
    match Command::new(cmd).args(args).output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let version = stdout.lines().next().unwrap_or("").trim().to_string();
            CheckResult {
                name: display_name,
                found: true,
                detail: version,
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = stderr
                .lines()
                .next()
                .unwrap_or("unknown error")
                .trim()
                .to_string();
            CheckResult {
                name: display_name,
                found: false,
                detail: msg,
            }
        }
        Err(_) => CheckResult {
            name: display_name,
            found: false,
            detail: "not found in PATH".to_string(),
        },
    }
}
