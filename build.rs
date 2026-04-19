//! Build-time fetcher for the litellm model pricing catalog.
//!
//! On every `cargo build` we attempt to refresh
//! `assets/litellm-prices.fallback.json` from upstream so the binary always
//! ships with a recent snapshot. The runtime `pricing` module embeds whatever
//! this file contains via `include_bytes!`, so the catalog is available even
//! on machines with no network at launch time.
//!
//! # Failure policy
//!
//! * Fetch fails, file already exists → keep the old file, print a warning.
//! * Fetch fails, file does not exist → write `{}` so `include_bytes!` still
//!   has a valid JSON to embed; the build succeeds with an empty catalog.
//!
//! Users / CI can set `AGENT_TUI_DISABLE_LITELLM_DOWNLOAD=1` to force the
//! "no network" path (useful for reproducible builds).

use std::env;
use std::fs;
use std::path::Path;
use std::time::Duration;

const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

const ASSET_PATH: &str = "assets/litellm-prices.fallback.json";

fn main() {
    // Only rerun when the build script itself changes or the user flips the
    // env-var opt-out; the downloaded file lives outside cargo's change
    // tracking so we don't feedback-loop on a freshly refreshed cache.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=AGENT_TUI_DISABLE_LITELLM_DOWNLOAD");

    let path = Path::new(ASSET_PATH);
    ensure_fallback(path);

    // Embed build metadata so `atut version` can show more than just the
    // SemVer string. Both are best-effort: when git / system clock fail we
    // emit empty strings rather than breaking the build.
    emit_build_metadata();
}

/// Emit `ATUT_GIT_HASH` and `ATUT_BUILD_DATE` as `cargo:rustc-env` keys.
///
/// * `ATUT_GIT_HASH` = short SHA of `HEAD`, or `""` when git isn't available
///   (e.g. building from a crates.io source tarball).
/// * `ATUT_BUILD_DATE` = UTC date in `YYYY-MM-DD` form; uses the system
///   clock, which is fine for a "best guess at build time" display field.
fn emit_build_metadata() {
    let git_hash = git_short_hash().unwrap_or_default();
    println!("cargo:rustc-env=ATUT_GIT_HASH={git_hash}");

    // Avoid pulling chrono into build deps just for a single YMD stamp —
    // format manually from SystemTime.
    let build_date = build_date_utc();
    println!("cargo:rustc-env=ATUT_BUILD_DATE={build_date}");
}

fn git_short_hash() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short=9", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Format today's UTC date as `YYYY-MM-DD` using a tiny Zeller-free loop.
fn build_date_utc() -> String {
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;

    let Ok(dur) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return String::from("unknown");
    };
    let mut days = (dur.as_secs() / 86_400) as i64;
    let mut y = 1970_i64;
    loop {
        let leap = is_leap(y);
        let year_days = if leap { 366 } else { 365 };
        if days < year_days {
            break;
        }
        days -= year_days;
        y += 1;
    }
    let months = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m: i64 = 1;
    for (idx, mdays) in months.iter().enumerate() {
        let feb_bonus = if idx == 1 && is_leap(y) { 1 } else { 0 };
        let total = *mdays + feb_bonus;
        if days < total {
            break;
        }
        days -= total;
        m += 1;
    }
    let d = days + 1;
    format!("{y:04}-{m:02}-{d:02}")
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn ensure_fallback(path: &Path) {
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            println!("cargo:warning=failed to create assets directory: {e}");
            // Can't write anywhere; leave and let include_bytes! fail loudly.
            return;
        }
    }

    let already_exists = path.exists();

    if env::var_os("AGENT_TUI_DISABLE_LITELLM_DOWNLOAD").is_some() {
        println!("cargo:warning=AGENT_TUI_DISABLE_LITELLM_DOWNLOAD is set; skipping litellm fetch");
        if !already_exists {
            let _ = fs::write(path, b"{}");
        }
        return;
    }

    match download() {
        Ok(body) => match fs::write(path, &body) {
            Ok(()) => println!(
                "cargo:warning=litellm fallback refreshed ({} bytes)",
                body.len(),
            ),
            Err(e) => println!("cargo:warning=failed to write fallback JSON: {e}"),
        },
        Err(e) => {
            println!(
                "cargo:warning=failed to fetch litellm pricing ({e}); using existing file if any"
            );
            if !already_exists {
                let _ = fs::write(path, b"{}");
            }
        }
    }
}

fn download() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION"),
            " (build.rs)",
        ))
        .build()?;

    let resp = client.get(LITELLM_URL).send()?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()).into());
    }
    Ok(resp.bytes()?.to_vec())
}
