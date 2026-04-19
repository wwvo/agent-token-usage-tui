//! Sidecar tests for `startup_ui::StartupReporter`.
//!
//! We can't capture stderr easily from the outside without shelling out to a
//! child process, so these tests focus on the side-effect-free properties:
//! reporter construction, trait impl, and the Reporter default hooks.

use crate::collector::Reporter;
use crate::collector::ScanProgress;
use crate::collector::ScanSummary;
use crate::domain::Source;

use super::StartupReporter;

#[test]
fn on_progress_is_silent_noop() {
    // Calling the progress hook must never panic and must never allocate
    // unusual resources — we're simply asserting the trait is implemented.
    let r = StartupReporter;
    r.on_progress(ScanProgress {
        source: Source::Claude,
        files_done: 3,
        files_total: 10,
        current_file: None,
    });
}

#[test]
fn on_source_start_and_finished_do_not_panic() {
    // End-to-end safety: lifecycle hooks work for every Source variant.
    let r = StartupReporter;
    for src in Source::all() {
        r.on_source_start(*src);
        r.on_source_finished(*src, &ScanSummary::new(*src));
    }
}
