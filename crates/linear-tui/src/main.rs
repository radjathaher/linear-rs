use std::io;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use linear_core::auth::{AuthManager, FileCredentialStore, OAuthClient, OAuthConfig};
use linear_core::graphql::{LinearGraphqlClient, TeamSummary};
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
            match event::read()? {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('r') => app.load_issues().await,
                    KeyCode::Down | KeyCode::Char('j') => app.move_selection(1).await,
                    KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1).await,
                    KeyCode::Char('t') => app.next_team().await,
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
                        app.status = format!("Loaded {}", detail.identifier);
                        app.detail = Some(detail);
                    }
                    Ok(Ok(None)) => {
                        app.status = "Issue detail unavailable".into();
                        app.detail = None;
                    }
                    Ok(Err(err)) => {
                        app.status = format!("Error loading detail: {err}");
                        app.detail = None;
                    }
                    Err(err) => {
                        app.status = format!("Task error loading detail: {err}");
                        app.detail = None;
                    }
                }
            }
        }
    }
    Ok(())
}

struct App {
    service: IssueService,
    issues: Vec<linear_core::graphql::IssueSummary>,
    detail: Option<linear_core::graphql::IssueDetail>,
    status: String,
    selected: usize,
    pending_detail: Option<JoinHandle<Result<Option<linear_core::graphql::IssueDetail>>>>,
    teams: Vec<TeamSummary>,
    team_index: Option<usize>,
}

impl App {
    fn new(service: IssueService) -> Self {
        Self {
            service,
            issues: Vec::new(),
            detail: None,
            status: "Press 'r' to refresh, arrows to navigate, 'q' to quit".into(),
            selected: 0,
            pending_detail: None,
            teams: Vec::new(),
            team_index: None,
        }
    }

    async fn load_issues(&mut self) {
        self.abort_pending();
        match fetch_issue_summaries(&self.service, self.current_team_id()).await {
            Ok((issues, detail)) => {
                self.issues = issues;
                self.detail = detail;
                self.selected = 0;
                if self.detail.is_none() && !self.issues.is_empty() {
                    let first_key = self.issues[0].identifier.clone();
                    self.queue_detail_fetch(first_key);
                    self.status = format!(
                        "Loading first issue... (team: {})",
                        self.current_team_label()
                    );
                } else {
                    self.status = format!(
                        "Loaded {} issues for {}",
                        self.issues.len(),
                        self.current_team_label()
                    );
                }
            }
            Err(err) => {
                self.issues.clear();
                self.detail = None;
                self.selected = 0;
                self.status = format!("Error: {err}");
            }
        }
    }

    async fn move_selection(&mut self, delta: isize) {
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
                self.status = format!("Loading {}...", key);
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
                    self.status = format!("Failed to load teams: {err}");
                }
            }
        }
    }

    async fn next_team(&mut self) {
        self.ensure_teams().await;
        if self.teams.is_empty() {
            return;
        }
        let next_index = match self.team_index {
            None => Some(0),
            Some(idx) if idx + 1 < self.teams.len() => Some(idx + 1),
            _ => None,
        };
        self.team_index = next_index;
        let team_label = self
            .team_index
            .and_then(|idx| self.teams.get(idx))
            .map(|team| team.key.clone())
            .unwrap_or_else(|| "All".into());
        self.status = format!("Switched to team: {}", team_label);
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
}

async fn fetch_issue_summaries(
    service: &IssueService,
    team_id: Option<String>,
) -> Result<(
    Vec<linear_core::graphql::IssueSummary>,
    Option<linear_core::graphql::IssueDetail>,
)> {
    let issues = service
        .list(IssueQueryOptions {
            limit: 20,
            team_id,
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
    let chunks = Layout::default()
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
        .split(frame.size());

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

    let list = List::new(items)
        .block(Block::default().title("Issues").borders(Borders::ALL))
        .highlight_style(Style::default().fg(Color::Yellow));
    frame.render_stateful_widget(list, chunks[0], &mut list_state);

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
    frame.render_widget(detail, chunks[1]);

    let status = Paragraph::new(app.status.clone()).style(Style::default().fg(Color::Cyan));
    frame.render_widget(status, chunks[2]);

    let help = Paragraph::new("Commands: r=refresh  j/k=move  t=next-team  q=quit")
        .style(Style::default());
    frame.render_widget(help, chunks[3]);
}
