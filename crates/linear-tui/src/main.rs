use std::io;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use linear_core::auth::{AuthManager, FileCredentialStore, OAuthClient, OAuthConfig};
use linear_core::graphql::{LinearGraphqlClient, TeamSummary, WorkflowStateSummary};
use linear_core::services::issues::{IssueQueryOptions, IssueService};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::{Frame, Terminal};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

const DEFAULT_PROFILE: &str = "default";
const SPINNER_FRAMES: [char; 4] = ['-', '\\', '|', '/'];

fn main() -> Result<()> {
    let runtime = Runtime::new()?;
    runtime.block_on(async_main())
}

async fn async_main() -> Result<()> {
    let session = load_session(DEFAULT_PROFILE).await?;
    let service = IssueService::new(
        LinearGraphqlClient::from_session(&session).context("failed to build GraphQL client")?,
    );

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(&mut stdout, crossterm::terminal::EnterAlternateScreen)?;
    crossterm::execute!(&mut stdout, crossterm::event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(service);
    app.load_issues().await;

    let result = run_app(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::event::DisableMouseCapture,
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|frame| render_app(frame, app))?;

        if event::poll(Duration::from_millis(200))? {
            let evt = event::read()?;
            if app.palette_active {
                if let Event::Key(key) = evt {
                    match key.code {
                        KeyCode::Esc => {
                            app.palette_active = false;
                            app.palette_input.clear();
                            app.set_status("Exited command mode", false);
                        }
                        KeyCode::Enter => {
                            let cmd = app.palette_input.clone();
                            app.palette_active = false;
                            app.palette_input.clear();
                            app.execute_command(cmd).await;
                        }
                        KeyCode::Backspace => {
                            app.palette_input.pop();
                        }
                        KeyCode::Up => {
                            app.recall_palette_history(-1);
                        }
                        KeyCode::Down => {
                            app.recall_palette_history(1);
                        }
                        KeyCode::Char(c) => {
                            app.palette_input.push(c);
                        }
                        _ => {}
                    }
                }
                continue;
            }

            match evt {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('r') => app.load_issues().await,
                    KeyCode::Down | KeyCode::Char('j') => match app.focus {
                        Focus::Issues => app.move_issue_selection(1).await,
                        Focus::Teams => app.move_team_selection(1).await,
                        Focus::States => app.move_state_selection(1).await,
                    },
                    KeyCode::Up | KeyCode::Char('k') => match app.focus {
                        Focus::Issues => app.move_issue_selection(-1).await,
                        Focus::Teams => app.move_team_selection(-1).await,
                        Focus::States => app.move_state_selection(-1).await,
                    },
                    KeyCode::Tab => app.toggle_focus(),
                    KeyCode::Char('t') => app.move_team_selection(1).await,
                    KeyCode::Char('s') => app.move_state_selection(1).await,
                    KeyCode::Char(':') => app.enter_palette(),
                    _ => {}
                },
                _ => {}
            }
        }

        if let Some(handle) = app.pending_detail.as_mut() {
            if handle.is_finished() {
                let handle = app.pending_detail.take().unwrap();
                match handle.await {
                    Ok(Ok(Some(detail))) => {
                        app.set_status(format!("Loaded {}", detail.identifier), false);
                        app.detail = Some(detail);
                    }
                    Ok(Ok(None)) => {
                        app.set_status("Issue detail unavailable", false);
                        app.detail = None;
                    }
                    Ok(Err(err)) => {
                        app.set_status(format!("Error loading detail: {err}"), false);
                        app.detail = None;
                    }
                    Err(err) => {
                        app.set_status(format!("Task error loading detail: {err}"), false);
                        app.detail = None;
                    }
                }
            }
        }

        if app.status_spinner {
            if app.pending_detail.is_some() {
                app.status_tick();
            } else {
                app.status_spinner = false;
            }
        }
    }
    Ok(())
}

struct App {
    service: IssueService,
    issues: Vec<linear_core::graphql::IssueSummary>,
    detail: Option<linear_core::graphql::IssueDetail>,
    status_base: String,
    status_spinner: bool,
    spinner_index: usize,
    focus: Focus,
    selected: usize,
    pending_detail: Option<JoinHandle<Result<Option<linear_core::graphql::IssueDetail>>>>,
    teams: Vec<TeamSummary>,
    team_index: Option<usize>,
    states: Vec<WorkflowStateSummary>,
    state_index: Option<usize>,
    states_team_id: Option<String>,
    palette_active: bool,
    palette_input: String,
    palette_history: Vec<String>,
    palette_history_index: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Teams,
    States,
    Issues,
}

