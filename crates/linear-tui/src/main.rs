use std::io;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use linear_core::auth::{AuthManager, FileCredentialStore, OAuthClient, OAuthConfig};
use linear_core::graphql::{LinearGraphqlClient, TeamSummary, WorkflowStateSummary};
use linear_core::services::issues::{IssueQueryOptions, IssueService};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::{Frame, Terminal};
use textwrap::wrap;
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
            if app.show_help_overlay {
                if let Event::Key(key) = evt {
                    match key.code {
                        KeyCode::Char('?') | KeyCode::Esc => {
                            app.toggle_help_overlay();
                        }
                        _ => {}
                    }
                }
                continue;
            }

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
                    KeyCode::Char('/') => app.enter_contains_palette(),
                    KeyCode::Char('c') => app.clear_all_filters().await,
                    KeyCode::Char('?') => app.toggle_help_overlay(),
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
    title_contains: Option<String>,
    show_help_overlay: bool,
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
            title_contains: None,
            show_help_overlay: false,
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

    fn filters_text(&self) -> String {
        let team = self.current_team_label();
        let state = self.current_state_label();
        let contains = self
            .title_contains
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| format!("'{}'", s))
            .unwrap_or_else(|| "-".into());
        let mut parts = Vec::new();
        parts.push(format!("team={}", team));
        parts.push(format!("state={}", state));
        parts.push(format!("title~{}", contains));
        if let Some(issue) = self.issues.get(self.selected) {
            parts.push(format!("selected={}", issue.identifier));
        }
        format!("Filters: {}", parts.join("  "))
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
        self.load_issues_with_filters().await;
    }

    fn current_contains(&self) -> Option<String> {
        self.title_contains.clone()
    }

    async fn load_issues_with_filters(&mut self) {
        self.abort_pending();
        let contains = self.current_contains();
        match fetch_issue_summaries(
            &self.service,
            self.current_team_id(),
            self.current_state_id(),
            contains,
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

    async fn load_issues_with_contains(&mut self, contains: Option<String>) {
        self.title_contains = contains;
        self.load_issues_with_filters().await;
    }

    fn select_issue(&mut self, index: usize) {
        if self.issues.is_empty() || index >= self.issues.len() {
            return;
        }
        if self.selected == index && self.detail.is_some() {
            return;
        }
        self.selected = index;
        if let Some(issue) = self.issues.get(self.selected) {
            let key = issue.identifier.clone();
            self.detail = None;
            self.abort_pending();
            self.set_spinner_status(format!("Loading {}...", key));
            self.queue_detail_fetch(key);
        }
    }

    fn jump_to_issue(&mut self, key: &str) -> bool {
        if self.issues.is_empty() {
            return false;
        }
        if let Some(idx) = self
            .issues
            .iter()
            .position(|issue| issue.identifier.eq_ignore_ascii_case(key))
        {
            if idx == self.selected {
                self.set_status(format!("Already focused on {}", key.to_uppercase()), false);
            } else {
                self.select_issue(idx);
            }
            true
        } else {
            false
        }
    }

    fn jump_relative(&mut self, delta: isize) -> bool {
        if self.issues.is_empty() {
            return false;
        }
        let len = self.issues.len() as isize;
        let mut index = self.selected as isize + delta;
        index = index.clamp(0, len - 1);
        let new_index = index as usize;
        if new_index == self.selected {
            return false;
        }
        self.select_issue(new_index);
        true
    }

    fn jump_first(&mut self) -> bool {
        if self.issues.is_empty() {
            return false;
        }
        if self.selected == 0 {
            return false;
        }
        self.select_issue(0);
        true
    }

    fn jump_last(&mut self) -> bool {
        if self.issues.is_empty() {
            return false;
        }
        let last = self.issues.len() - 1;
        if self.selected == last {
            return false;
        }
        self.select_issue(last);
        true
    }
    async fn clear_all_filters(&mut self) {
        self.team_index = None;
        self.state_index = None;
        self.states_team_id = None;
        self.states.clear();
        self.title_contains = None;
        self.set_status("Cleared filters", false);
        self.load_issues().await;
    }
    async fn move_issue_selection(&mut self, delta: isize) {
        if self.issues.is_empty() {
            return;
        }
        let len = self.issues.len();
        let new_index = (self.selected as isize + delta).clamp(0, (len - 1) as isize) as usize;
        if new_index != self.selected {
            self.select_issue(new_index);
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
        self.load_issues_with_filters().await;
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
        self.load_issues_with_filters().await;
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
        } else if let Some(rest) = input.strip_prefix("contains ") {
            let term = rest.trim();
            if term.is_empty() {
                vec![Line::from("contains <text>"), Line::from("contains clear")]
            } else {
                vec![
                    Line::from(format!("contains {}", term)),
                    Line::from("contains clear"),
                ]
            }
        } else if let Some(rest) = input.strip_prefix("view ") {
            let term = rest.trim();
            if term.is_empty() {
                vec![
                    Line::from("view <issue-key>"),
                    Line::from("view next"),
                    Line::from("view prev"),
                    Line::from("view first"),
                    Line::from("view last"),
                ]
            } else {
                vec![Line::from(format!("view {}", term))]
            }
        } else {
            vec![
                Line::from("team <key>"),
                Line::from("state <name>"),
                Line::from("contains <text>"),
                Line::from("contains clear"),
                Line::from("view <issue-key>"),
                Line::from("view next"),
                Line::from("view prev"),
                Line::from("view first"),
                Line::from("view last"),
                Line::from("clear"),
                Line::from("reload"),
                Line::from("help"),
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
        self.show_help_overlay = false;
        self.palette_input.clear();
        self.palette_history_index = None;
        self.set_status("Command mode (: to exit, ↑/↓ history)", false);
    }

    fn enter_contains_palette(&mut self) {
        self.palette_active = true;
        self.show_help_overlay = false;
        self.palette_history_index = None;
        self.palette_input = match self.current_contains() {
            Some(term) => format!("contains {}", term),
            None => "contains ".into(),
        };
        self.set_status("Contains filter (Esc to cancel, Enter to apply)", false);
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
        if let Some(team_key) = cmd.strip_prefix("team ") {
            let team_key = team_key.trim();
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
                self.load_issues_with_filters().await;
            }
            return;
        }

        if let Some(state_name) = cmd.strip_prefix("state ") {
            let state_name = state_name.trim();
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
                    self.load_issues_with_filters().await;
                }
            }
            return;
        }

        if let Some(term) = cmd.strip_prefix("contains ") {
            let term = term.trim();
            if term.is_empty() {
                self.set_status("Usage: contains <term>", false);
            } else if term.eq_ignore_ascii_case("clear") {
                self.set_status("Cleared title filter", false);
                self.load_issues_with_contains(None).await;
            } else {
                self.set_status(format!("Title contains '{}'", term), false);
                self.load_issues_with_contains(Some(term.to_string())).await;
            }
            return;
        }

        match cmd {
            "view next" => {
                if self.jump_relative(1) {
                    self.set_status("Jumped to next issue", false);
                } else {
                    self.set_status("Already at last issue", false);
                }
                return;
            }
            "view prev" | "view previous" => {
                if self.jump_relative(-1) {
                    self.set_status("Jumped to previous issue", false);
                } else {
                    self.set_status("Already at first issue", false);
                }
                return;
            }
            "view first" => {
                if self.jump_first() {
                    self.set_status("Jumped to first issue", false);
                } else {
                    self.set_status("Already at first issue", false);
                }
                return;
            }
            "view last" => {
                if self.jump_last() {
                    self.set_status("Jumped to last issue", false);
                } else {
                    self.set_status("Already at last issue", false);
                }
                return;
            }
            _ => {}
        }

        if let Some(key) = cmd.strip_prefix("view ") {
            let key = key.trim();
            if key.is_empty() {
                self.set_status("Usage: view <issue-key>", false);
            } else if self.jump_to_issue(key) {
                self.set_status(format!("Jumped to {}", key.to_uppercase()), false);
            } else {
                self.set_status(format!("Issue '{}' not in the current list", key), false);
            }
            return;
        }

        if matches!(cmd, "help" | "?") {
            self.open_help_overlay();
            return;
        }

        match cmd {
            "" => self.set_status("Command mode exited", false),
            "clear" => {
                self.clear_all_filters().await;
            }
            "reload" => {
                self.teams.clear();
                self.team_index = None;
                self.states.clear();
                self.state_index = None;
                self.states_team_id = None;
                self.set_status("Reloading metadata", true);
                self.load_issues().await;
            }
            "contains" => self.set_status("Usage: contains <term>", false),
            _ => self.set_status(format!("Unknown command: {}", cmd), false),
        }
    }

    fn toggle_help_overlay(&mut self) {
        self.show_help_overlay = !self.show_help_overlay;
        if self.show_help_overlay {
            self.palette_active = false;
            self.set_status("Help open (? or Esc to close)", false);
        } else {
            self.set_status("Help closed", false);
        }
    }

    fn open_help_overlay(&mut self) {
        if !self.show_help_overlay {
            self.show_help_overlay = true;
            self.palette_active = false;
            self.set_status("Help open (? or Esc to close)", false);
        } else {
            self.set_status("Help already open (? or Esc to close)", false);
        }
    }
}

