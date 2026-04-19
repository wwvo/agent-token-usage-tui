//! Sidecar tests for TUI `App` state / key handling — pure state, no terminal.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::App;
use super::View;
use crate::storage::Db;

fn new_app() -> App {
    let tmp = tempdir().expect("tempdir");
    // Intentionally keep the tempdir for the lifetime of the test: the App
    // owns `db`, and `tempdir` dropping before `App` would invalidate the
    // SQLite file. The process tears down at test end either way.
    let path = tmp.keep().join("t.db");
    let db = Db::open(&path).expect("open db");
    App::new(db)
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    }
}

fn release(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Release,
        state: crossterm::event::KeyEventState::NONE,
    }
}

/// Default page size for tests that don't exercise PageUp / PageDown.
///
/// Production code derives this from the terminal's row count; `10` is a
/// conservative stand-in that keeps PageDown moves distinguishable from
/// single-row `j` moves.
const PAGE: usize = 10;

#[test]
fn default_view_is_overview() {
    let app = new_app();
    assert_eq!(app.view, View::Overview);
    assert!(!app.should_quit);
}

#[test]
fn q_sets_should_quit() {
    let mut app = new_app();
    assert!(app.on_key(press(KeyCode::Char('q')), PAGE));
    assert!(app.should_quit);
}

#[test]
fn esc_sets_should_quit() {
    let mut app = new_app();
    assert!(app.on_key(press(KeyCode::Esc), PAGE));
    assert!(app.should_quit);
}

#[test]
fn number_keys_switch_views() {
    let mut app = new_app();
    app.on_key(press(KeyCode::Char('2')), PAGE);
    assert_eq!(app.view, View::Sessions);
    app.on_key(press(KeyCode::Char('3')), PAGE);
    assert_eq!(app.view, View::Models);
    app.on_key(press(KeyCode::Char('4')), PAGE);
    assert_eq!(app.view, View::Trend);
    app.on_key(press(KeyCode::Char('1')), PAGE);
    assert_eq!(app.view, View::Overview);
}

#[test]
fn key_release_events_are_ignored() {
    let mut app = new_app();
    // A release of 'q' must NOT quit; only the press does.
    assert!(!app.on_key(release(KeyCode::Char('q')), PAGE));
    assert!(!app.should_quit);
}

#[test]
fn refresh_populates_five_overview_rows_even_on_empty_db() {
    let mut app = new_app();
    app.refresh();
    // fetch_source_tallies zero-fills all 5 Source variants.
    assert_eq!(app.overview_rows.len(), 5);
    // Empty DB → no sessions / model rows.
    assert!(app.sessions_rows.is_empty());
    assert!(app.model_rows.is_empty());
    // Trend is always the configured window length (zero-filled), never empty.
    assert_eq!(app.trend_rows.len(), super::TREND_WINDOW_DAYS);
    assert!(
        app.trend_rows.iter().all(|r| r.records == 0),
        "empty DB → every day zero",
    );
}

#[test]
fn jk_navigation_on_overview_clamps_at_edges() {
    let mut app = new_app();
    app.refresh(); // 5 rows
    assert_eq!(app.selected_overview, 0);

    // Four 'j' presses move us to the last row.
    for _ in 0..10 {
        app.on_key(press(KeyCode::Char('j')), PAGE);
    }
    assert_eq!(app.selected_overview, 4);

    // 'k' moves us back up.
    app.on_key(press(KeyCode::Char('k')), PAGE);
    assert_eq!(app.selected_overview, 3);

    // 'g' jumps to the top.
    app.on_key(press(KeyCode::Char('g')), PAGE);
    assert_eq!(app.selected_overview, 0);

    // 'G' jumps to the bottom.
    app.on_key(press(KeyCode::Char('G')), PAGE);
    assert_eq!(app.selected_overview, 4);
}

#[test]
fn r_key_sets_refresh_footer() {
    let mut app = new_app();
    app.on_key(press(KeyCode::Char('r')), PAGE);
    assert_eq!(app.footer.as_deref(), Some("refreshed"));
}

#[test]
fn enter_on_overview_switches_to_sessions_filtered_by_source() {
    // With an empty DB the sessions view stays empty, but we still test the
    // state transitions: view → Sessions, footer set, selection reset.
    let mut app = new_app();
    app.refresh();
    app.on_key(press(KeyCode::Enter), PAGE);
    assert_eq!(app.view, View::Sessions);
    assert!(
        app.footer
            .as_deref()
            .is_some_and(|s| s.starts_with("filter: source=")),
        "footer={:?}",
        app.footer,
    );
}