impl App {
    fn new(service: IssueService) -> Self {
        Self {
            service,
            issues: Vec::new(),
            detail: None,
            status_base: "Press 'r' to refresh, arrows to navigate, 'q' to quit".into(),
            status_spinner: false,
            spinner_index: 0,
            focus: Focus::Issues,
            selected: 0,
            pending_detail: None,
            teams: Vec::new(),
            team_index: None,
            states: Vec::new(),
            state_index: None,
            states_team_id: None,
            palette_active: false,
            palette_input: String::new(),
            palette_history: Vec::new(),
            palette_history_index: None,
        }
    }

    fn set_status(&mut self, message: impl Into<String>, spinner: bool) {
        self.status_base = message.into();
        self.status_spinner = spinner;
        if spinner {
            self.spinner_index = 0;
        }
    }

    fn set_spinner_status(&mut self, message: impl Into<String>) {
        self.set_status(message, true);
    }

    fn status_text(&self) -> String {
        if self.status_spinner {
            let frame = SPINNER_FRAMES[self.spinner_index % SPINNER_FRAMES.len()];
            format!("{} {}", self.status_base, frame)
        } else {
            self.status_base.clone()
        }
    }

    fn status_tick(&mut self) {
        if self.status_spinner {
            self.spinner_index = (self.spinner_index + 1) % SPINNER_FRAMES.len();
        }
    }

    async fn load_issues(&mut self) {
        self.abort_pending();
        self.ensure_teams().await;
        self.ensure_states().await;
        match fetch_issue_summaries(
            &self.service,
            self.current_team_id(),
            self.current_state_id(),
        )
        .await
        {
            Ok((issues, detail)) => {
                self.issues = issues;
                self.detail = detail;
                self.selected = 0;
                if self.detail.is_none() && !self.issues.is_empty() {
                    let first_key = self.issues[0].identifier.clone();
                    self.queue_detail_fetch(first_key);
                    self.set_spinner_status(format!(
                        "Loading first issue... (team: {}, state: {})",
                        self.current_team_label(),
                        self.current_state_label()
                    ));
                } else {
                    self.set_status(
                        format!(
                            "Loaded {} issues (team: {}, state: {})",
                            self.issues.len(),
                            self.current_team_label(),
                            self.current_state_label()
                        ),
                        false,
                    );
                }
            }
            Err(err) => {
                self.issues.clear();
                self.detail = None;
                self.selected = 0;
                self.set_status(format!("Error: {err}"), false);
            }
        }
    }

    async fn move_issue_selection(&mut self, delta: isize) {
        if self.issues.is_empty() {
            return;
        }
        let len = self.issues.len();
        let new_index = (self.selected as isize + delta).clamp(0, (len - 1) as isize) as usize;
        if new_index != self.selected {
            self.selected = new_index;
            if let Some(issue) = self.issues.get(self.selected) {
                let key = issue.identifier.clone();
                self.detail = None;
                self.abort_pending();
                self.set_spinner_status(format!("Loading {}...", key));
                self.queue_detail_fetch(key);
            }
        }
    }

    fn queue_detail_fetch(&mut self, key: String) {
        let service = self.service.clone();
        self.pending_detail = Some(tokio::spawn(fetch_issue_detail(service, key)));
    }

    fn abort_pending(&mut self) {
        if let Some(handle) = self.pending_detail.take() {
            handle.abort();
        }
    }

    async fn ensure_teams(&mut self) {
        if self.teams.is_empty() {
            match self.service.teams().await {
                Ok(teams) => self.teams = teams,
                Err(err) => {
                    self.set_status(format!("Failed to load teams: {err}"), false);
                }
            }
        }
    }

    async fn move_team_selection(&mut self, delta: isize) {
        self.ensure_teams().await;
        if self.teams.is_empty() {
            return;
        }
        let total = self.teams.len() as isize + 1; // include "All"
        let current = self.team_index.map(|idx| idx as isize + 1).unwrap_or(0);
        let next = (current + delta).clamp(0, total - 1);
        self.team_index = if next == 0 {
            None
        } else {
            Some((next - 1) as usize)
        };
        self.states.clear();
        self.state_index = None;
        self.states_team_id = None;
        let team_label = self.current_team_label();
        self.set_status(format!("Switched to team: {}", team_label), false);
        self.load_issues().await;
    }

    fn current_team_id(&self) -> Option<String> {
        self.team_index
            .and_then(|idx| self.teams.get(idx))
            .map(|team| team.id.clone())
    }

