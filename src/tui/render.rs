//! Rendering functions for each TUI view.
//!
//! Kept free of `App` mutation so we can render the same state to different
//! backends (e.g. ratatui test backend) without side effects.

use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Margin;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Bar;
use ratatui::widgets::BarChart;
use ratatui::widgets::BarGroup;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Cell;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Row;
use ratatui::widgets::Scrollbar;
use ratatui::widgets::ScrollbarOrientation;
use ratatui::widgets::ScrollbarState;
use ratatui::widgets::Table;
use ratatui::widgets::TableState;

use super::app::App;
use super::app::View;
use crate::storage::DailyTotal;
use crate::storage::ModelTally;
use crate::storage::SessionSummary;
use crate::storage::SourceTally;
use crate::storage::WindsurfSessionSummary;

// ---- Color policy ---------------------------------------------------------

/// Honor the community `NO_COLOR` convention (<https://no-color.org>).
///
/// Present *and non-empty* → suppress every foreground / background color.
/// `Modifier::BOLD` and friends are untouched; they carry meaning on
/// monochrome terminals too (and the convention explicitly allows bold /
/// inverse). Queried per-call rather than cached so tests can toggle the
/// env var between scenarios without an OnceLock reset dance.
fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

/// Extension methods that route `.fg` / `.bg` through the NO_COLOR gate.
///
/// The motivation is ergonomic: using `style.maybe_fg(Color::X)` keeps the
/// existing builder-style composition readable, versus sprinkling
/// `if !no_color() { ... }` around every table definition.
trait StyleExt {
    fn maybe_fg(self, c: Color) -> Self;
    fn maybe_bg(self, c: Color) -> Self;
}

impl StyleExt for Style {
    fn maybe_fg(self, c: Color) -> Self {
        if no_color() { self } else { self.fg(c) }
    }
    fn maybe_bg(self, c: Color) -> Self {
        if no_color() { self } else { self.bg(c) }
    }
}

// ---- Public entry ---------------------------------------------------------

/// Render the whole frame. Pure w.r.t. `app` — we only *read* state.
pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let size = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // tab bar
            Constraint::Min(0),    // main body
            Constraint::Length(2), // footer
        ])
        .split(size);

    draw_tabs(frame, chunks[0], app);
    match app.view {
        View::Overview => draw_overview(frame, chunks[1], app),
        View::Trend => draw_trend(frame, chunks[1], app),
        View::Sessions => draw_sessions(frame, chunks[1], app),
        View::Models => draw_models(frame, chunks[1], app),
        View::Cascades => draw_cascades(frame, chunks[1], app),
    }
    draw_footer(frame, chunks[2], app);
}

// ---- Tab bar --------------------------------------------------------------

fn draw_tabs(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let mut spans: Vec<Span<'_>> = Vec::new();
    for (idx, v) in View::all().iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw(" "));
        }
        let label = format!(" {} {} ", idx + 1, v.title());
        let style = if *v == app.view {
            Style::default()
                .maybe_fg(Color::Black)
                .maybe_bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().maybe_fg(Color::Gray)
        };
        spans.push(Span::styled(label, style));
    }
    let para = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(para, area);
}

// ---- Footer ---------------------------------------------------------------

fn draw_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    // `w` is Trend-only and `c` jumps to Cascades; advertising both in
    // every view's footer is still the simplest path — the hint bar is
    // already a dense pile of keybindings and users don't expect every
    // key to work everywhere.
    let hints = "q/Esc quit  1/2/3/4 view  c cascades  j/k move  g/G top/bottom  Enter drill-in  r refresh  w window";
    let msg = app.footer.as_deref().unwrap_or("");
    let line = Line::from(vec![
        Span::styled(hints, Style::default().maybe_fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(msg, Style::default().maybe_fg(Color::Yellow)),
    ]);
    frame.render_widget(Paragraph::new(line).alignment(Alignment::Left), area);
}

// ---- Overview view --------------------------------------------------------

