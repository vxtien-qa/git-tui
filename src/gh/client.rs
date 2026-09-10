use anyhow::{bail, Context, Result};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

// ── Rate Limit Cache ──────────────────────────────────────────
// Cache the rate_limit API response for 60 seconds to avoid
// redundant API calls from settings screen, error screen, etc.
fn rate_limit_cache() -> &'static Mutex<(Option<String>, Instant)> {
    static CACHE: OnceLock<Mutex<(Option<String>, Instant)>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new((None, Instant::now())))
}

/// Query GitHub API rate limit and return human-readable status.
/// Results are cached in-memory for 60 seconds to avoid burning API calls.
pub fn rate_limit_info() -> String {
    const CACHE_TTL_SECS: u64 = 60;

    // Check in-memory cache first
    if let Ok(guard) = rate_limit_cache().lock() {
        if let (Some(ref cached), last_fetch) = *guard {
            if last_fetch.elapsed() < Duration::from_secs(CACHE_TTL_SECS) {
                return cached.clone();
            }
        }
    }

    let output = Command::new("gh")
        .args([
            "api",
            "rate_limit",
            "--jq",
            ".resources.graphql | \"\\(.remaining)/\\(.limit) reset:\\(.reset)\"",
        ])
        .output();

    let result = match output {
        Ok(out) if out.status.success() => {
            let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let parts: Vec<&str> = raw.splitn(2, " reset:").collect();
            if parts.len() == 2 {
                let quota = parts[0];
                if let Ok(reset_ts) = parts[1].parse::<i64>() {
                    let now = chrono::Utc::now().timestamp();
                    let wait_secs = (reset_ts - now).max(0);
                    let mins = wait_secs / 60;
                    let secs = wait_secs % 60;

                    let reset_time = chrono::DateTime::from_timestamp(reset_ts, 0)
                        .map(|dt| {
                            dt.with_timezone(&chrono::Local)
                                .format("%H:%M:%S")
                                .to_string()
                        })
                        .unwrap_or_default();

                    if wait_secs > 0 {
                        format!(
                            "API quota: {} - resets at {} ({}m {}s)",
                            quota, reset_time, mins, secs
                        )
                    } else {
                        format!("API quota: {} - limit has reset!", quota)
                    }
                } else {
                    format!("API quota: {}", quota)
                }
            } else {
                format!("API: {}", raw)
            }
        }
        _ => "Could not check API rate limit".to_string(),
    };

    // Store in cache
    if let Ok(mut guard) = rate_limit_cache().lock() {
        *guard = (Some(result.clone()), Instant::now());
    }

    result
}

/// Get just the remaining rate limit count (cached for 60s).
/// Returns None if unable to check.
pub fn rate_limit_remaining() -> Option<i64> {
    let info = rate_limit_info();
    parse_remaining(&info)
}

/// Non-blocking variant: returns the remaining count ONLY if a fresh cached
/// value exists - never spawns a process. For UI-thread callers (the blocking
/// lookup used to freeze the UI for the length of an HTTPS round trip on
/// every auto-refresh).
pub fn rate_limit_remaining_cached() -> Option<i64> {
    const CACHE_TTL_SECS: u64 = 60;
    if let Ok(guard) = rate_limit_cache().lock() {
        if let (Some(ref cached), last_fetch) = *guard {
            if last_fetch.elapsed() < Duration::from_secs(CACHE_TTL_SECS) {
                return parse_remaining(cached);
            }
        }
    }
    None
}

fn parse_remaining(info: &str) -> Option<i64> {
    info.strip_prefix("API quota: ")
        .and_then(|s| s.split('/').next())
        .and_then(|s| s.parse::<i64>().ok())
}

/// Execute a `gh` CLI command and return stdout as String.
pub fn exec(args: &[&str]) -> Result<String> {
    let output = Command::new("gh")
        .args(args)
        .output()
        .context("Failed to run gh CLI. Is it installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gh {} failed: {}", args.join(" "), stderr.trim());
    }

    let stdout = String::from_utf8(output.stdout).context("gh output is not valid UTF-8")?;
    Ok(stdout)
}

/// Execute a `gh` CLI command and parse JSON output.
pub fn exec_json<T: serde::de::DeserializeOwned>(args: &[&str]) -> Result<T> {
    let stdout = exec(args)?;
    let parsed: T = serde_json::from_str(&stdout).with_context(|| {
        // char-based preview - a byte slice could split a multibyte char and panic
        let preview: String = stdout.chars().take(200).collect();
        format!("Failed to parse gh JSON output:\n{}", preview)
    })?;
    Ok(parsed)
}

/// Execute a `gh api` call.
pub fn api(endpoint: &str, method: Option<&str>, body_args: &[(&str, &str)]) -> Result<String> {
    let mut cmd = Command::new("gh");
    cmd.arg("api");

    if let Some(m) = method {
        cmd.arg("--method").arg(m);
    }

    cmd.arg(endpoint);

    for (key, val) in body_args {
        cmd.arg("-f").arg(format!("{}={}", key, val));
    }

    let output = cmd.output().context("Failed to run gh api")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gh api {} failed: {}", endpoint, stderr.trim());
    }

    let stdout = String::from_utf8(output.stdout).context("gh api output is not valid UTF-8")?;
    Ok(stdout)
}

/// Check if `gh` CLI is installed and return its version.
pub fn version() -> Result<String> {
    let output = exec(&["--version"])?;
    // Output: "gh version 2.87.2 (2026-02-10)\n..."
    let version = output.lines().next().unwrap_or("").trim().to_string();
    Ok(version)
}