    fn current_team_label(&self) -> String {
        self.team_index
            .and_then(|idx| self.teams.get(idx))
            .map(|team| team.key.clone())
            .unwrap_or_else(|| "All".into())
    }

    fn current_state_label(&self) -> String {
        self.state_index
            .and_then(|idx| self.states.get(idx))
            .map(|state| state.name.clone())
            .unwrap_or_else(|| "All".into())
    }

    async fn ensure_states(&mut self) {
        if let Some(team_id) = self.current_team_id() {
            if self.states_team_id.as_deref() != Some(&team_id) {
                match self.service.workflow_states(&team_id).await {
                    Ok(states) => {
                        self.states = states;
                        self.states_team_id = Some(team_id);
                        self.state_index = None;
                    }
                    Err(err) => {
                        self.set_status(format!("Failed to load states: {err}"), false);
                    }
                }
            }
        } else {
            self.states.clear();
            self.states_team_id = None;
            self.state_index = None;
        }
        if let Some(idx) = self.state_index {
            if idx >= self.states.len() {
                self.state_index = None;
            }
        }
    }

    async fn move_state_selection(&mut self, delta: isize) {
        self.ensure_states().await;
        if self.states.is_empty() {
            return;
        }
        let total = self.states.len() as isize + 1; // include "All"
        let current = self.state_index.map(|idx| idx as isize + 1).unwrap_or(0);
        let next = (current + delta).clamp(0, total - 1);
        self.state_index = if next == 0 {
            None
        } else {
            Some((next - 1) as usize)
        };
        let state_label = self
            .state_index
            .and_then(|idx| self.states.get(idx))
            .map(|state| state.name.clone())
            .unwrap_or_else(|| "All".into());
        self.set_status(format!("State filter: {}", state_label), false);
        self.load_issues().await;
    }

    fn current_state_id(&self) -> Option<String> {
        self.state_index
            .and_then(|idx| self.states.get(idx))
            .map(|state| state.id.clone())
    }

    fn palette_suggestions(&self) -> Vec<Line<'static>> {
        let input = self.palette_input.trim().to_ascii_lowercase();
        if let Some(rest) = input.strip_prefix("team ") {
            let key = rest.trim();
            let mut lines = Vec::new();
            for team in self
                .teams
                .iter()
                .filter(|team| team.key.to_ascii_lowercase().starts_with(key))
                .take(3)
            {
                lines.push(Line::from(format!("team {}", team.key)));
            }
            if lines.is_empty() {
                lines.push(Line::from("team <key>"));
            }
            lines
        } else if let Some(rest) = input.strip_prefix("state ") {
            let name = rest.trim();
            if self.states.is_empty() {
                vec![Line::from("state <name> (load a team first)")]
            } else {
                let mut lines = Vec::new();
                for state in self
                    .states
                    .iter()
                    .filter(|state| state.name.to_ascii_lowercase().starts_with(name))
                    .take(3)
                {
                    lines.push(Line::from(format!("state {}", state.name)));
                }
                if lines.is_empty() {
                    lines.push(Line::from("state <name>"));
                }
                lines
            }
        } else {
            vec![
                Line::from("team <key>"),
                Line::from("state <name>"),
                Line::from("clear"),
                Line::from("reload"),
            ]
        }
    }

    fn recall_palette_history(&mut self, delta: isize) {
        if self.palette_history.is_empty() {
            return;
        }
        let len = self.palette_history.len() as isize;
        let current = self
            .palette_history_index
            .map(|idx| idx as isize)
            .unwrap_or(len);
        let next = (current + delta).clamp(0, len);
        if next == len {
            self.palette_history_index = None;
            self.palette_input.clear();
        } else {
            self.palette_history_index = Some(next as usize);
            self.palette_input = self.palette_history[next as usize].clone();
        }
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Issues => Focus::Teams,
            Focus::Teams => Focus::States,
            Focus::States => Focus::Issues,
        };
        let message = match self.focus {
            Focus::Issues => "Focus: issues",
            Focus::Teams => "Focus: teams",
            Focus::States => "Focus: states",
        };
        self.set_status(message, false);
    }

    fn enter_palette(&mut self) {
        self.palette_active = true;
        self.palette_input.clear();
        self.palette_history_index = None;
        self.set_status("Command mode (: to exit, ↑/↓ history)", false);
    }

    async fn execute_command(&mut self, command: String) {
        let cmd = command.trim();
        self.palette_history_index = None;
        if !cmd.is_empty() {
            if self
                .palette_history
                .last()
                .map(|last| last != cmd)
                .unwrap_or(true)
            {
                self.palette_history.push(cmd.to_string());
            }
        }
        if cmd.starts_with("team ") {
            let team_key = cmd.trim_start_matches("team ").trim();
            self.ensure_teams().await;
            self.team_index = self
                .teams
                .iter()
                .position(|team| team.key.eq_ignore_ascii_case(team_key));
            if self.team_index.is_none() {
                self.set_status(format!("Team '{}' not found", team_key), false);
            } else {
                self.states.clear();
                self.state_index = None;
                self.states_team_id = None;
                self.set_status(format!("Command: team {}", team_key), false);
                self.load_issues().await;
            }
        } else if cmd.starts_with("state ") {
            let state_name = cmd.trim_start_matches("state ").trim();
            self.ensure_states().await;
            if self.states.is_empty() {
                self.set_status("Load a team with workflow states first", false);
            } else {
                self.state_index = self
                    .states
                    .iter()
                    .position(|state| state.name.eq_ignore_ascii_case(state_name));
                if self.state_index.is_none() {
                    self.set_status(format!("State '{}' not found", state_name), false);
                } else {
                    self.set_status(format!("Command: state {}", state_name), false);
                    self.load_issues().await;
                }
            }
        } else if cmd == "clear" {
            self.team_index = None;
            self.state_index = None;
            self.states_team_id = None;
            self.states.clear();
            self.set_status("Cleared filters", false);
            self.load_issues().await;
        } else if cmd == "reload" {
            self.teams.clear();
            self.team_index = None;
            self.states.clear();
            self.state_index = None;
            self.states_team_id = None;
            self.set_status("Reloading metadata", true);
            self.load_issues().await;
        } else if !cmd.is_empty() {
            self.set_status(format!("Unknown command: {}", cmd), false);
        } else {
            self.set_status("Command mode exited", false);
        }
    }
}

