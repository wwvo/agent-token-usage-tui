//! Sidecar tests for TUI render helpers (pure formatting).

use super::format_int;
use super::format_usd;
use super::truncate;
use pretty_assertions::assert_eq;

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