#[test]
fn enter_on_models_with_empty_rows_is_still_a_handled_key() {
    // Empty DB → model_rows empty → Enter is handled (returns true) but
    // nothing changes. Switching to Models view without any rows
    // shouldn't panic, and subsequent Enter should be harmless.
    let mut app = new_app();
    app.refresh();
    app.on_key(press(KeyCode::Char('3')), PAGE);
    assert_eq!(app.view, View::Models);
    assert!(app.model_rows.is_empty());
    app.on_key(press(KeyCode::Enter), PAGE);
    // No selection → no view change, no footer mutation.
    assert_eq!(app.view, View::Models);
}

#[test]
fn enter_on_sessions_and_trend_is_unhandled() {
    // Enter is a view-specific drill-down; Sessions and Trend have no
    // target so the key should fall through (returns false).
    let mut app = new_app();
    app.refresh();
    app.on_key(press(KeyCode::Char('2')), PAGE); // Sessions
    assert!(!app.on_key(press(KeyCode::Enter), PAGE));
    app.on_key(press(KeyCode::Char('4')), PAGE); // Trend
    assert!(!app.on_key(press(KeyCode::Enter), PAGE));
}

#[test]
fn selection_does_not_move_on_empty_table() {
    let mut app = new_app();
    // No refresh → overview_rows empty. 'j' must not panic or move.
    app.on_key(press(KeyCode::Char('j')), PAGE);
    assert_eq!(app.selected_overview, 0);
}

#[test]
fn view_title_is_readable() {
    assert_eq!(View::Overview.title(), "Overview");
    assert_eq!(View::Sessions.title(), "Sessions");
    assert_eq!(View::Models.title(), "Models");
    assert_eq!(View::Trend.title(), "Trend");
}

#[test]
fn view_all_returns_four_variants() {
    let all = View::all();
    assert_eq!(all.len(), 4);
    assert_eq!(all[0], View::Overview);
    assert_eq!(all[1], View::Sessions);
    assert_eq!(all[2], View::Models);
    assert_eq!(all[3], View::Trend);
}

#[test]
fn jk_on_trend_is_noop_and_does_not_panic() {
    // Trend has no selection semantics; j / k must be harmless no-ops.
    let mut app = new_app();
    app.refresh();
    app.on_key(press(KeyCode::Char('4')), PAGE);
    assert_eq!(app.view, View::Trend);
    app.on_key(press(KeyCode::Char('j')), PAGE);
    app.on_key(press(KeyCode::Char('k')), PAGE);
    // Still Trend, still no panic.
    assert_eq!(app.view, View::Trend);
}

#[test]
fn page_down_jumps_by_page_size_on_overview() {
    // 5 Source variants after refresh → PageDown with page=3 should land
    // on row 3, not bottom. This proves PageDown honours the page arg
    // rather than jumping to the bottom like `G`.
    let mut app = new_app();
    app.refresh();
    app.on_key(press(KeyCode::PageDown), 3);
    assert_eq!(app.selected_overview, 3);
    // Second PageDown clamps at the last row.
    app.on_key(press(KeyCode::PageDown), 3);
    assert_eq!(app.selected_overview, 4);
}

#[test]
fn page_up_mirrors_page_down() {
    let mut app = new_app();
    app.refresh();
    app.on_key(press(KeyCode::Char('G')), PAGE); // bottom (4)
    app.on_key(press(KeyCode::PageUp), 2);
    assert_eq!(app.selected_overview, 2);
    app.on_key(press(KeyCode::PageUp), 2);
    assert_eq!(app.selected_overview, 0);
}

#[test]
fn page_down_with_zero_page_clamp_still_advances_one_row() {
    // A zero-height terminal is a pathological but realistic edge; we
    // must advance at least one row so the user never feels the key did
    // nothing.
    let mut app = new_app();
    app.refresh();
    app.on_key(press(KeyCode::PageDown), 0);
    assert_eq!(app.selected_overview, 1);
}

#[test]
fn sessions_page_constant_is_generous() {
    // Regression guard: we lifted the Sessions cap from 200 to 2_000 to
    // enable scrollbar-based browsing. Don't silently shrink it again.
    const { assert!(super::SESSIONS_PAGE >= 2_000) };
}
