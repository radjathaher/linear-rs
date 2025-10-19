use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::tui::app::{App, Focus};

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    render_team_list(frame, panels[0], app);
    render_state_list(frame, panels[1], app);
}

fn render_team_list(frame: &mut Frame, area: Rect, app: &App) {
    let mut items = Vec::new();
    items.push(ListItem::new("All teams"));
    for team in app.teams() {
        items.push(ListItem::new(format!("{}  {}", team.key, team.name)));
    }
    let mut state = ListState::default();
    let selected = app.team_index().map(|idx| idx + 1).unwrap_or(0);
    state.select(Some(selected));
    let highlight = if matches!(app.focus(), Focus::Teams) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let list = List::new(items)
        .block(Block::default().title("Teams").borders(Borders::ALL))
        .highlight_style(highlight);
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_state_list(frame: &mut Frame, area: Rect, app: &App) {
    let mut items = Vec::new();
    items.push(ListItem::new("All states"));
    for state in app.states() {
        items.push(ListItem::new(state.name.clone()));
    }
    let mut list_state = ListState::default();
    let selected = app.state_index().map(|idx| idx + 1).unwrap_or(0);
    list_state.select(Some(selected));
    let highlight = if matches!(app.focus(), Focus::States) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let list = List::new(items)
        .block(Block::default().title("States").borders(Borders::ALL))
        .highlight_style(highlight);
    frame.render_stateful_widget(list, area, &mut list_state);
}
