//! Sidecar tests for [`Source`] round-tripping.

use std::str::FromStr;

use pretty_assertions::assert_eq;

use super::Source;

#[test]
fn as_str_and_from_str_roundtrip_for_all_variants() {
    for source in Source::all() {
        let s = source.as_str();
        let parsed = Source::from_str(s).expect("canonical strings must parse back");
        assert_eq!(parsed, *source);
    }
}

#[test]
fn display_matches_as_str() {
    for source in Source::all() {
        assert_eq!(source.to_string(), source.as_str());
    }
}

#[test]
fn from_str_rejects_unknown() {
    let err = Source::from_str("unknown-agent").expect_err("nonsense must fail");
    assert!(err.to_string().contains("unknown-agent"));
}

#[test]
fn serde_roundtrip_uses_lowercase() {
    let json = serde_json::to_string(&Source::OpenClaw).expect("serialize");
    assert_eq!(json, "\"openclaw\"");

    let back: Source = serde_json::from_str("\"opencode\"").expect("deserialize");
    assert_eq!(back, Source::OpenCode);
}

#[test]
fn all_covers_every_variant() {
    // Exhaustive match guard: if a new variant is added and not appended to
    // `Source::all()`, this test fails at compile time because the match stops
    // being exhaustive.
    for source in Source::all() {
        match source {
            Source::Claude
            | Source::Codex
            | Source::OpenClaw
            | Source::OpenCode
            | Source::Windsurf => {}
        }
    }
    assert_eq!(Source::all().len(), 5);
}
