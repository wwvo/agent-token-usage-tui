//! Sidecar tests for [`NoopReporter`] and [`ChannelReporter`].

use std::path::PathBuf;

use pretty_assertions::assert_eq;
use tokio::sync::mpsc;

use super::ChannelReporter;
use super::NoopReporter;
use super::Reporter;
use super::ScanProgress;
use crate::domain::Source;

fn sample_progress(files_done: usize) -> ScanProgress {
    ScanProgress {
        source: Source::Claude,
        files_done,
        files_total: 10,
        current_file: Some(PathBuf::from("/tmp/session.jsonl")),
    }
}

#[test]
fn noop_reporter_accepts_every_progress() {
    let r = NoopReporter;
    for i in 0..5 {
        r.on_progress(sample_progress(i));
    }
    // No observable effect beyond "no panic".
}

#[tokio::test]
async fn channel_reporter_forwards_progress_to_receiver() {
    let (tx, mut rx) = mpsc::channel::<ScanProgress>(4);
    let r = ChannelReporter::new(tx);

    r.on_progress(sample_progress(1));
    r.on_progress(sample_progress(2));

    let first = rx.recv().await.expect("first");
    assert_eq!(first.files_done, 1);
    let second = rx.recv().await.expect("second");
    assert_eq!(second.files_done, 2);
}

#[tokio::test]
async fn channel_reporter_drops_when_channel_full_without_blocking() {
    // Capacity 1: fill with one message, the second must be dropped not blocked.
    let (tx, mut rx) = mpsc::channel::<ScanProgress>(1);
    let r = ChannelReporter::new(tx);

    r.on_progress(sample_progress(1));
    r.on_progress(sample_progress(2)); // would block if try_send weren't used

    let received = rx.recv().await.expect("one message present");
    assert_eq!(received.files_done, 1);
}

#[test]
fn scan_progress_equality_is_structural() {
    let a = sample_progress(3);
    let b = sample_progress(3);
    let c = sample_progress(4);
    assert_eq!(a, b);
    assert!(a != c);
}
