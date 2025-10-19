use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::tui::app::App;

const KEYMAP_TEXT: &str = "\
Navigation  j/k or arrow keys move selection\n\
Focus       Tab cycles issues -> teams -> states\n\
Refresh     r reload issues  c clear filters\n\
Project     p next  Shift+p prev  Ctrl+p clear  o overlay\n\
Status      1 Todo 2 Doing 3 Done 4 All  Ctrl+[ prev  Ctrl+] next\n\
Filters     / contains filter  :team|:state|:project|:status\n\
Paging      ] next page  [ previous page  :page <n|next|prev>\n\
Jump        view next|prev|first|last|<key>\n\
Command     : enter palette  Esc exits palette\n\
Cycles      y show team cycles\n\
Automation  Ctrl+Enter run CLI agent\n\
Help        ? toggle overlay  :help command\n\
Quit        q or Esc";

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    let keymap = Paragraph::new(KEYMAP_TEXT)
        .block(Block::default().title("Keymap").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    frame.render_widget(keymap, chunks[0]);

    if !app.palette_active() {
        return;
    }

    frame.render_widget(Clear, chunks[1]);

    let prompt = Paragraph::new(format!(":{}", app.palette_input()))
        .style(Style::default().fg(Color::Yellow));
    frame.render_widget(prompt, chunks[1]);

    let suggestions_lines: Vec<Line> = app.palette_suggestions();
    let history_lines: Vec<Line> = app
        .palette_history()
        .iter()
        .rev()
        .take(5)
        .map(|entry| Line::from(entry.clone()))
        .collect();

    let mut overlay_y = chunks[1].y.saturating_sub(1);

    if !history_lines.is_empty() {
        let history_height = history_lines.len() as u16;
        let history_area = Rect {
            x: chunks[1].x,
            y: chunks[1].y.saturating_sub(history_height + 1),
            width: chunks[1].width,
            height: history_height,
        };
        let widget = Paragraph::new(history_lines)
            .block(Block::default().title("History").borders(Borders::NONE))
            .style(Style::default().fg(Color::Gray));
        frame.render_widget(widget, history_area);
        overlay_y = history_area.y;
    }

    if !suggestions_lines.is_empty() {
        let suggestions_height = suggestions_lines.len() as u16;
        let suggestions_area = Rect {
            x: chunks[1].x,
            y: overlay_y.saturating_sub(suggestions_height),
            width: chunks[1].width,
            height: suggestions_height,
        };
        let widget = Paragraph::new(suggestions_lines)
            .block(Block::default().title("Suggestions").borders(Borders::NONE))
            .style(Style::default().fg(Color::Gray));
        frame.render_widget(widget, suggestions_area);
    }
}
