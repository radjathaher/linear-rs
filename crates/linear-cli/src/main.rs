use std::env;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand};
use linear_core::auth::{
    AuthFlow, AuthManager, CredentialStore, FileCredentialStore, FlowPreference, OAuthClient,
    OAuthConfig,
};
use linear_core::graphql::{IssueDetail, IssueSummary, LinearGraphqlClient, Viewer};
use tokio::task;
use url::Url;

const DEFAULT_PROFILE: &str = "default";
const DEFAULT_SCOPES: &[&str] = &["read", "write"];

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
}

#[derive(Args, Debug)]
struct IssueListArgs {
    /// Profile name for stored credentials
    #[arg(long, default_value = DEFAULT_PROFILE)]
    profile: String,
    /// Maximum number of issues to return
    #[arg(long, default_value_t = 20)]
    limit: usize,
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
struct LoginArgs {
    /// Profile name for stored credentials
    #[arg(long, default_value = DEFAULT_PROFILE)]
    profile: String,
    /// Do not attempt to launch a browser automatically
    #[arg(long)]
    no_browser: bool,
    /// Force the manual copy/paste flow
    #[arg(long)]
    manual: bool,
    /// Force the browser loopback flow
    #[arg(long)]
    browser: bool,
    /// Authenticate with a personal API key instead of OAuth
    #[arg(long)]
    api_key: Option<String>,
    /// Authenticate using the client credentials grant (requires client secret)
    #[arg(long = "client-credentials")]
    client_credentials: bool,
    /// Scopes requested when using client credentials
    #[arg(long = "scope")]
    scopes: Vec<String>,
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
        },
    }
    Ok(())
}

async fn auth_login(args: LoginArgs) -> Result<()> {
    let store = FileCredentialStore::with_default_locator()
        .context("unable to initialise credential store")?;

    let mut config = build_oauth_config()?;
    config = config.with_scopes(DEFAULT_SCOPES.iter().copied());
    let oauth = OAuthClient::new(config).context("failed to build OAuth client")?;

    let manager = AuthManager::new(store, oauth, &args.profile);

    if let Some(api_key) = args.api_key {
        manager
            .authenticate_api_key(api_key)
            .await
            .context("failed to store API key")?;
        println!("Personal API key stored for profile '{}'.", args.profile);
        return Ok(());
    }

    if args.client_credentials {
        ensure_client_secret_present()?;
        let scopes = if args.scopes.is_empty() {
            DEFAULT_SCOPES.iter().map(|s| s.to_string()).collect()
        } else {
            args.scopes.clone()
        };
        let session = manager
            .authenticate_client_credentials(&scopes)
            .await
            .context("client credentials flow failed")?;
        println!(
            "Client credentials token stored for profile '{}' (scopes: {}).",
            args.profile,
            session.scope.join(", ")
        );
        return Ok(());
    }

    let open_browser = !args.no_browser;

    let result = if args.manual {
        manager
            .authenticate_manual(open_browser, print_authorization_url, || async {
                prompt_for_code().await
            })
            .await
    } else if args.browser {
        manager
            .authenticate_browser(open_browser, print_authorization_url)
            .await
    } else {
        let preference = FlowPreference::detect();
        let open = open_browser && preference.browser_available();
        match preference.preferred() {
            AuthFlow::Browser => {
                manager
                    .authenticate_browser(open, print_authorization_url)
                    .await
            }
            _ => {
                manager
                    .authenticate_manual(open_browser, print_authorization_url, || async {
                        prompt_for_code().await
                    })
                    .await
            }
        }
    };

    match result {
        Ok(session) => {
            match session.expires_at {
                Some(expiry) => println!("Login succeeded; token expires at {} (UTC).", expiry),
                None => println!("Login succeeded; token stored."),
            }
            println!("Profile '{}': credentials saved.", args.profile);
            Ok(())
        }
        Err(err) => Err(anyhow!("authentication failed: {err}")),
    }
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
    let client_id = env::var("LINEAR_CLIENT_ID")
        .context("LINEAR_CLIENT_ID environment variable is required")?;
    let redirect = env::var("LINEAR_REDIRECT_URI")
        .context("LINEAR_REDIRECT_URI environment variable is required")?;
    let redirect_uri = Url::parse(&redirect).context("invalid LINEAR_REDIRECT_URI")?;

    let mut config = OAuthConfig::new(client_id, redirect_uri);
    if let Ok(secret) = env::var("LINEAR_CLIENT_SECRET") {
        if !secret.is_empty() {
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
        } else {
            config = config.with_scopes(DEFAULT_SCOPES.iter().copied());
        }
    } else {
        config = config.with_scopes(DEFAULT_SCOPES.iter().copied());
    }

    Ok(config)
}

fn ensure_client_secret_present() -> Result<()> {
    match env::var("LINEAR_CLIENT_SECRET") {
        Ok(secret) if !secret.is_empty() => Ok(()),
        _ => Err(anyhow!(
            "client credentials flow requires LINEAR_CLIENT_SECRET to be set"
        )),
    }
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

async fn load_session(profile: &str) -> Result<linear_core::auth::AuthSession> {
    let store = FileCredentialStore::with_default_locator()
        .context("unable to initialise credential store")?;

    match build_oauth_config() {
        Ok(config) => {
            let oauth = OAuthClient::new(config).context("failed to build OAuth client")?;
            let manager = AuthManager::new(store, oauth, profile);
            manager.ensure_fresh_session().await?.ok_or_else(|| {
                anyhow!(
                    "no credentials stored for profile '{}'; run `linear auth login`",
                    profile
                )
            })
        }
        Err(_) => store
            .load(profile)
            .map_err(anyhow::Error::from)?
            .ok_or_else(|| {
                anyhow!(
                    "no credentials stored for profile '{}'; run `linear auth login`",
                    profile
                )
            }),
    }
}

fn render_viewer(viewer: &Viewer) {
    println!("Viewer ID: {}", viewer.id);
    if let Some(name) = &viewer.name {
        println!("Name      : {}", name);
    }
    if let Some(display) = &viewer.display_name {
        println!("Display   : {}", display);
    }
    if let Some(handle) = &viewer.handle {
        println!("Handle    : @{}", handle);
    }
    if let Some(email) = &viewer.email {
        println!("Email     : {}", email);
    }
    println!("Created   : {}", viewer.created_at.to_rfc3339());
}

async fn issue_list(args: IssueListArgs) -> Result<()> {
    let session = load_session(&args.profile).await?;
    let client = LinearGraphqlClient::from_session(&session)
        .context("failed to build GraphQL client")?;
    let issues = client
        .list_issues(args.limit)
        .await
        .context("GraphQL request failed")?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&issues)?);
    } else {
        render_issue_list(&issues);
    }

    Ok(())
}

async fn issue_view(args: IssueViewArgs) -> Result<()> {
    let session = load_session(&args.profile).await?;
    let client = LinearGraphqlClient::from_session(&session)
        .context("failed to build GraphQL client")?;
    let issue = client
        .issue_by_key(&args.key)
        .await
        .context("GraphQL request failed")?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&issue)?);
    } else {
        render_issue_detail(&issue);
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
        let state = issue
            .state
            .as_ref()
            .map(|s| s.name.as_str())
            .unwrap_or("-");
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
        if !description.trim().is_empty() {
            println!("\n{}
", description.trim());
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
