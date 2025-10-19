use std::env;

mod tui;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand};
use linear_core::auth::{
    default_redirect_ports, AuthError, AuthManager, CredentialStore, FileCredentialStore,
    OAuthClient, OAuthConfig,
};
use linear_core::graphql::{
    IssueDetail, IssueSummary, LinearGraphqlClient, TeamSummary, Viewer, WorkflowStateSummary,
};
use linear_core::services::issues::{IssueCreateOptions, IssueQueryOptions, IssueService};
use pulldown_cmark::{Event, Options, Parser as MarkdownParser, Tag, TagEnd};
use serde_json::json;
use textwrap::wrap;
use tokio::task;
use url::Url;

const DEFAULT_PROFILE: &str = "default";

#[derive(Parser, Debug)]
#[command(author, version, about = "Linear terminal CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Authentication related commands
    #[command(subcommand)]
    Auth(AuthCommand),
    /// User account details
    #[command(subcommand)]
    User(UserCommand),
    /// Issue operations
    #[command(subcommand)]
    Issue(IssueCommand),
    /// Team metadata
    #[command(subcommand)]
    Team(TeamCommand),
    /// Workflow state metadata
    #[command(subcommand)]
    State(StateCommand),
    /// Launch interactive TUI
    Tui(TuiArgs),
}

#[derive(Subcommand, Debug)]
enum AuthCommand {
    /// Log in to Linear using OAuth, API keys, or client credentials
    Login(LoginArgs),
    /// Forget stored credentials for a profile
    Logout(LogoutArgs),
}

#[derive(Subcommand, Debug)]
enum UserCommand {
    /// Show the current authenticated user (viewer)
    Me(MeArgs),
}

#[derive(Args, Debug)]
struct MeArgs {
    /// Profile name for stored credentials
    #[arg(long, default_value = DEFAULT_PROFILE)]
    profile: String,
    /// Output raw JSON
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand, Debug)]
enum IssueCommand {
    /// List recent issues
    List(IssueListArgs),
    /// View a single issue by key (e.g. ENG-123)
    View(IssueViewArgs),
    /// Create a new issue
    Create(IssueCreateArgs),
}

#[derive(Subcommand, Debug)]
enum TeamCommand {
    /// List all accessible teams
    List(TeamListArgs),
}

#[derive(Subcommand, Debug)]
enum StateCommand {
    /// List workflow states for a team
    List(StateListArgs),
}