async fn fetch_issue_summaries(
    service: &IssueService,
    team_id: Option<String>,
    state_id: Option<String>,
    contains: Option<String>,
) -> Result<(
    Vec<linear_core::graphql::IssueSummary>,
    Option<linear_core::graphql::IssueDetail>,
)> {
    let issues = service
        .list(IssueQueryOptions {
            limit: 20,
            team_id,
            state_id,
            title_contains: contains,
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
                let line = issue_list_line(issue, app.title_contains.as_deref());
                ListItem::new(line)
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
        let width = right_chunks[1].width.saturating_sub(2).max(20) as usize;
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
        lines.join(
            "
",
        )
    } else {
        "Select an issue to view details".into()
    };
    let detail = Paragraph::new(detail_text).block(detail_block);
    frame.render_widget(detail, right_chunks[1]);

    let filters = Paragraph::new(app.filters_text()).style(Style::default().fg(Color::Gray));
    frame.render_widget(filters, right_chunks[2]);

    let status = Paragraph::new(app.status_text()).style(Style::default().fg(Color::Cyan));
    frame.render_widget(status, right_chunks[3]);

    let help_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
        .split(right_chunks[4]);

    let help = Paragraph::new(
        "Commands: r=refresh  c=clear filters  tab=focus  j/k=move  t=team  s=state  /=contains  view next/prev/<key>  reload  :team/:state/:contains  q=quit",
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

    if app.show_help_overlay {
        let overlay_width = layout[1].width.min(80).max(40);
        let overlay_height = layout[1].height.min(12).max(7);
        let overlay_area = centered_rect(overlay_width, overlay_height, layout[1]);
        let help_lines = vec![
            Line::from("Navigation:"),
            Line::from("  j/k or arrow keys  move selection"),
            Line::from("  tab cycles focus between issues/teams/states"),
            Line::from("Actions:"),
            Line::from("  r refreshes issues   c clears filters   q exits"),
            Line::from("  t / s cycle team or state filters"),
            Line::from("  view next/prev/first/last/<key> jumps to an issue"),
            Line::from("Filters:"),
            Line::from("  / opens contains filter  :team/:state/:contains"),
            Line::from("  clear resets filters  contains clear drops title filter"),
            Line::from("  help opens this overlay"),
            Line::from("Close help with ? or Esc"),
        ];
        let help_overlay = Paragraph::new(help_lines)
            .block(Block::default().title("Help").borders(Borders::ALL))
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(Clear, overlay_area);
        frame.render_widget(help_overlay, overlay_area);
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

fn issue_list_line(
    issue: &linear_core::graphql::IssueSummary,
    filter: Option<&str>,
) -> Line<'static> {
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

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
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
