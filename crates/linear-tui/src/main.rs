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
                    _ => {}
                },
                _ => {}
            }
        }
    }
    Ok(())
}

struct App {
    issues: Vec<String>,
    status: String,
}

impl App {
    fn new() -> Self {
        Self {
            issues: Vec::new(),
            status: "Press 'r' to refresh issues; 'q' to quit".into(),
        }
    }

    async fn load_issues(&mut self) {
        match fetch_issue_summaries().await {
            Ok(issues) => {
                self.issues = issues;
                self.status = "Loaded issues".into();
            }
            Err(err) => {
                self.issues.clear();
                self.status = format!("Error: {err}");
            }
        }
    }
}

async fn fetch_issue_summaries() -> Result<Vec<String>> {
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

    Ok(issues
        .into_iter()
        .map(|issue| format!("{} — {}", issue.identifier, issue.title))
        .collect())
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
        .constraints([Constraint::Min(1), Constraint::Length(3)].as_ref())
        .split(frame.size());

    let items: Vec<ListItem> = if app.issues.is_empty() {
        vec![ListItem::new("No issues loaded")]
    } else {
        app.issues
            .iter()
            .map(|issue| ListItem::new(Line::from(issue.clone())))
            .collect()
    };

    let list = List::new(items).block(Block::default().title("Issues").borders(Borders::ALL));
    frame.render_widget(list, chunks[0]);

    let status = Paragraph::new(app.status.clone()).style(Style::default().fg(Color::Cyan));
    frame.render_widget(status, chunks[1]);
}