#[derive(Args, Debug)]
struct IssueListArgs {
    /// Profile name for stored credentials
    #[arg(long, default_value = DEFAULT_PROFILE)]
    profile: String,
    /// Maximum number of issues to return
    #[arg(long, default_value_t = 20)]
    limit: usize,
    /// Filter by team key (e.g. ENG)
    #[arg(long = "team-key")]
    team_key: Option<String>,
    /// Filter by team id (if known)
    #[arg(long = "team-id")]
    team_id: Option<String>,
    /// Filter by team key/name/id (resolved automatically)
    #[arg(long = "team")]
    team: Option<String>,
    /// Filter by state id
    #[arg(long = "state-id")]
    state_id: Option<String>,
    /// Filter by state name (requires team context)
    #[arg(long = "state")]
    state: Option<String>,
    /// Filter by assignee id
    #[arg(long = "assignee-id")]
    assignee_id: Option<String>,
    /// Filter by label ids (repeatable)
    #[arg(long = "label-id")]
    label_ids: Vec<String>,
    /// Match issues whose title contains the term
    #[arg(long = "contains")]
    contains: Option<String>,
    /// Output raw JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct IssueViewArgs {
    /// Issue key (e.g. ENG-123)
    key: String,
    /// Profile name for stored credentials
    #[arg(long, default_value = DEFAULT_PROFILE)]
    profile: String,
    /// Output raw JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct IssueCreateArgs {
    /// Profile name for stored credentials
    #[arg(long, default_value = DEFAULT_PROFILE)]
    profile: String,
    /// Team key/name/id for the issue (resolved automatically)
    #[arg(long = "team", required_unless_present = "team-id")]
    team: Option<String>,
    /// Explicit team id for the issue
    #[arg(long = "team-id", required_unless_present = "team")]
    team_id: Option<String>,
    /// Issue title
    #[arg(long)]
    title: String,
    /// Issue description (Markdown supported)
    #[arg(long)]
    description: Option<String>,
    /// Assign to a user by id
    #[arg(long = "assignee-id")]
    assignee_id: Option<String>,
    /// Explicit workflow state id
    #[arg(long = "state-id", conflicts_with = "state")]
    state_id: Option<String>,
    /// Workflow state name (requires --team/--team-id)
    #[arg(long = "state")]
    state: Option<String>,
    /// Apply label ids (repeatable)
    #[arg(long = "label-id")]
    label_ids: Vec<String>,
    /// Priority (0-4)
    #[arg(long, value_parser = clap::value_parser!(i32).range(0..=4))]
    priority: Option<i32>,
    /// Output raw JSON instead of formatted text
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct TuiArgs {
    /// Profile name for stored credentials
    #[arg(long, default_value = DEFAULT_PROFILE)]
    profile: String,
}

#[derive(Args, Debug)]
struct TeamListArgs {
    /// Profile name for stored credentials
    #[arg(long, default_value = DEFAULT_PROFILE)]
    profile: String,
    /// Output raw JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct StateListArgs {
    /// Profile name for stored credentials
    #[arg(long, default_value = DEFAULT_PROFILE)]
    profile: String,
    /// Team identifier (key, name, or id)
    #[arg(long = "team")]
    team: String,
    /// Output raw JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct LoginArgs {
    /// Authenticate with a personal API key instead of OAuth
    #[arg(long = "api-key")]
    api_key: Option<String>,
    /// Use manual copy/paste flow instead of launching a browser
    #[arg(long)]
    manual: bool,
}

#[derive(Args, Debug)]
struct LogoutArgs {
    /// Profile name for stored credentials
    #[arg(long, default_value = DEFAULT_PROFILE)]
    profile: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Auth(cmd) => match cmd {
            AuthCommand::Login(args) => auth_login(args).await?,
            AuthCommand::Logout(args) => auth_logout(args).await?,
        },
        Commands::User(cmd) => match cmd {
            UserCommand::Me(args) => user_me(args).await?,
        },
        Commands::Issue(cmd) => match cmd {
            IssueCommand::List(args) => issue_list(args).await?,
            IssueCommand::View(args) => issue_view(args).await?,
            IssueCommand::Create(args) => issue_create(args).await?,
        },
        Commands::Team(cmd) => match cmd {
            TeamCommand::List(args) => team_list(args).await?,
        },
        Commands::State(cmd) => match cmd {
            StateCommand::List(args) => state_list(args).await?,
        },
        Commands::Tui(args) => tui::run(&args.profile).await?,
    }
    Ok(())
}

async fn auth_login(args: LoginArgs) -> Result<()> {
    let store = FileCredentialStore::with_default_locator()
        .context("unable to initialise credential store")?;

    let oauth = OAuthClient::new(build_oauth_config()?).context("failed to build OAuth client")?;

    let manager = AuthManager::new(store, oauth, DEFAULT_PROFILE);

    if let Some(api_key) = args.api_key {
        manager
            .authenticate_api_key(api_key)
            .await
            .context("failed to store API key")?;
        println!("Personal API key stored for profile '{}'.", DEFAULT_PROFILE);
        return Ok(());
    }

    let session = if args.manual {
        manager
            .authenticate_manual(false, print_authorization_url, || async {
                prompt_for_code().await
            })
            .await
    } else {
        match manager
            .authenticate_browser_auto_port(true, print_authorization_url, default_redirect_ports())
            .await
        {
            Ok(session) => Ok(session),
            Err(AuthError::BrowserLaunch(reason)) => {
                eprintln!(
                    "Failed to launch browser ({reason}); falling back to manual copy/paste flow."
                );
                manager
                    .authenticate_manual(false, print_authorization_url, || async {
                        prompt_for_code().await
                    })
                    .await
            }
            Err(AuthError::NoAvailablePort) => {
                eprintln!(
                    "Unable to bind to a loopback port between 9000 and 9999; using manual copy/paste flow."
                );
                manager
                    .authenticate_manual(false, print_authorization_url, || async {
                        prompt_for_code().await
                    })
                    .await
            }
            Err(other) => Err(other),
        }
    }?;

    let identity = match LinearGraphqlClient::from_session(&session) {
        Ok(client) => match client.viewer().await {
            Ok(viewer) => viewer
                .email
                .clone()
                .or_else(|| viewer.display_name.clone())
                .or_else(|| viewer.name.clone())
                .unwrap_or(viewer.id.clone()),
            Err(err) => {
                eprintln!("Login succeeded but viewer query failed: {err}");
                session.scope.join(", ")
            }
        },
        Err(err) => {
            eprintln!("Login succeeded but failed to build GraphQL client: {err}");
            session.scope.join(", ")
        }
    };

    println!(
        "Login succeeded. Credentials stored for profile '{}'.",
        DEFAULT_PROFILE
    );
    if !identity.is_empty() {
        println!("Logged in as {}", identity);
    }
    if let Some(expiry) = session.expires_at {
        println!("Token expires at {} (UTC).", expiry);
    }

    Ok(())
}