async fn fetch_issue_summaries(
    service: &IssueService,
    team_id: Option<String>,
    state_id: Option<String>,
) -> Result<(
    Vec<linear_core::graphql::IssueSummary>,
    Option<linear_core::graphql::IssueDetail>,
)> {
    let issues = service
        .list(IssueQueryOptions {
            limit: 20,
            team_id,
            state_id,
            ..Default::default()
        })
        .await
        .context("failed to fetch issues")?;

    let detail = if let Some(first) = issues.get(0) {
        fetch_issue_detail(service.clone(), first.identifier.clone()).await?
    } else {
        None
    };

    Ok((issues, detail))
}

async fn fetch_issue_detail(
    service: IssueService,
    key: String,
) -> Result<Option<linear_core::graphql::IssueDetail>> {
    Ok(service.get_by_key(&key).await.ok())
}

async fn load_session(profile: &str) -> Result<linear_core::auth::AuthSession> {
    let store = FileCredentialStore::with_default_locator()
        .context("unable to initialise credential store")?;
    let oauth_config = build_oauth_config()?;
    let oauth = OAuthClient::new(oauth_config).context("failed to build OAuth client")?;
    let manager = AuthManager::new(store, oauth, profile);
    manager.ensure_fresh_session().await?.ok_or_else(|| {
        anyhow!(
            "no credentials stored for profile '{}'; run `linear auth login`",
            profile
        )
    })
}

fn build_oauth_config() -> Result<OAuthConfig> {
    let client_id = std::env::var("LINEAR_CLIENT_ID")
        .context("LINEAR_CLIENT_ID environment variable is required for the TUI")?;
    let redirect = std::env::var("LINEAR_REDIRECT_URI")
        .context("LINEAR_REDIRECT_URI environment variable is required for the TUI")?;
    let redirect_uri = redirect.parse()?;
    let mut config = OAuthConfig::new(client_id, redirect_uri);
    if let Ok(secret) = std::env::var("LINEAR_CLIENT_SECRET") {
        if !secret.is_empty() {
            config = config.with_secret(secret);
        }
    }
    Ok(config)
}

