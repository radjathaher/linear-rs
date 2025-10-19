use std::io;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use linear_core::graphql::LinearGraphqlClient;
use linear_core::services::cycles::CycleService;
use linear_core::services::issues::IssueService;
use linear_core::services::projects::ProjectService;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::tui::app::{App, Focus, StatusTab};
use crate::tui::view::render_app;

pub async fn run(profile: &str) -> Result<()> {
    let session = crate::load_session(profile).await?;
    let client =
        LinearGraphqlClient::from_session(&session).context("failed to build GraphQL client")?;
    let issue_service = IssueService::new(client.clone());
    let project_service = ProjectService::new(client.clone());
    let cycle_service = CycleService::new(client);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(&mut stdout, crossterm::terminal::EnterAlternateScreen)?;
    crossterm::execute!(&mut stdout, crossterm::event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(
        issue_service,
        project_service,
        cycle_service,
        profile.to_string(),
    );
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
            if let Event::Key(key_event) = evt {
                if key_event.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key_event.code, KeyCode::Enter)
                {
                    app.trigger_cli_action();
                    continue;
                }
            }
            if app.show_help_overlay() {
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

            if app.show_projects_overlay() {
                if let Event::Key(key) = evt {
                    match key.code {
                        KeyCode::Char('o') | KeyCode::Char('O') | KeyCode::Esc => {
                            app.close_projects_overlay();
                        }
                        _ => {}
                    }
                }
                continue;
            }

            if app.show_cycles_overlay() {
                if let Event::Key(key) = evt {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Esc => {
                            app.close_cycles_overlay();
                        }
                        _ => {}
                    }
                }
                continue;
            }

            if app.palette_active() {
                if let Event::Key(key) = evt {
                    match key.code {
                        KeyCode::Esc => {
                            app.exit_palette();
                        }
                        KeyCode::Enter => {
                            let cmd = app.take_palette_input();
                            app.execute_command(cmd).await;
                        }
                        KeyCode::Backspace => {
                            app.pop_palette_char();
                        }
                        KeyCode::Up => {
                            app.recall_palette_history(-1);
                        }
                        KeyCode::Down => {
                            app.recall_palette_history(1);
                        }
                        KeyCode::Char(c) => {
                            app.push_palette_char(c);
                        }
                        _ => {}
                    }
                }
                continue;
            }

            match evt {
                Event::Key(key) => {
                    let modifiers = key.modifiers;
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('r') | KeyCode::Char('R') if modifiers.is_empty() => {
                            app.load_issues().await
                        }
                        KeyCode::Char('p') | KeyCode::Char('P') => {
                            if modifiers.contains(KeyModifiers::CONTROL) {
                                app.clear_project_filter().await;
                            } else if modifiers.contains(KeyModifiers::SHIFT)
                                || matches!(key.code, KeyCode::Char('P'))
                            {
                                app.cycle_project_filter(-1).await;
                            } else {
                                app.cycle_project_filter(1).await;
                            }
                        }
                        KeyCode::Char('o') | KeyCode::Char('O') => {
                            app.open_projects_overlay().await
                        }
                        KeyCode::Char('y') | KeyCode::Char('Y') => app.open_cycles_overlay().await,
                        KeyCode::Char('1') => app.set_status_tab(StatusTab::Todo).await,
                        KeyCode::Char('2') => app.set_status_tab(StatusTab::Doing).await,
                        KeyCode::Char('3') => app.set_status_tab(StatusTab::Done).await,
                        KeyCode::Char('4') => app.set_status_tab(StatusTab::All).await,
                        KeyCode::Char(']') => {
                            if modifiers.contains(KeyModifiers::CONTROL) {
                                app.cycle_status_tab(1).await;
                            } else if app.has_next_page() {
                                app.next_page().await;
                            } else {
                                app.set_status("No more issues", false);
                            }
                        }
                        KeyCode::Char('[') => {
                            if modifiers.contains(KeyModifiers::CONTROL) {
                                app.cycle_status_tab(-1).await;
                            } else {
                                app.previous_page().await;
                            }
                        }
                        KeyCode::Char('.') if !modifiers.contains(KeyModifiers::CONTROL) => {
                            app.cycle_detail_tab(1);
                        }
                        KeyCode::Char(',') if !modifiers.contains(KeyModifiers::CONTROL) => {
                            app.cycle_detail_tab(-1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => match app.focus() {
                            Focus::Issues => app.move_issue_selection(1).await,
                            Focus::Teams => app.move_team_selection(1).await,
                            Focus::States => app.move_state_selection(1).await,
                        },
                        KeyCode::Up | KeyCode::Char('k') => match app.focus() {
                            Focus::Issues => app.move_issue_selection(-1).await,
                            Focus::Teams => app.move_team_selection(-1).await,
                            Focus::States => app.move_state_selection(-1).await,
                        },
                        KeyCode::Tab => app.toggle_focus(),
                        KeyCode::Char('t') | KeyCode::Char('T')
                            if !modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            app.move_team_selection(1).await
                        }
                        KeyCode::Char('s') | KeyCode::Char('S')
                            if !modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            app.move_state_selection(1).await
                        }
                        KeyCode::Char('/') => app.enter_contains_palette(),
                        KeyCode::Char('c') | KeyCode::Char('C')
                            if !modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            app.clear_all_filters().await
                        }
                        KeyCode::Char('?') => app.toggle_help_overlay(),
                        KeyCode::Char(':') => app.enter_palette(),
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        app.process_pending_detail().await;
        app.process_automation().await;

        if app.status_spinner_active() {
            app.tick_status_spinner();
        }
    }
    Ok(())
}
