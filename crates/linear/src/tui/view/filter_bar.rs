use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::app::{App, StatusTab};

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(28),
            Constraint::Percentage(27),
            Constraint::Percentage(15),
        ])
        .split(area);

    let team_line = Line::from(vec![
        Span::styled("Team ", Style::default().fg(Color::Gray)),
        Span::raw(app.current_team_label()),
    ]);
    let state_line = Line::from(vec![
        Span::styled("State ", Style::default().fg(Color::Gray)),
        Span::raw(app.current_state_label()),
    ]);
    let filters = Paragraph::new(vec![team_line, state_line])
        .block(Block::default().title("Context").borders(Borders::ALL));
    frame.render_widget(filters, chunks[0]);

    let project_style = if app.project_filter_index().is_some() {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let project_line = Line::from(vec![
        Span::styled("Project ", Style::default().fg(Color::Gray)),
        Span::styled(app.current_project_label(), project_style),
    ]);
    let project_hint = Line::from(vec![
        Span::styled("p", Style::default().fg(Color::Gray)),
        Span::raw(" next  "),
        Span::styled("Shift+p", Style::default().fg(Color::Gray)),
        Span::raw(" prev  "),
        Span::styled("Ctrl+p", Style::default().fg(Color::Gray)),
        Span::raw(" clear"),
    ]);
    let project_widget = Paragraph::new(vec![project_line, project_hint]).block(
        Block::default()
            .title("Project Filter")
            .borders(Borders::ALL),
    );
    frame.render_widget(project_widget, chunks[1]);

    let tabs = StatusTab::all();
    let mut status_spans = Vec::new();
    for (idx, tab) in tabs.iter().enumerate() {
        let label = format!("{} {}", idx + 1, tab.label());
        let style = if *tab == app.status_tab() {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        status_spans.push(Span::styled(label, style));
        if idx + 1 != tabs.len() {
            status_spans.push(Span::raw("  "));
        }
    }
    let status_line = Line::from(status_spans);
    let status_hint = Line::from(vec![
        Span::styled("Ctrl+[", Style::default().fg(Color::Gray)),
        Span::raw(" prev  "),
        Span::styled("Ctrl+]", Style::default().fg(Color::Gray)),
        Span::raw(" next"),
    ]);
    let status_widget = Paragraph::new(vec![status_line, status_hint])
        .block(Block::default().title("Status Tabs").borders(Borders::ALL));
    frame.render_widget(status_widget, chunks[2]);

    let contains_line = Line::from(vec![
        Span::styled("Contains ", Style::default().fg(Color::Gray)),
        Span::raw(app.title_contains().unwrap_or("-")),
    ]);
    let detail_line = Line::from(vec![
        Span::styled("Selected ", Style::default().fg(Color::Gray)),
        Span::raw(
            app.selected_issue()
                .map(|issue| issue.identifier.clone())
                .unwrap_or_else(|| "-".to_string()),
        ),
    ]);
    let actions_line = Line::from(vec![
        Span::styled("o", Style::default().fg(Color::Gray)),
        Span::raw(" overlay  "),
        Span::styled("y", Style::default().fg(Color::Gray)),
        Span::raw(" cycles  "),
        Span::styled("Ctrl+Enter", Style::default().fg(Color::Gray)),
        Span::raw(" automation"),
    ]);
    let search = Paragraph::new(vec![contains_line, detail_line, actions_line])
        .block(Block::default().title("Selection").borders(Borders::ALL));
    frame.render_widget(search, chunks[3]);
}
