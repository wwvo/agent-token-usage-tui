//! Rendering functions for each TUI view.
//!
//! Kept free of `App` mutation so we can render the same state to different
//! backends (e.g. ratatui test backend) without side effects.

use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Cell;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Row;
use ratatui::widgets::Sparkline;
use ratatui::widgets::Table;
use ratatui::widgets::TableState;

use super::app::App;
use super::app::View;
use crate::storage::DailyTotal;
use crate::storage::ModelTally;
use crate::storage::SessionSummary;
use crate::storage::SourceTally;

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
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        spans.push(Span::styled(label, style));
    }
    let para = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(para, area);
}

// ---- Footer ---------------------------------------------------------------

fn draw_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let hints = "q/Esc quit  1/2/3 view  j/k move  g/G top/bottom  Enter drill-in  r refresh";
    let msg = app.footer.as_deref().unwrap_or("");
    let line = Line::from(vec![
        Span::styled(hints, Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(msg, Style::default().fg(Color::Yellow)),
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
                .bg(Color::DarkGray)
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

    let mut state = TableState::default();
    state.select(Some(
        app.selected_sessions.min(rows.len().saturating_sub(1)),
    ));

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
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, &mut state);
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
                .bg(Color::DarkGray)
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
    // Split: top half = cost sparkline, bottom half = per-day detail table.
    // A single `Sparkline` gives a clear "shape of the week" visual; the
    // table below answers "what were the actual numbers" without needing a
    // separate drill-down.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // sparkline block (borders + 5 sample rows)
            Constraint::Min(0),    // data table fills the rest
        ])
        .split(area);

    draw_trend_sparkline(frame, chunks[0], &app.trend_rows);
    draw_trend_table(frame, chunks[1], &app.trend_rows);
}

fn draw_trend_sparkline(frame: &mut Frame<'_>, area: Rect, rows: &[DailyTotal]) {
    // ratatui::Sparkline takes `&[u64]`. We scale cost (USD) by 10_000 so a
    // sub-cent day still registers as a non-zero bar.
    let data: Vec<u64> = rows
        .iter()
        .map(|r| (r.total_cost_usd * 10_000.0).max(0.0) as u64)
        .collect();

    let (total_cost, total_tokens) = rows.iter().fold((0.0_f64, 0_i64), |(c, t), r| {
        (c + r.total_cost_usd, t + r.total_tokens)
    });

    let title = format!(
        " Trend (last {} days)   total: {} tok / {} ",
        rows.len(),
        format_int(total_tokens),
        format_usd(total_cost),
    );

    let spark = Sparkline::default()
        .block(Block::default().borders(Borders::ALL).title(title))
        .data(&data)
        .style(Style::default().fg(Color::Cyan));

    frame.render_widget(spark, area);
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