async fn auth_logout(args: LogoutArgs) -> Result<()> {
    let store = FileCredentialStore::with_default_locator()
        .context("unable to initialise credential store")?;
    store
        .delete(&args.profile)
        .context("failed to remove stored credentials")?;
    println!("Deleted credentials for profile '{}'.", args.profile);
    Ok(())
}

fn build_oauth_config() -> Result<OAuthConfig> {
    let mut config = OAuthConfig::with_defaults();

    if let Ok(client_id) = env::var("LINEAR_CLIENT_ID") {
        if !client_id.trim().is_empty() {
            config.client_id = client_id;
        }
    }

    if let Ok(redirect) = env::var("LINEAR_REDIRECT_URI") {
        if !redirect.trim().is_empty() {
            config.redirect_uri = Url::parse(&redirect).context("invalid LINEAR_REDIRECT_URI")?;
        }
    }

    if let Ok(secret) = env::var("LINEAR_CLIENT_SECRET") {
        if !secret.trim().is_empty() {
            config = config.with_secret(secret);
        }
    }

    if let Ok(scopes) = env::var("LINEAR_SCOPES") {
        let requested = scopes
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !requested.is_empty() {
            config = config.with_scopes(requested);
        }
    }

    Ok(config)
}

async fn prompt_for_code() -> Result<String, linear_core::auth::AuthError> {
    task::spawn_blocking(|| {
        use std::io::{self, Write};
        print!("Paste the verification code or redirect URL: ");
        io::stdout()
            .flush()
            .map_err(linear_core::auth::AuthError::Io)?;
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(linear_core::auth::AuthError::Io)?;
        Ok(input.trim().to_owned())
    })
    .await
    .map_err(|_| linear_core::auth::AuthError::Cancelled)?
}

fn print_authorization_url(url: &Url) -> Result<(), linear_core::auth::AuthError> {
    println!("\nAuthorize the application by visiting:\n  {}\n", url);
    Ok(())
}

async fn user_me(args: MeArgs) -> Result<()> {
    let session = load_session(&args.profile).await?;
    let client =
        LinearGraphqlClient::from_session(&session).context("failed to build GraphQL client")?;
    let viewer = client.viewer().await.context("GraphQL request failed")?;

    if args.json {
        let json = serde_json::to_string_pretty(&viewer)?;
        println!("{}", json);
    } else {
        render_viewer(&viewer);
    }

    Ok(())
}

pub(crate) async fn load_session(profile: &str) -> Result<linear_core::auth::AuthSession> {
    let store = FileCredentialStore::with_default_locator()
        .context("unable to initialise credential store")?;
    let oauth = OAuthClient::new(build_oauth_config()?).context("failed to build OAuth client")?;
    let manager = AuthManager::new(store, oauth, profile);
    manager.ensure_fresh_session().await?.ok_or_else(|| {
        anyhow!(
            "no credentials stored for profile '{}'; run `linear auth login`",
            profile
        )
    })
}

fn render_viewer(viewer: &Viewer) {
    println!("Viewer ID: {}", viewer.id);
    if let Some(name) = &viewer.name {
        println!("Name      : {}", name);
    }
    if let Some(display) = &viewer.display_name {
        println!("Display   : {}", display);
    }
    if let Some(email) = &viewer.email {
        println!("Email     : {}", email);
    }
    println!("Created   : {}", viewer.created_at.to_rfc3339());
}