fn draw_overview(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let header = Row::new(vec![
        "Source", "Records", "Prompts", "Sessions", "Tokens", "Cost USD", "Last",
    ])
    .style(Style::default().add_modifier(Modifier::BOLD))
    .height(1);

    let rows: Vec<Row<'_>> = app.overview_rows.iter().map(|t| overview_row(t)).collect();

    let mut state = TableState::default();
    state.select(Some(
        app.selected_overview.min(rows.len().saturating_sub(1)),
    ));

    let widths = [
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(14),
        Constraint::Length(12),
        Constraint::Min(20),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Overview "))
        .row_highlight_style(
            Style::default()
                .maybe_bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, &mut state);
}

fn overview_row(t: &SourceTally) -> Row<'_> {
    let last = t
        .last_activity
        .map(|ts| ts.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "—".to_string());
    Row::new(vec![
        Cell::from(t.source.to_string()),
        Cell::from(format_int(t.records)),
        Cell::from(format_int(t.prompts)),
        Cell::from(format_int(t.sessions)),
        Cell::from(format_int(t.total_tokens())),
        Cell::from(format_usd(t.total_cost_usd)),
        Cell::from(last),
    ])
}

// ---- Sessions view --------------------------------------------------------

fn draw_sessions(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let header = Row::new(vec![
        "Time", "Source", "Session", "Project", "Prompts", "Records", "Tokens", "Cost USD",
    ])
    .style(Style::default().add_modifier(Modifier::BOLD))
    .height(1);

    let rows: Vec<Row<'_>> = app.sessions_rows.iter().map(|s| session_row(s)).collect();
    let row_count = rows.len();

    let mut state = TableState::default();
    state.select(Some(app.selected_sessions.min(row_count.saturating_sub(1))));

    let widths = [
        Constraint::Length(20),
        Constraint::Length(10),
        Constraint::Length(18),
        Constraint::Length(24),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(12),
        Constraint::Length(12),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Sessions "))
        .row_highlight_style(
            Style::default()
                .maybe_bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, &mut state);
    draw_scrollbar(frame, area, row_count, app.selected_sessions);
}

/// Paint a vertical scrollbar inside the right edge of `area`.
///
/// Rendered only when `content_len > 0` — ratatui silently no-ops on empty
/// ranges, but guarding explicitly saves an allocation and keeps the intent
/// obvious. We use the selected row as `position`; this diverges slightly
/// from ratatui's internal `TableState::offset` when the viewport is
/// partially filled, but the visual is close enough to act as a "where am I
/// in the list" indicator without tracking dual state.
fn draw_scrollbar(frame: &mut Frame<'_>, area: Rect, content_len: usize, position: usize) {
    if content_len == 0 {
        return;
    }
    let mut state = ScrollbarState::new(content_len).position(position);
    // `Margin { vertical: 1, horizontal: 0 }` keeps the track inside the
    // block's top/bottom borders; the bar sits flush with the right border.
    let inner = area.inner(Margin {
        vertical: 1,
        horizontal: 0,
    });
    let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("▲"))
        .end_symbol(Some("▼"));
    frame.render_stateful_widget(bar, inner, &mut state);
}

fn session_row(s: &SessionSummary) -> Row<'_> {
    Row::new(vec![
        Cell::from(s.start_time.format("%Y-%m-%d %H:%M").to_string()),
        Cell::from(s.source.to_string()),
        Cell::from(truncate(&s.session_id, 18)),
        Cell::from(truncate(&s.project, 24)),
        Cell::from(format_int(s.prompts)),
        Cell::from(format_int(s.records)),
        Cell::from(format_int(s.total_tokens)),
        Cell::from(format_usd(s.total_cost_usd)),
    ])
}

// ---- Cascades view --------------------------------------------------------

