use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use linear_core::graphql::IssueSummary;

pub fn issue_list_line(issue: &IssueSummary, filter: Option<&str>) -> Line<'static> {
    let mut spans = Vec::new();
    spans.push(Span::raw(format!("{}  ", issue.identifier)));
    if let Some(query) = filter.filter(|q| !q.is_empty()) {
        spans.extend(highlight_spans(&issue.title, query));
    } else {
        spans.push(Span::raw(issue.title.clone()));
    }
    Line::from(spans)
}

fn highlight_spans(text: &str, query: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let needle = query.to_lowercase();
    if needle.is_empty() {
        spans.push(Span::raw(text.to_string()));
        return spans;
    }
    let haystack = text.to_lowercase();
    let mut start = 0;
    let mut offset = 0;
    while let Some(pos) = haystack[offset..].find(&needle) {
        let match_start = offset + pos;
        if match_start > start {
            spans.push(Span::raw(text[start..match_start].to_string()));
        }
        let match_end = match_start + needle.len();
        spans.push(Span::styled(
            text[match_start..match_end].to_string(),
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        ));
        start = match_end;
        offset = match_end;
    }
    if start < text.len() {
        spans.push(Span::raw(text[start..].to_string()));
    }
    spans
}

pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width).max(1);
    let height = height.min(area.height).max(1);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}