async fn issue_list(args: IssueListArgs) -> Result<()> {
    let session = load_session(&args.profile).await?;
    let client =
        LinearGraphqlClient::from_session(&session).context("failed to build GraphQL client")?;
    let service = IssueService::new(client);
    let mut options = IssueQueryOptions {
        limit: args.limit,
        team_key: args.team_key.clone(),
        team_id: args.team_id.clone(),
        assignee_id: args.assignee_id.clone(),
        state_id: args.state_id.clone(),
        label_ids: args.label_ids.clone(),
        title_contains: args.contains.clone(),
        after: None,
        ..Default::default()
    };

    if options.team_id.is_none() {
        if let Some(team_input) = args.team.clone() {
            options.team_id = Some(
                service
                    .resolve_team_id(&team_input)
                    .await?
                    .ok_or_else(|| anyhow!("team '{}' not found", team_input))?,
            );
            options.team_key = None;
        } else if let Some(team_id) = args.team_id.clone() {
            options.team_id = Some(team_id);
        }
    }

    if let Some(state_name) = args.state.clone() {
        let team_id = options
            .team_id
            .as_ref()
            .ok_or_else(|| anyhow!("--state requires --team/--team-id to resolve workflow"))?;
        options.state_id = Some(
            service
                .resolve_state_id(team_id, &state_name)
                .await?
                .ok_or_else(|| anyhow!("state '{}' not found for team", state_name))?,
        );
    }

    let issues = service
        .list(options)
        .await
        .context("GraphQL request failed")?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&issues)?);
    } else {
        render_issue_list(&issues.issues);
        if issues.has_next_page {
            eprintln!("… more issues available (use pagination commands in the TUI)");
        }
    }

    Ok(())
}

async fn issue_create(args: IssueCreateArgs) -> Result<()> {
    let session = load_session(&args.profile).await?;
    let client =
        LinearGraphqlClient::from_session(&session).context("failed to build GraphQL client")?;
    let service = IssueService::new(client);

    let team_id = match (&args.team_id, &args.team) {
        (Some(id), _) => id.clone(),
        (None, Some(team_input)) => service
            .resolve_team_id(team_input)
            .await?
            .ok_or_else(|| anyhow!("team '{}' not found", team_input))?,
        (None, None) => return Err(anyhow!("--team or --team-id is required")),
    };

    let mut state_id = args.state_id.clone();
    if state_id.is_none() {
        if let Some(state_name) = &args.state {
            state_id = Some(
                service
                    .resolve_state_id(&team_id, state_name)
                    .await?
                    .ok_or_else(|| anyhow!("state '{}' not found for team", state_name))?,
            );
        }
    }

    let mut options = IssueCreateOptions::new(team_id, args.title.clone());
    options.description = args.description.clone();
    options.assignee_id = args.assignee_id.clone();
    options.state_id = state_id;
    options.label_ids = args.label_ids.clone();
    options.priority = args.priority;

    let issue = service
        .create(options)
        .await
        .context("GraphQL request failed")?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&issue)?);
    } else {
        println!("Created {}", issue.identifier);
        println!();
        render_issue_detail(&issue);
    }

    Ok(())
}

async fn issue_view(args: IssueViewArgs) -> Result<()> {
    let session = load_session(&args.profile).await?;
    let client =
        LinearGraphqlClient::from_session(&session).context("failed to build GraphQL client")?;
    let service = IssueService::new(client);
    let issue = service
        .get_by_key(&args.key)
        .await
        .context("GraphQL request failed")?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&issue)?);
    } else {
        render_issue_detail(&issue);
    }

    Ok(())
}

async fn team_list(args: TeamListArgs) -> Result<()> {
    let session = load_session(&args.profile).await?;
    let client =
        LinearGraphqlClient::from_session(&session).context("failed to build GraphQL client")?;
    let service = IssueService::new(client);
    let teams = service.teams().await.context("GraphQL request failed")?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&teams)?);
    } else {
        render_team_list(&teams);
    }

    Ok(())
}

async fn state_list(args: StateListArgs) -> Result<()> {
    let session = load_session(&args.profile).await?;
    let client =
        LinearGraphqlClient::from_session(&session).context("failed to build GraphQL client")?;
    let service = IssueService::new(client);
    let result = service
        .workflow_states_for_team(&args.team)
        .await?
        .ok_or_else(|| anyhow!("team '{}' not found", args.team))?;
    let (team, states) = result;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "team": team,
                "states": states,
            }))?
        );
    } else {
        println!("Team: {} ({})", team.name, team.key);
        render_state_list(&states);
    }

    Ok(())
}

