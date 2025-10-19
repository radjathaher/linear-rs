use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::tui::app::App;

mod bottom;
mod filter_bar;
mod overlays;
mod palette;
mod sidebar;
pub mod util;
mod workspace;

pub fn render_app(frame: &mut Frame, app: &App) {
    let frame_size = frame.size();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(8),
        ])
        .split(frame_size);

    filter_bar::render(frame, layout[0], app);

    let content_area = layout[1];
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(1)])
        .split(content_area);

    sidebar::render(frame, content_chunks[0], app);
    workspace::render(frame, content_chunks[1], app);

    bottom::render_filters(frame, layout[2], app);
    bottom::render_status(frame, layout[3], app);
    palette::render(frame, layout[4], app);

    overlays::render(frame, content_chunks[1], app);
}

// helper for future responsive sizing
#[allow(dead_code)]
fn clamp_rect_height(area: Rect, min: u16) -> Rect {
    Rect {
        height: area.height.max(min),
        ..area
    }
}