fn draw_cascades(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let header = Row::new(vec![
        "Last Seen",
        "Cascade",
        "Summary",
        "Workspace",
        "Model",
        "Turns",
        "Tokens",
        "Cost USD",
    ])
    .style(Style::default().add_modifier(Modifier::BOLD))
    .height(1);

    let rows: Vec<Row<'_>> = app.cascade_rows.iter().map(|c| cascade_row(c)).collect();
    let row_count = rows.len();

    let mut state = TableState::default();
    state.select(Some(app.selected_cascades.min(row_count.saturating_sub(1))));

    // Width budget reasoning: Last Seen (16) + Cascade (12, truncated
    // UUID) + Summary (flex, Min) + Workspace (28) + Model (16) + Turns
    // (6) + Tokens (12) + Cost (12) fits a typical 120-col terminal with
    // Summary taking the slack. Narrower terminals clip the flex column
    // gracefully.
    let widths = [
        Constraint::Length(16),
        Constraint::Length(12),
        Constraint::Min(24),
        Constraint::Length(28),
        Constraint::Length(16),
        Constraint::Length(6),
        Constraint::Length(12),
        Constraint::Length(12),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Windsurf Cascades "),
        )
        .row_highlight_style(
            Style::default()
                .maybe_bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, &mut state);
    draw_scrollbar(frame, area, row_count, app.selected_cascades);
}

fn cascade_row(c: &WindsurfSessionSummary) -> Row<'_> {
    // `Last Seen` is always populated; we format to minute precision for
    // screen density. `Summary` is the Cascade's human-readable title —
    // the whole reason this view exists — so it gets the flex column.
    Row::new(vec![
        Cell::from(c.last_seen.format("%Y-%m-%d %H:%M").to_string()),
        Cell::from(truncate(&c.cascade_id, 12)),
        Cell::from(truncate(&c.summary, 40)),
        Cell::from(truncate(&c.workspace, 28)),
        Cell::from(truncate(&c.last_model, 16)),
        Cell::from(format_int(c.turns)),
        Cell::from(format_int(c.total_tokens)),
        Cell::from(format_usd(c.total_cost_usd)),
    ])
}

// ---- Models view ----------------------------------------------------------

fn draw_models(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let header = Row::new(vec!["Source", "Model", "Records", "Tokens", "Cost USD"])
        .style(Style::default().add_modifier(Modifier::BOLD))
        .height(1);

    let rows: Vec<Row<'_>> = app.model_rows.iter().map(|m| model_row(m)).collect();

    let mut state = TableState::default();
    state.select(Some(app.selected_models.min(rows.len().saturating_sub(1))));

    let widths = [
        Constraint::Length(10),
        Constraint::Min(20),
        Constraint::Length(10),
        Constraint::Length(14),
        Constraint::Length(12),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Models "))
        .row_highlight_style(
            Style::default()
                .maybe_bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, &mut state);
}

fn model_row(m: &ModelTally) -> Row<'_> {
    Row::new(vec![
        Cell::from(m.source.to_string()),
        Cell::from(truncate(&m.model, 36)),
        Cell::from(format_int(m.records)),
        Cell::from(format_int(m.total_tokens)),
        Cell::from(format_usd(m.total_cost_usd)),
    ])
}

// ---- Trend view -----------------------------------------------------------

fn draw_trend(frame: &mut Frame<'_>, area: Rect, app: &App) {
    // Split: top = cost bar chart (labeled, discrete per-day bars), bottom
    // = per-day detail table. The bar chart trades the sparkline's
    // continuous-curve aesthetic for inline numeric labels + dates — a
    // better fit for terminal-width-constrained data where each day is
    // already a discrete bucket.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9), // bar chart block (borders + 7 bar rows)
            Constraint::Min(0),    // data table fills the rest
        ])
        .split(area);

    draw_trend_barchart(frame, chunks[0], &app.trend_rows);
    draw_trend_table(frame, chunks[1], &app.trend_rows);
}

