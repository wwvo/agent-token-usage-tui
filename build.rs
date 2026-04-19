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
