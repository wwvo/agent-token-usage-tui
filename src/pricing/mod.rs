//! litellm pricing sync, fallback, and cost calculation.
//!
//! Populated across M2 C6–C7. `build.rs` fetches
//! `model_prices_and_context_window.json` at compile time and `fallback.rs`
//! embeds it via `include_bytes!` for offline resilience.
