//! Runtime litellm pricing fetch.
//!
//! Called by [`super::sync_or_fallback`] when the on-disk cache has gone stale.
//! The byte-level parsing and filtering live in [`super::parse_litellm_json`]
//! so this module only deals with the HTTP round-trip.

use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use reqwest::Client;

use crate::domain::ModelPrice;

/// Upstream catalog URL. Identical to the one `build.rs` uses so fresh/stale
/// comparisons are apples-to-apples.
const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

/// Request timeout. litellm's JSON is ~1.5 MB; 30 s covers slow networks
/// without hanging the TUI indefinitely.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Fetch the latest litellm pricing snapshot and parse it into [`ModelPrice`]s.
///
/// # Errors
///
/// Returns any network / HTTP / body-read error. Parsing errors degrade to
/// `Ok(Vec::new())` inside [`super::parse_litellm_json`], so the caller
/// distinguishes "no network" from "empty catalog" via the returned vector.
pub async fn sync_from_github() -> Result<Vec<ModelPrice>> {
    let client = Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION"),
        ))
        .build()
        .context("build reqwest client")?;

    let resp = client
        .get(LITELLM_URL)
        .send()
        .await
        .context("GET litellm pricing")?;

    if !resp.status().is_success() {
        anyhow::bail!("litellm fetch returned HTTP {}", resp.status());
    }

    let bytes = resp.bytes().await.context("read litellm response body")?;

    Ok(super::parse_litellm_json(&bytes))
}
