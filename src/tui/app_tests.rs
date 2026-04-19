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

#[test]
fn default_view_is_overview() {
    let app = new_app();
    assert_eq!(app.view, View::Overview);
    assert!(!app.should_quit);
}

#[test]
fn q_sets_should_quit() {
    let mut app = new_app();
    assert!(app.on_key(press(KeyCode::Char('q'))));
    assert!(app.should_quit);
}

#[test]
fn esc_sets_should_quit() {
    let mut app = new_app();
    assert!(app.on_key(press(KeyCode::Esc)));
    assert!(app.should_quit);
}

#[test]
fn number_keys_switch_views() {
    let mut app = new_app();
    app.on_key(press(KeyCode::Char('2')));
    assert_eq!(app.view, View::Sessions);
    app.on_key(press(KeyCode::Char('3')));
    assert_eq!(app.view, View::Models);
    app.on_key(press(KeyCode::Char('1')));
    assert_eq!(app.view, View::Overview);
}

#[test]
fn key_release_events_are_ignored() {
    let mut app = new_app();
    // A release of 'q' must NOT quit; only the press does.
    assert!(!app.on_key(release(KeyCode::Char('q'))));
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
}

#[test]
fn jk_navigation_on_overview_clamps_at_edges() {
    let mut app = new_app();
    app.refresh(); // 5 rows
    assert_eq!(app.selected_overview, 0);

    // Four 'j' presses move us to the last row.
    for _ in 0..10 {
        app.on_key(press(KeyCode::Char('j')));
    }
    assert_eq!(app.selected_overview, 4);

    // 'k' moves us back up.
    app.on_key(press(KeyCode::Char('k')));
    assert_eq!(app.selected_overview, 3);

    // 'g' jumps to the top.
    app.on_key(press(KeyCode::Char('g')));
    assert_eq!(app.selected_overview, 0);

    // 'G' jumps to the bottom.
    app.on_key(press(KeyCode::Char('G')));
    assert_eq!(app.selected_overview, 4);
}

#[test]
fn r_key_sets_refresh_footer() {
    let mut app = new_app();
    app.on_key(press(KeyCode::Char('r')));
    assert_eq!(app.footer.as_deref(), Some("refreshed"));
}

#[test]
fn enter_on_overview_switches_to_sessions_filtered() {
    // With an empty DB the sessions view stays empty, but we still test the
    // state transitions: view → Sessions, footer set, selection reset.
    let mut app = new_app();
    app.refresh();
    app.on_key(press(KeyCode::Enter));
    assert_eq!(app.view, View::Sessions);
    assert!(
        app.footer
            .as_deref()
            .is_some_and(|s| s.starts_with("filter:"))
    );
}

#[test]
fn selection_does_not_move_on_empty_table() {
    let mut app = new_app();
    // No refresh → overview_rows empty. 'j' must not panic or move.
    app.on_key(press(KeyCode::Char('j')));
    assert_eq!(app.selected_overview, 0);
}

#[test]
fn view_title_is_readable() {
    assert_eq!(View::Overview.title(), "Overview");
    assert_eq!(View::Sessions.title(), "Sessions");
    assert_eq!(View::Models.title(), "Models");
}

#[test]
fn view_all_returns_three_variants() {
    let all = View::all();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0], View::Overview);
    assert_eq!(all[1], View::Sessions);
    assert_eq!(all[2], View::Models);
}
