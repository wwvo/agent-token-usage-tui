//! TUI application state and key handling.
//!
//! State is deliberately kept small and pure: no terminal I/O here, no
//! rendering. That lets us unit-test every keybinding without spinning up a
//! real terminal. Rendering lives in [`crate::tui::render`]; the event loop
//! that wires state → rendering → crossterm is in [`crate::tui`].

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;

use crate::domain::Source;
use crate::storage::DailyTotal;
use crate::storage::Db;
use crate::storage::ModelTally;
use crate::storage::SessionSummary;
use crate::storage::SourceTally;

/// Which screen the user is currently looking at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum View {
    /// Source coverage table.
    Overview,
    /// Recent sessions, newest first.
    Sessions,
    /// Per-model rollup, sorted by cost descending.
    Models,
    /// 7-day cost + tokens sparkline.
    Trend,
}

impl View {
    /// Title shown in the tab bar.
    #[must_use]
    pub const fn title(&self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Sessions => "Sessions",
            Self::Models => "Models",
            Self::Trend => "Trend",
        }
    }

    /// Declared order for the tab bar; also drives the `1 / 2 / 3 / 4` shortcut keys.
    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::Overview, Self::Sessions, Self::Models, Self::Trend]
    }
}

/// How many sessions we pull into the sessions view on each refresh.
///
/// `2_000` is a deliberate compromise: large enough that "scroll through
/// the last quarter's work" is a viewport-scroll rather than a re-query,
/// small enough that the initial SELECT stays under a millisecond on
/// typical SQLite pages. True pagination (fetch-on-scroll) is a future
/// optimization; for now the scrollbar + PageUp/PageDown keys make 2k
/// rows navigable.
pub const SESSIONS_PAGE: usize = 2_000;

/// Window (in UTC days) for the Trend view.
///
/// 7 maps to a full week in one glance; daily buckets keep the sparkline
/// readable on narrow terminals.
pub const TREND_WINDOW_DAYS: usize = 7;

/// Top-level TUI state.
pub struct App {
    pub db: Db,
    pub view: View,
    pub overview_rows: Vec<SourceTally>,
    pub sessions_rows: Vec<SessionSummary>,
    pub model_rows: Vec<ModelTally>,
    pub trend_rows: Vec<DailyTotal>,
    pub selected_overview: usize,
    pub selected_sessions: usize,
    pub selected_models: usize,
    pub should_quit: bool,
    /// Optional transient footer message (key hints, errors).
    pub footer: Option<String>,
}

impl App {
    /// Create an empty app tied to `db`. Data is populated by [`App::refresh`].
    pub fn new(db: Db) -> Self {
        Self {
            db,
            view: View::Overview,
            overview_rows: Vec::new(),
            sessions_rows: Vec::new(),
            model_rows: Vec::new(),
            trend_rows: Vec::new(),
            selected_overview: 0,
            selected_sessions: 0,
            selected_models: 0,
            should_quit: false,
            footer: None,
        }
    }

    /// Pull every table's data from the DB.
    ///
    /// Called once at startup and again whenever the user hits `r`. Any SQL
    /// error is surfaced in [`App::footer`] — not fatal; the TUI keeps
    /// running with whatever data it already has.
    pub fn refresh(&mut self) {
        match self.db.fetch_source_tallies() {
            Ok(rows) => self.overview_rows = rows,
            Err(e) => self.footer = Some(format!("refresh overview: {e:#}")),
        }
        match self.db.fetch_recent_sessions(None, SESSIONS_PAGE) {
            Ok(rows) => {
                self.sessions_rows = rows;
                self.clamp_selection();
            }
            Err(e) => self.footer = Some(format!("refresh sessions: {e:#}")),
        }
        match self.db.fetch_model_tallies(None) {
            Ok(rows) => self.model_rows = rows,
            Err(e) => self.footer = Some(format!("refresh models: {e:#}")),
        }
        match self.db.fetch_daily_totals(TREND_WINDOW_DAYS) {
            Ok(rows) => self.trend_rows = rows,
            Err(e) => self.footer = Some(format!("refresh trend: {e:#}")),
        }
    }

