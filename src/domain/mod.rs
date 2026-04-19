//! Domain types shared across storage, collector, pricing, and TUI.
//!
//! Kept free of heavy dependencies (only `chrono` + `serde` + `thiserror`) so
//! this module can be re-exported from any layer without pulling in rusqlite
//! or reqwest.

mod price;
mod prompt;
mod record;
mod session;
mod source;
mod windsurf_cost_diff;
mod windsurf_session;

pub use price::ModelPrice;
pub use prompt::PromptEvent;
pub use record::UsageRecord;
pub use session::SessionRecord;
pub use source::Source;
pub use source::UnknownSource;
pub use windsurf_cost_diff::WindsurfCostDiff;
pub use windsurf_session::WindsurfSessionRecord;