fn draw_trend_barchart(frame: &mut Frame<'_>, area: Rect, rows: &[DailyTotal]) {
    // Scale cost (USD) by 10_000 so a sub-cent day still registers as a
    // non-zero bar; same trick the old sparkline used. We deliberately
    // chart cost — not tokens — because cost is the one quantity users
    // compare across models and days.
    //
    // Per-bar label format `MM-DD` (5 chars): month+day is enough context
    // for a 7-or-30-day window; year would only become necessary if we
    // ever extended the window across a year boundary, which we don't.
    let labels: Vec<String> = rows
        .iter()
        .map(|r| r.date.format("%m-%d").to_string())
        .collect();
    let values: Vec<u64> = rows
        .iter()
        .map(|r| (r.total_cost_usd * 10_000.0).max(0.0) as u64)
        .collect();

    let (total_cost, total_tokens) = rows.iter().fold((0.0_f64, 0_i64), |(c, t), r| {
        (c + r.total_cost_usd, t + r.total_tokens)
    });

    let title = format!(
        " Trend (last {} days)   total: {} tok / {}   [w] window ",
        rows.len(),
        format_int(total_tokens),
        format_usd(total_cost),
    );

    // Auto-size the bar width + gap to the available inner area. The block
    // borders eat 2 cols; after that we want each bar-plus-gap to evenly
    // partition the remaining width. Clamp bar_width to [1, 5] so very wide
    // terminals don't produce absurdly fat bars and very narrow ones still
    // fit at least one glyph per bar.
    let n = u16::try_from(rows.len()).unwrap_or(u16::MAX).max(1);
    let inner_width = area.width.saturating_sub(2);
    let per_bar = (inner_width / n).max(1);
    let bar_gap: u16 = if per_bar >= 3 { 1 } else { 0 };
    let bar_width = per_bar.saturating_sub(bar_gap).clamp(1, 5);

    // Build Bar objects individually so labels survive the `&str` lifetime
    // tangle: `BarChart::data(BarGroup)` takes owned `Bar`s whose `label`
    // field is a `Line<'static>` via `Line::raw(String)`.
    let bars: Vec<Bar<'_>> = labels
        .into_iter()
        .zip(values.iter().copied())
        .map(|(label, v)| {
            Bar::default()
                .value(v)
                .label(Line::raw(label))
                .style(Style::default().maybe_fg(Color::Cyan))
        })
        .collect();

    let chart = BarChart::default()
        .block(Block::default().borders(Borders::ALL).title(title))
        .data(BarGroup::default().bars(&bars))
        .bar_width(bar_width)
        .bar_gap(bar_gap)
        .value_style(
            Style::default()
                .maybe_fg(Color::Black)
                .maybe_bg(Color::Cyan),
        );

    frame.render_widget(chart, area);
}

fn draw_trend_table(frame: &mut Frame<'_>, area: Rect, rows: &[DailyTotal]) {
    let header = Row::new(vec!["Date", "Records", "Tokens", "Cost USD"])
        .style(Style::default().add_modifier(Modifier::BOLD))
        .height(1);

    let data_rows: Vec<Row<'_>> = rows
        .iter()
        .map(|r| {
            Row::new(vec![
                Cell::from(r.date.format("%Y-%m-%d").to_string()),
                Cell::from(format_int(r.records)),
                Cell::from(format_int(r.total_tokens)),
                Cell::from(format_usd(r.total_cost_usd)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Length(14),
        Constraint::Length(12),
    ];

    let table = Table::new(data_rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Daily "));

    frame.render_widget(table, area);
}

// ---- Formatting helpers ---------------------------------------------------

/// Thousands-separated integer formatter (no_std-friendly path).
fn format_int(n: i64) -> String {
    let mut s = n.to_string();
    // Insert ',' every three digits from the right.
    let neg = s.starts_with('-');
    let digits_start = usize::from(neg);
    let digits = s[digits_start..].to_string();
    let mut with_sep = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            with_sep.push(',');
        }
        with_sep.push(ch);
    }
    let rev: String = with_sep.chars().rev().collect();
    s.truncate(digits_start);
    s.push_str(&rev);
    s
}

fn format_usd(v: f64) -> String {
    format!("${v:.4}")
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