    /// Which source the user has highlighted in Overview (used to filter
    /// the Sessions view on `Enter`).
    #[must_use]
    pub fn selected_overview_source(&self) -> Option<Source> {
        self.overview_rows
            .get(self.selected_overview)
            .map(|t| t.source)
    }

    /// Apply one keypress to the state machine.
    ///
    /// `page_size` is the caller's best estimate of how many data rows fit
    /// into the active view's viewport — the TUI loop computes it from
    /// the terminal's current row count and passes it here so PageUp /
    /// PageDown scroll **exactly one visible page**. A sensible fallback
    /// is `10` (roughly the height of a small terminal minus chrome).
    ///
    /// Returns `true` if the event was handled; `false` for genuinely unknown
    /// keys (we don't currently use the boolean but leaving it lets tests
    /// assert we did not accidentally consume an un-handled key).
    pub fn on_key(&mut self, key: KeyEvent, page_size: usize) -> bool {
        // Skip key-release events on Windows (crossterm emits both by default).
        if matches!(key.kind, KeyEventKind::Release) {
            return false;
        }

        // Clamp to 1 so PageDown always moves at least one row even on a
        // zero-height terminal (e.g. when a unit test passes `0`).
        let page = page_size.max(1);

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
                true
            }
            KeyCode::Char('1') => {
                self.view = View::Overview;
                true
            }
            KeyCode::Char('2') => {
                self.view = View::Sessions;
                true
            }
            KeyCode::Char('3') => {
                self.view = View::Models;
                true
            }
            KeyCode::Char('4') => {
                self.view = View::Trend;
                true
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                true
            }
            KeyCode::PageDown => {
                // saturating cast: page fits in i32 for every realistic
                // terminal (> 2_000 rows is unreachable).
                self.move_selection(i32::try_from(page).unwrap_or(i32::MAX));
                true
            }
            KeyCode::PageUp => {
                self.move_selection(-i32::try_from(page).unwrap_or(i32::MAX));
                true
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.set_selection(0);
                true
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.set_selection(usize::MAX);
                true
            }
            KeyCode::Char('r') => {
                self.refresh();
                self.footer = Some("refreshed".to_string());
                true
            }
            KeyCode::Enter => {
                // Jump from Overview into Sessions filtered by the highlighted source.
                if self.view == View::Overview {
                    if let Some(src) = self.selected_overview_source() {
                        match self.db.fetch_recent_sessions(Some(src), SESSIONS_PAGE) {
                            Ok(rows) => {
                                self.sessions_rows = rows;
                                self.selected_sessions = 0;
                                self.view = View::Sessions;
                                self.footer = Some(format!("filter: {src}"));
                            }
                            Err(e) => self.footer = Some(format!("filter failed: {e:#}")),
                        }
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn current_len(&self) -> usize {
        match self.view {
            View::Overview => self.overview_rows.len(),
            View::Sessions => self.sessions_rows.len(),
            View::Models => self.model_rows.len(),
            // Trend is a chart, not a list — j/k still no-op via zero len.
            View::Trend => 0,
        }
    }

    fn current_selection_mut(&mut self) -> &mut usize {
        match self.view {
            View::Overview => &mut self.selected_overview,
            View::Sessions => &mut self.selected_sessions,
            View::Models => &mut self.selected_models,
            // Unused for Trend but still needs a valid mut ref.
            View::Trend => &mut self.selected_overview,
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let len = self.current_len();
        if len == 0 {
            return;
        }
        let cur = *self.current_selection_mut() as i32;
        let next = (cur + delta).clamp(0, (len - 1) as i32);
        *self.current_selection_mut() = next as usize;
    }

    fn set_selection(&mut self, target: usize) {
        let len = self.current_len();
        if len == 0 {
            return;
        }
        let clamped = target.min(len - 1);
        *self.current_selection_mut() = clamped;
    }

    /// Make sure no selection points past the end of its table.
    fn clamp_selection(&mut self) {
        let max_overview = self.overview_rows.len().saturating_sub(1);
        self.selected_overview = self.selected_overview.min(max_overview);
        let max_sessions = self.sessions_rows.len().saturating_sub(1);
        self.selected_sessions = self.selected_sessions.min(max_sessions);
        let max_models = self.model_rows.len().saturating_sub(1);
        self.selected_models = self.selected_models.min(max_models);
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
