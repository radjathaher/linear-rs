use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use textwrap::wrap;

use crate::tui::app::{App, Focus};
use crate::tui::view::util::issue_list_line;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    render_issue_list(frame, chunks[0], app);
    render_detail(frame, chunks[1], app);
}

fn render_issue_list(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = if app.issues().is_empty() {
        vec![ListItem::new("No issues loaded")]
    } else {
        app.issues()
            .iter()
            .map(|issue| {
                let line = issue_list_line(issue, app.title_contains());
                ListItem::new(line)
            })
            .collect()
    };

    let mut state = ListState::default();
    if !app.issues().is_empty() {
        state.select(Some(app.selected_index()));
    }

    let highlight = if matches!(app.focus(), Focus::Issues) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let list = List::new(items)
        .block(Block::default().title("Issues").borders(Borders::ALL))
        .highlight_style(highlight);
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().title("Details").borders(Borders::ALL);
    let text = if let Some(issue) = app.detail() {
        let width = area.width.saturating_sub(2).max(20) as usize;
        let description = issue.description.as_deref().unwrap_or("(no description)");
        let mut lines = wrap(description, width)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        lines.push(String::new());
        lines.push(format!(
            "State: {}",
            issue.state.as_ref().map(|s| s.name.as_str()).unwrap_or("-")
        ));
        lines.push(format!(
            "Priority: {}",
            issue
                .priority
                .map(|p| p.to_string())
                .unwrap_or_else(|| "-".into())
        ));
        lines.push(format!("Updated: {}", issue.updated_at.to_rfc3339()));
        lines.join("\n")
    } else {
        "Select an issue to view details".into()
    };
    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}
