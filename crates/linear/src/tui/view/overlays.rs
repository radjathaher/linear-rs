use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::view::util::centered_rect;

pub fn render(frame: &mut Frame, base_area: Rect, app: &App) {
    if app.show_help_overlay() {
        render_help(frame, base_area);
    }
    if app.show_projects_overlay() {
        render_projects(frame, base_area, app);
    }
    if app.show_cycles_overlay() {
        render_cycles(frame, base_area, app);
    }
}

fn render_help(frame: &mut Frame, area: Rect) {
    let overlay_width = area.width.min(80).max(40);
    let overlay_height = area.height.min(12).max(7);
    let overlay_area = centered_rect(overlay_width, overlay_height, area);
    let lines = vec![
        Line::from("Navigation:"),
        Line::from("  j/k or arrow keys  move selection"),
        Line::from("  tab cycles focus between issues/teams/states"),
        Line::from("Actions:"),
        Line::from("  r refresh issues   c clear filters   q exit"),
        Line::from("  ] next page  [ previous page"),
        Line::from("  p next project  Shift+p prev  Ctrl+p clear  o overlay"),
        Line::from("  1/2/3/4 set status tab  Ctrl+[ prev  Ctrl+] next"),
        Line::from("  t / s cycle team or state filters"),
        Line::from("  view next/prev/first/last/<key> jumps to an issue"),
        Line::from("Automation:"),
        Line::from("  Ctrl+Enter trigger CLI agent for active issue"),
        Line::from("Filters:"),
        Line::from("  / opens contains filter  :team/:state/:project/:status"),
        Line::from("  clear resets filters  contains clear drops title filter"),
        Line::from("  help or :help opens this overlay"),
        Line::from("Close help with ? or Esc"),
    ];
    let widget = Paragraph::new(lines)
        .block(Block::default().title("Help").borders(Borders::ALL))
        .style(Style::default().fg(Color::Yellow));
    frame.render_widget(Clear, overlay_area);
    frame.render_widget(widget, overlay_area);
}

fn render_projects(frame: &mut Frame, area: Rect, app: &App) {
    let overlay_width = area.width.min(90).max(50);
    let overlay_height = area.height.min(14).max(8);
    let overlay_area = centered_rect(overlay_width, overlay_height, area);
    let mut lines = Vec::new();
    if app.projects().is_empty() {
        lines.push(Line::from("No projects available"));
    } else {
        lines.push(Line::from("Latest projects (press o or Esc to close):"));
        for project in app.projects() {
            let lead = project
                .lead
                .as_ref()
                .and_then(|l| l.display_name.as_ref().or(l.name.as_ref()))
                .map(|s| s.as_str())
                .unwrap_or("-");
            lines.push(Line::from(format!(
                "{} [{}] lead: {}  target: {}",
                project.name,
                project.state.as_deref().unwrap_or("-"),
                lead,
                project.target_date.as_deref().unwrap_or("-"),
            )));
        }
    }
    let widget = Paragraph::new(lines)
        .block(Block::default().title("Projects").borders(Borders::ALL))
        .style(Style::default().fg(Color::Green))
        .wrap(Wrap { trim: true });
    frame.render_widget(Clear, overlay_area);
    frame.render_widget(widget, overlay_area);
}

fn render_cycles(frame: &mut Frame, area: Rect, app: &App) {
    let overlay_width = area.width.min(80).max(40);
    let overlay_height = area.height.min(12).max(7);
    let overlay_area = centered_rect(overlay_width, overlay_height, area);
    let mut lines = Vec::new();
    if app.cycles().is_empty() {
        lines.push(Line::from("No cycles available (press y or Esc to close)"));
    } else {
        lines.push(Line::from("Recent cycles (press y or Esc to close):"));
        for cycle in app.cycles() {
            lines.push(Line::from(format!(
                "#{} {} {} → {} [{}]",
                cycle.number,
                cycle
                    .team
                    .as_ref()
                    .map(|t| t.key.clone())
                    .unwrap_or_else(|| "-".into()),
                cycle.starts_at.as_deref().unwrap_or("-"),
                cycle.ends_at.as_deref().unwrap_or("-"),
                cycle.state.as_deref().unwrap_or("-"),
            )));
        }
    }
    let widget = Paragraph::new(lines)
        .block(Block::default().title("Cycles").borders(Borders::ALL))
        .style(Style::default().fg(Color::LightBlue))
        .wrap(Wrap { trim: true });
    frame.render_widget(Clear, overlay_area);
    frame.render_widget(widget, overlay_area);
}
