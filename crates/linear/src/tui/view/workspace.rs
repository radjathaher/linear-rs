use chrono::{DateTime, Local, NaiveDate, Utc};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use textwrap::wrap;

use crate::tui::app::{App, DetailTab, Focus};
use crate::tui::view::util::issue_list_line;
use linear_core::graphql::{IssueAssignee, IssueDetail, IssueHistory, IssueSubIssue, UserSummary};

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
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 4 || inner.height == 0 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    render_detail_tabs(frame, chunks[0], app);
    render_detail_content(frame, chunks[1], app);
}

fn render_detail_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans = Vec::new();
    for tab in DetailTab::all() {
        let label = format!(" {} ", tab.label());
        let style = if tab == app.detail_tab() {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::Gray)
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
    }
    let tabs_line = Line::from(spans);
    let widget = Paragraph::new(tabs_line);
    frame.render_widget(widget, area);
}

fn render_detail_content(frame: &mut Frame, area: Rect, app: &App) {
    let Some(issue) = app.detail() else {
        frame.render_widget(Paragraph::new("Select an issue to view details"), area);
        return;
    };

    match app.detail_tab() {
        DetailTab::Summary => render_summary(frame, area, issue),
        DetailTab::Description => render_description(frame, area, issue),
        DetailTab::Activity => render_activity(frame, area, issue),
        DetailTab::SubIssues => render_sub_issues(frame, area, issue),
    }
}

