use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;

pub fn render_filters(frame: &mut Frame, area: Rect, app: &App) {
    let widget = Paragraph::new(app.filters_text()).style(Style::default().fg(Color::Gray));
    frame.render_widget(widget, area);
}

pub fn render_status(frame: &mut Frame, area: Rect, app: &App) {
    let widget = Paragraph::new(app.status_text()).style(Style::default().fg(Color::Cyan));
    frame.render_widget(widget, area);
}