fn render_app(frame: &mut Frame, app: &App) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(28), Constraint::Min(1)])
        .split(frame.size());

    render_team_panel(frame, layout[0], app);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage(60),
                Constraint::Percentage(40),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(layout[1]);

    let items: Vec<ListItem> = if app.issues.is_empty() {
        vec![ListItem::new("No issues loaded")]
    } else {
        app.issues
            .iter()
            .map(|issue| {
                let line = format!("{}  {}", issue.identifier, issue.title);
                ListItem::new(Line::from(line))
            })
            .collect()
    };

    let mut list_state = ratatui::widgets::ListState::default();
    if !app.issues.is_empty() {
        list_state.select(Some(app.selected));
    }

    let issue_highlight = if matches!(app.focus, Focus::Issues) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let list = List::new(items)
        .block(Block::default().title("Issues").borders(Borders::ALL))
        .highlight_style(issue_highlight);
    frame.render_stateful_widget(list, right_chunks[0], &mut list_state);

    let detail_block = Block::default().title("Details").borders(Borders::ALL);
    let detail_text = if let Some(issue) = &app.detail {
        format!(
            "{}

State: {}
Priority: {}
Updated: {}",
            issue.description.as_deref().unwrap_or("(no description)"),
            issue.state.as_ref().map(|s| s.name.as_str()).unwrap_or("-"),
            issue
                .priority
                .map(|p| p.to_string())
                .unwrap_or_else(|| "-".into()),
            issue.updated_at.to_rfc3339()
        )
    } else {
        "Select an issue to view details".into()
    };
    let detail = Paragraph::new(detail_text).block(detail_block);
    frame.render_widget(detail, right_chunks[1]);

    let status = Paragraph::new(app.status_text()).style(Style::default().fg(Color::Cyan));
    frame.render_widget(status, right_chunks[2]);

    let help_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
        .split(right_chunks[3]);

    let help = Paragraph::new(
        "Commands: r=refresh  tab=focus  j/k=move  t=team  s=state  :=command  q=quit",
    )
    .style(Style::default());
    frame.render_widget(help, help_chunks[0]);

    if app.palette_active {
        let prompt = Paragraph::new(format!(":{}", app.palette_input))
            .style(Style::default().fg(Color::Yellow));
        let suggestions_lines = app.palette_suggestions();
        let history_lines: Vec<Line> = app
            .palette_history
            .iter()
            .rev()
            .take(3)
            .map(|entry| Line::from(entry.as_str()))
            .collect();

        frame.render_widget(prompt, help_chunks[1]);

        let mut overlay_y = help_chunks[1].y;

        if !history_lines.is_empty() {
            let history_height = history_lines.len() as u16;
            let history_area = ratatui::layout::Rect {
                x: help_chunks[1].x,
                y: overlay_y.saturating_sub(history_height),
                width: help_chunks[1].width,
                height: history_height,
            };
            let history_widget = Paragraph::new(history_lines)
                .block(Block::default().title("History").borders(Borders::NONE))
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(history_widget, history_area);
            overlay_y = history_area.y;
        }

        if !suggestions_lines.is_empty() {
            let suggestions_height = suggestions_lines.len() as u16;
            let suggestions_area = ratatui::layout::Rect {
                x: help_chunks[1].x,
                y: overlay_y.saturating_sub(suggestions_height),
                width: help_chunks[1].width,
                height: suggestions_height,
            };
            let suggestions_widget = Paragraph::new(suggestions_lines)
                .block(Block::default().title("Suggestions").borders(Borders::NONE))
                .style(Style::default().fg(Color::Gray));
            frame.render_widget(suggestions_widget, suggestions_area);
        }
    }
}

fn render_team_panel(frame: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(area);

    // Teams panel
    let mut team_items = Vec::new();
    team_items.push(ListItem::new("All teams"));
    for team in &app.teams {
        team_items.push(ListItem::new(format!("{}  {}", team.key, team.name)));
    }
    let mut team_state = ratatui::widgets::ListState::default();
    let selected_team = app.team_index.map(|idx| idx + 1).unwrap_or(0);
    team_state.select(Some(selected_team));
    let team_highlight = if matches!(app.focus, Focus::Teams) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let team_list = List::new(team_items)
        .block(Block::default().title("Teams").borders(Borders::ALL))
        .highlight_style(team_highlight);
    frame.render_stateful_widget(team_list, panels[0], &mut team_state);

    // States panel
    let mut state_items = Vec::new();
    state_items.push(ListItem::new("All states"));
    for workflow in &app.states {
        state_items.push(ListItem::new(format!("{}", workflow.name)));
    }
    let mut state_state = ratatui::widgets::ListState::default();
    let selected_state = app.state_index.map(|idx| idx + 1).unwrap_or(0);
    state_state.select(Some(selected_state));
    let state_highlight = if matches!(app.focus, Focus::States) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let state_list = List::new(state_items)
        .block(Block::default().title("States").borders(Borders::ALL))
        .highlight_style(state_highlight);
    frame.render_stateful_widget(state_list, panels[1], &mut state_state);
}