fn render_summary(frame: &mut Frame, area: Rect, issue: &IssueDetail) {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            issue.identifier.to_uppercase(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::raw(issue.title.clone()),
    ]));

    lines.push(Line::from(format!(
        "State: {}",
        issue.state.as_ref().map(|s| s.name.as_str()).unwrap_or("-")
    )));

    if let Some(assignee) = issue.assignee.as_ref() {
        let name = assignee
            .display_name
            .as_ref()
            .or(assignee.name.as_ref())
            .map(|s| s.as_str())
            .unwrap_or("-");
        lines.push(Line::from(format!("Assignee: {}", name)));
    } else {
        lines.push(Line::from("Assignee: -"));
    }

    let priority = issue
        .priority
        .map(|p| p.to_string())
        .unwrap_or_else(|| "-".into());
    lines.push(Line::from(format!("Priority: {}", priority)));

    if let Some(labels) = issue.labels.as_ref() {
        if labels.nodes.is_empty() {
            lines.push(Line::from("Labels: -"));
        } else {
            let list = labels
                .nodes
                .iter()
                .map(|l| l.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(Line::from(format!("Labels: {}", list)));
        }
    } else {
        lines.push(Line::from("Labels: -"));
    }

    if let Some(team) = issue.team.as_ref() {
        lines.push(Line::from(format!("Team: {} ({})", team.name, team.key)));
    }

    lines.push(Line::from(format!(
        "Created: {}",
        issue.created_at.to_rfc3339()
    )));
    lines.push(Line::from(format!(
        "Updated: {}",
        issue.updated_at.to_rfc3339()
    )));

    if let Some(url) = issue.url.as_deref() {
        lines.push(Line::from(format!("URL: {}", url)));
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_description(frame: &mut Frame, area: Rect, issue: &IssueDetail) {
    let width = area.width.saturating_sub(1).max(10) as usize;
    let description = issue.description.as_deref().unwrap_or("(no description)");
    let wrapped = wrap(description, width)
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    frame.render_widget(Paragraph::new(wrapped).wrap(Wrap { trim: false }), area);
}

fn render_activity(frame: &mut Frame, area: Rect, issue: &IssueDetail) {
    let available_width = area.width.saturating_sub(2) as usize;
    if available_width < 6 {
        render_placeholder(frame, area, "Area too small for activity");
        return;
    }

    let mut entries = collect_activity_entries(issue, available_width);
    if entries.is_empty() {
        render_placeholder(frame, area, "No recent activity");
        return;
    }

    entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    let mut grouped: Vec<(NaiveDate, Vec<ActivityEntry>)> = Vec::new();
    for entry in entries {
        if let Some((date, list)) = grouped.last_mut() {
            if *date == entry.date {
                list.push(entry);
                continue;
            }
        }
        grouped.push((entry.date, vec![entry]));
    }

    let mut lines: Vec<Line> = Vec::new();
    let header_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let actor_style = Style::default().fg(Color::Cyan);
    for (date, events) in grouped {
        lines.push(Line::styled(
            date.format("%Y-%m-%d").to_string(),
            header_style,
        ));
        for (idx, event) in events.iter().enumerate() {
            let branch = if idx + 1 == events.len() {
                "└─"
            } else {
                "├─"
            };
            let spacer = if idx + 1 == events.len() {
                "   "
            } else {
                "│  "
            };
            let mut header_text = format!("{branch} {}  {}", event.time_label, event.actor);
            if let Some(first) = event.summary.first() {
                header_text.push_str(" — ");
                header_text.push_str(first);
            }
            lines.push(Line::styled(header_text, actor_style));
            for summary in event.summary.iter().skip(1) {
                lines.push(Line::from(format!("{spacer}• {summary}")));
            }
            for body_line in &event.body {
                lines.push(Line::from(format!("{spacer}{body_line}")));
            }
        }
        lines.push(Line::default());
    }
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_sub_issues(frame: &mut Frame, area: Rect, issue: &IssueDetail) {
    let Some(connection) = issue.sub_issues.as_ref() else {
        render_placeholder(frame, area, "No sub-issues found");
        return;
    };
    if connection.nodes.is_empty() {
        render_placeholder(frame, area, "No sub-issues found");
        return;
    }

    let width = area.width.saturating_sub(2) as usize;
    if width < 6 {
        render_placeholder(frame, area, "Area too small for sub-issues");
        return;
    }

    let mut lines = Vec::new();
    for (idx, node) in connection.nodes.iter().enumerate() {
        append_sub_issue_lines(
            &mut lines,
            node,
            &[],
            idx + 1 == connection.nodes.len(),
            width,
        );
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_placeholder(frame: &mut Frame, area: Rect, message: &str) {
    frame.render_widget(Paragraph::new(message), area);
}

#[derive(Debug)]
struct ActivityEntry {
    timestamp: DateTime<Utc>,
    date: NaiveDate,
    time_label: String,
    actor: String,
    summary: Vec<String>,
    body: Vec<String>,
}

fn collect_activity_entries(issue: &IssueDetail, width: usize) -> Vec<ActivityEntry> {
    let mut events = Vec::new();
    let wrap_width = width.saturating_sub(6).max(10);

    if let Some(comments) = issue.comments.as_ref() {
        for comment in &comments.nodes {
            let (date, time_label) = split_datetime(comment.created_at);
            let actor = comment
                .user
                .as_ref()
                .map(display_user_summary)
                .unwrap_or_else(|| "Unknown".into());
            let body = wrap_lines(comment.body.trim(), wrap_width);
            let summary_snippet = truncate_snippet(comment.body.trim(), 64);
            let summary = if summary_snippet.is_empty() {
                vec!["Comment".into()]
            } else {
                vec![format!("Comment: {summary_snippet}")]
            };
            events.push(ActivityEntry {
                timestamp: comment.created_at,
                date,
                time_label,
                actor,
                summary,
                body,
            });
        }
    }

    if let Some(history) = issue.history.as_ref() {
        for entry in &history.nodes {
            let summary = summarize_history(entry);
            let mut body = Vec::new();
            if let Some(desc) = entry.updated_description.as_ref() {
                if !desc.trim().is_empty() {
                    body = wrap_lines(desc.trim(), wrap_width);
                }
            }
            if summary.is_empty() && body.is_empty() {
                continue;
            }
            let (date, time_label) = split_datetime(entry.created_at);
            let actor = if entry.actors.is_empty() {
                "System".into()
            } else {
                entry
                    .actors
                    .iter()
                    .map(display_user_summary)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            events.push(ActivityEntry {
                timestamp: entry.created_at,
                date,
                time_label,
                actor,
                summary,
                body,
            });
        }
    }

    events
}

fn summarize_history(entry: &IssueHistory) -> Vec<String> {
    let mut lines = Vec::new();

    push_change(
        &mut lines,
        "State",
        entry.from_state.as_ref().map(|s| s.name.clone()),
        entry.to_state.as_ref().map(|s| s.name.clone()),
    );
    push_change(
        &mut lines,
        "Assignee",
        entry
            .from_assignee
            .as_ref()
            .map(|a| display_assignee_short(a.clone())),
        entry
            .to_assignee
            .as_ref()
            .map(|a| display_assignee_short(a.clone())),
    );
    push_change(
        &mut lines,
        "Priority",
        entry.from_priority.map(|p| p.to_string()),
        entry.to_priority.map(|p| p.to_string()),
    );
    push_change(
        &mut lines,
        "Due",
        entry.from_due_date.clone(),
        entry.to_due_date.clone(),
    );
    push_change(
        &mut lines,
        "Title",
        entry.from_title.clone(),
        entry.to_title.clone(),
    );

    if entry
        .updated_description
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        lines.push("Description updated".into());
    }

    lines
}

fn push_change(lines: &mut Vec<String>, label: &str, from: Option<String>, to: Option<String>) {
    if normalize_opt(&from) == normalize_opt(&to) {
        return;
    }
    let from_display = from.filter(|s| !s.is_empty()).unwrap_or_else(|| "-".into());
    let to_display = to.filter(|s| !s.is_empty()).unwrap_or_else(|| "-".into());
    lines.push(format!("{label}: {from_display} → {to_display}"));
}

fn normalize_opt(value: &Option<String>) -> Option<String> {
    value.as_ref().map(|s| s.trim().to_ascii_lowercase())
}

fn wrap_lines(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() || width == 0 {
        return Vec::new();
    }
    wrap(text, width)
        .into_iter()
        .map(|line| line.to_string())
        .collect()
}

fn truncate_snippet(text: &str, max: usize) -> String {
    let snippet = text.lines().next().unwrap_or("").trim();
    if snippet.len() <= max {
        snippet.to_string()
    } else if max > 3 {
        format!("{}...", &snippet[..max - 3])
    } else {
        snippet[..max.min(snippet.len())].to_string()
    }
}

fn split_datetime(ts: DateTime<Utc>) -> (NaiveDate, String) {
    let local = ts.with_timezone(&Local);
    (local.date_naive(), local.format("%H:%M").to_string())
}

fn display_user_summary(user: &UserSummary) -> String {
    user.display_name
        .as_ref()
        .or(user.name.as_ref())
        .cloned()
        .unwrap_or_else(|| "Unknown".into())
}

fn display_assignee_short(assignee: IssueAssignee) -> String {
    assignee
        .display_name
        .or(assignee.name)
        .unwrap_or_else(|| "-".into())
}

fn append_sub_issue_lines(
    lines: &mut Vec<Line>,
    node: &IssueSubIssue,
    ancestors_last: &[bool],
    is_last: bool,
    width: usize,
) {
    let mut prefix = String::new();
    for &last in ancestors_last {
        prefix.push_str(if last { "   " } else { "│  " });
    }
    let branch = if is_last { "└─" } else { "├─" };
    let continuation = if is_last { "   " } else { "│  " };

    let title = format!("{} {}", node.identifier.to_uppercase(), node.title.trim());
    let wrap_width = width
        .saturating_sub(prefix.len() + continuation.len() + 1)
        .max(10);
    let wrapped_title = wrap(title.as_str(), wrap_width);

    for (idx, part) in wrapped_title.into_iter().map(|c| c.to_string()).enumerate() {
        if idx == 0 {
            lines.push(Line::from(format!("{prefix}{branch} {part}")));
        } else {
            lines.push(Line::from(format!("{prefix}{continuation}{part}")));
        }
    }

    let state = node.state.as_ref().map(|s| s.name.as_str()).unwrap_or("-");
    let assignee = node
        .assignee
        .as_ref()
        .map(|a| display_assignee_short(a.clone()))
        .unwrap_or_else(|| "-".into());
    let priority = node
        .priority
        .map(|p| p.to_string())
        .unwrap_or_else(|| "-".into());
    let mut meta = format!("state: {state}, assignee: {assignee}, priority: {priority}");
    if let Some(team) = node.team.as_ref() {
        meta.push_str(&format!(", team: {}", team.key));
    }
    lines.push(Line::from(format!("{prefix}{continuation}{meta}")));

    if let Some(children) = node.children.as_ref() {
        let mut next_prefix = ancestors_last.to_vec();
        next_prefix.push(is_last);
        for (idx, child) in children.nodes.iter().enumerate() {
            append_sub_issue_lines(
                lines,
                child,
                &next_prefix,
                idx + 1 == children.nodes.len(),
                width,
            );
        }
    }
}
