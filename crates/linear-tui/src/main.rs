use std::io;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use linear_core::auth::{AuthManager, FileCredentialStore, OAuthClient, OAuthConfig};
use linear_core::graphql::LinearGraphqlClient;
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
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(&mut stdout, crossterm::terminal::EnterAlternateScreen)?;
    crossterm::execute!(&mut stdout, crossterm::event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
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
    issues: Vec<linear_core::graphql::IssueSummary>,
    detail: Option<linear_core::graphql::IssueDetail>,
    status: String,
    selected: usize,
    pending_detail: Option<JoinHandle<Result<Option<linear_core::graphql::IssueDetail>>>>,
}

impl App {
    fn new() -> Self {
        Self {
            issues: Vec::new(),
            detail: None,
            status: "Press 'r' to refresh, arrows to navigate, 'q' to quit".into(),
            selected: 0,
            pending_detail: None,
        }
    }

    async fn load_issues(&mut self) {
        self.abort_pending();
        match fetch_issue_summaries().await {
            Ok((issues, detail)) => {
                self.issues = issues;
                self.detail = detail;
                self.selected = 0;
                if self.detail.is_none() && !self.issues.is_empty() {
                    let first_key = self.issues[0].identifier.clone();
                    self.queue_detail_fetch(first_key);
                    self.status = "Loading first issue...".into();
                } else {
                    self.status = "Loaded issues".into();
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
        self.pending_detail = Some(tokio::spawn(fetch_issue_detail(
            DEFAULT_PROFILE.to_string(),
            key,
        )));
    }

    fn abort_pending(&mut self) {
        if let Some(handle) = self.pending_detail.take() {
            handle.abort();
        }
    }
}

async fn fetch_issue_summaries() -> Result<(
    Vec<linear_core::graphql::IssueSummary>,
    Option<linear_core::graphql::IssueDetail>,
)> {
    let session = match load_session(DEFAULT_PROFILE).await {
        Ok(session) => session,
        Err(err) => return Err(anyhow!("authentication error: {err}")),
    };

    let client =
        LinearGraphqlClient::from_session(&session).context("failed to build GraphQL client")?;
    let service = IssueService::new(client);
    let issues = service
        .list(IssueQueryOptions {
            limit: 20,
            ..Default::default()
        })
        .await
        .context("failed to fetch issues")?;

    let detail = if let Some(first) = issues.get(0) {
        fetch_issue_detail(DEFAULT_PROFILE.to_string(), first.identifier.clone()).await?
    } else {
        None
    };

    Ok((issues, detail))
}

async fn fetch_issue_detail(
    profile: String,
    key: String,
) -> Result<Option<linear_core::graphql::IssueDetail>> {
    let session = load_session(&profile).await?;
    let client =
        LinearGraphqlClient::from_session(&session).context("failed to build GraphQL client")?;
    let service = IssueService::new(client);
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

    let help = Paragraph::new("Commands: r=refresh  j/k=move  q=quit").style(Style::default());
    frame.render_widget(help, chunks[3]);
}