fn render_issue_list(issues: &[IssueSummary]) {
    println!(
        "{:<12} {:<40} {:<16} {:<20} {:<8}",
        "IDENTIFIER", "TITLE", "STATE", "ASSIGNEE", "PRIOR"
    );
    println!("{}", "-".repeat(100));
    for issue in issues {
        let state = issue.state.as_ref().map(|s| s.name.as_str()).unwrap_or("-");
        let assignee = issue
            .assignee
            .as_ref()
            .and_then(|a| a.display_name.as_deref().or(a.name.as_deref()))
            .unwrap_or("-");
        println!(
            "{:<12} {:<40} {:<16} {:<20} {:<8}",
            issue.identifier,
            truncate(&issue.title, 40),
            truncate(state, 16),
            truncate(assignee, 20),
            issue.priority.map_or("-".to_string(), |p| p.to_string())
        );
    }
}

fn render_issue_detail(issue: &IssueDetail) {
    println!("{} — {}", issue.identifier, issue.title);
    if let Some(url) = &issue.url {
        println!("URL       : {}", url);
    }
    if let Some(state) = &issue.state {
        println!("State     : {}", state.name);
    }
    if let Some(assignee) = &issue.assignee {
        let name = assignee
            .display_name
            .as_ref()
            .or(assignee.name.as_ref())
            .cloned()
            .unwrap_or_else(|| "Unassigned".into());
        println!("Assignee  : {}", name);
    }
    if let Some(priority) = issue.priority {
        println!("Priority  : {}", priority);
    }
    let labels = issue
        .labels
        .as_ref()
        .map(|c| c.nodes.iter().map(|l| l.name.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();
    if !labels.is_empty() {
        println!("Labels    : {}", labels.join(", "));
    }
    println!("Created   : {}", issue.created_at.to_rfc3339());
    println!("Updated   : {}", issue.updated_at.to_rfc3339());

    if let Some(description) = &issue.description {
        let trimmed = description.trim();
        if !trimmed.is_empty() {
            println!();
            let width = 80;
            let plain = markdown_to_text(trimmed);
            for line in wrap(plain.trim(), width) {
                println!("{}", line);
            }
            println!();
        }
    }
}

fn truncate(value: &str, max_len: usize) -> String {
    let mut chars = value.chars();
    let mut collected = String::new();
    for _ in 0..max_len.saturating_sub(1) {
        match chars.next() {
            Some(ch) => collected.push(ch),
            None => return value.to_owned(),
        }
    }
    if chars.next().is_some() {
        collected.push('…');
        collected
    } else {
        value.to_owned()
    }
}

fn render_team_list(teams: &[TeamSummary]) {
    println!("{:<8} {:<32} {:<36}", "KEY", "NAME", "ID");
    println!("{}", "-".repeat(80));
    for team in teams {
        println!(
            "{:<8} {:<32} {:<36}",
            team.key,
            truncate(&team.name, 32),
            truncate(&team.id, 36)
        );
    }
}

fn render_state_list(states: &[WorkflowStateSummary]) {
    println!("{:<25} {:<15} {:<36}", "NAME", "TYPE", "ID");
    println!("{}", "-".repeat(80));
    for state in states {
        println!(
            "{:<25} {:<15} {:<36}",
            truncate(&state.name, 25),
            truncate(state.type_name.as_deref().unwrap_or("-"), 15),
            truncate(&state.id, 36)
        );
    }
}

fn markdown_to_text(input: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = MarkdownParser::new_ext(input, options);
    let mut out = String::new();
    let mut need_space = false;
    for event in parser {
        match event {
            Event::Text(text) | Event::Code(text) => {
                if need_space && !out.ends_with([' ', '\n']) {
                    out.push(' ');
                }
                out.push_str(&text);
                need_space = true;
            }
            Event::SoftBreak => {
                out.push(' ');
                need_space = false;
            }
            Event::HardBreak => {
                out.push('\n');
                need_space = false;
            }
            Event::Start(Tag::Paragraph) => {
                if !out.ends_with('\n') && !out.is_empty() {
                    out.push('\n');
                }
                need_space = false;
            }
            Event::End(TagEnd::Paragraph) => {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                need_space = false;
            }
            Event::Start(Tag::List(_)) => {
                if !out.ends_with('\n') && !out.is_empty() {
                    out.push('\n');
                }
            }
            Event::Start(Tag::Item) => {
                if !out.ends_with('\n') && !out.is_empty() {
                    out.push('\n');
                }
                out.push_str("- ");
                need_space = false;
            }
            Event::End(TagEnd::Item) => {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                need_space = false;
            }
            _ => {}
        }
    }
    out.trim().to_string()
}
