//! Sidecar tests for TUI render helpers (pure formatting + color policy).

use pretty_assertions::assert_eq;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

use super::StyleExt;
use super::format_int;
use super::format_usd;
use super::no_color;
use super::truncate;

#[test]
fn format_int_thousands_separator() {
    assert_eq!(format_int(0), "0");
    assert_eq!(format_int(42), "42");
    assert_eq!(format_int(1_234), "1,234");
    assert_eq!(format_int(1_000_000), "1,000,000");
    assert_eq!(format_int(-1_234), "-1,234");
}

#[test]
fn format_usd_always_four_decimals() {
    assert_eq!(format_usd(0.0), "$0.0000");
    assert_eq!(format_usd(1.2345), "$1.2345");
    assert_eq!(format_usd(1234.5678), "$1234.5678");
}

#[test]
fn truncate_short_strings_passthrough() {
    assert_eq!(truncate("abc", 10), "abc");
}

#[test]
fn truncate_long_strings_append_ellipsis() {
    assert_eq!(truncate("abcdefghijklmn", 5), "abcd…");
}

#[test]
fn truncate_exactly_max_passthrough() {
    assert_eq!(truncate("abcde", 5), "abcde");
}

// ---- NO_COLOR policy ------------------------------------------------------

/// Drive the full `NO_COLOR` state machine inside one test so env mutations
/// don't race across parallel tests (cargo test runs tests concurrently by
/// default, and `std::env::set_var` is process-global — hence unsafe on
/// edition 2024). Any regression in any sub-case fails this test.
#[test]
fn no_color_policy_respects_env() {
    // SAFETY: this test is the only one in the crate touching NO_COLOR,
    // and it restores the original state at the end. `env::set_var` is
    // safe when callers agree there is no concurrent access — which is
    // the case here because no other test reads or mutates NO_COLOR.
    let original = std::env::var_os("NO_COLOR");

    // 1. Unset → no_color() is false, colors pass through unchanged.
    unsafe {
        std::env::remove_var("NO_COLOR");
    }
    assert!(!no_color(), "unset NO_COLOR must be no_color()==false");
    let with_color = Style::default().maybe_fg(Color::Red).maybe_bg(Color::Blue);
    assert_eq!(with_color.fg, Some(Color::Red));
    assert_eq!(with_color.bg, Some(Color::Blue));

    // 2. Empty string → treated as "not set" per the spec (no-color.org).
    unsafe {
        std::env::set_var("NO_COLOR", "");
    }
    assert!(!no_color(), "empty NO_COLOR must behave as unset");

    // 3. Any non-empty value → colors are suppressed, modifiers preserved.
    unsafe {
        std::env::set_var("NO_COLOR", "1");
    }
    assert!(no_color(), "non-empty NO_COLOR must be no_color()==true");
    let stripped = Style::default()
        .maybe_fg(Color::Red)
        .maybe_bg(Color::Blue)
        .add_modifier(Modifier::BOLD);
    assert!(stripped.fg.is_none(), "NO_COLOR must drop fg");
    assert!(stripped.bg.is_none(), "NO_COLOR must drop bg");
    assert!(
        stripped.add_modifier.contains(Modifier::BOLD),
        "modifiers like BOLD must survive NO_COLOR (they work on mono terms)",
    );

    // Restore original state so this test doesn't leak into the rest of
    // the test binary.
    // SAFETY: same justification as the earlier mutations.
    unsafe {
        match original {
            Some(v) => std::env::set_var("NO_COLOR", v),
            None => std::env::remove_var("NO_COLOR"),
        }
    }
}
