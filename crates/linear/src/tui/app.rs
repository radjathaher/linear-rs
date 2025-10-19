use std::collections::HashMap;
use std::env;
use std::process::Stdio;

use anyhow::{Context, Result};
use linear_core::graphql::{
    CycleSummary, IssueDetail, IssueSummary, ProjectSummary, TeamSummary, WorkflowStateSummary,
};
use linear_core::services::cycles::{CycleQueryOptions, CycleService, CycleSort};
use linear_core::services::issues::{IssueListResult, IssueQueryOptions, IssueService};
use linear_core::services::projects::{ProjectQueryOptions, ProjectService, ProjectSort};
use ratatui::text::Line;
use tokio::process::Command;
use tokio::task::JoinHandle;

const SPINNER_FRAMES: [char; 4] = ['-', '\\', '|', '/'];
const PAGE_SIZE: usize = 20;

pub struct App {
    service: IssueService,
    project_service: ProjectService,
    cycle_service: CycleService,
    profile: String,
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
    show_projects_overlay: bool,
    show_cycles_overlay: bool,
    page: usize,
    has_next_page: bool,
    page_cache: HashMap<usize, PageData>,
    page_cursors: Vec<Option<String>>,
    projects: Vec<ProjectSummary>,
    cycles: Vec<CycleSummary>,
    project_filter_options: Vec<ProjectSummary>,
    project_filter_index: Option<usize>,
    project_filter_cache: HashMap<Option<String>, Vec<ProjectSummary>>,
    status_tab: StatusTab,
    automation_task: Option<JoinHandle<AutomationOutcome>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Teams,
    States,
    Issues,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusTab {
    All,
    Todo,
    Doing,
    Done,
}

impl StatusTab {
    pub const fn all() -> [StatusTab; 4] {
        [
            StatusTab::All,
            StatusTab::Todo,
            StatusTab::Doing,
            StatusTab::Done,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            StatusTab::All => "All",
            StatusTab::Todo => "Todo",
            StatusTab::Doing => "Doing",
            StatusTab::Done => "Done",
        }
    }
}

impl App {
    pub(crate) fn new(
        service: IssueService,
        project_service: ProjectService,
        cycle_service: CycleService,
        profile: impl Into<String>,
    ) -> Self {
        Self {
            service,
            project_service,
            cycle_service,
            profile: profile.into(),
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
            show_projects_overlay: false,
            show_cycles_overlay: false,
            page: 0,
            has_next_page: false,
            page_cache: HashMap::new(),
            page_cursors: Vec::new(),
            projects: Vec::new(),
            cycles: Vec::new(),
            project_filter_options: Vec::new(),
            project_filter_index: None,
            project_filter_cache: HashMap::new(),
            status_tab: StatusTab::All,
            automation_task: None,
        }
    }

    pub(crate) fn set_status(&mut self, message: impl Into<String>, spinner: bool) {
        self.status_base = message.into();
        self.status_spinner = spinner;
        if spinner {
            self.spinner_index = 0;
        }
    }

    fn set_spinner_status(&mut self, message: impl Into<String>) {
        self.set_status(message, true);
    }

    pub(crate) fn status_text(&self) -> String {
        if self.status_spinner {
            let frame = SPINNER_FRAMES[self.spinner_index % SPINNER_FRAMES.len()];
            format!("{} {}", self.status_base, frame)
        } else {
            self.status_base.clone()
        }
    }

    pub(crate) fn status_tab(&self) -> StatusTab {
        self.status_tab
    }

    pub(crate) fn status_tab_label(&self) -> &'static str {
        self.status_tab.label()
    }

    pub(crate) fn filters_text(&self) -> String {
        let team = self.current_team_label();
        let state = self.current_state_label();
        let project = self.current_project_label();
        let status = self.status_tab_label();
        let contains = self
            .title_contains
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| format!("'{}'", s))
            .unwrap_or_else(|| "-".into());
        let mut parts = Vec::new();
        parts.push(format!("team={}", team));
        parts.push(format!("project={}", project));
        parts.push(format!("state={}", state));
        parts.push(format!("status={}", status));
        parts.push(format!("title~{}", contains));
        parts.push(format!("page={}", self.page + 1));
        if let Some(issue) = self.issues.get(self.selected) {
            parts.push(format!("selected={}", issue.identifier));
        }
        format!("Filters: {}", parts.join("  "))
    }

    fn filter_context(&self) -> String {
        format!(
            "team: {}, project: {}, state: {}, status: {}",
            self.current_team_label(),
            self.current_project_label(),
            self.current_state_label(),
            self.status_tab_label()
        )
    }

    fn status_tick(&mut self) {
        if self.status_spinner {
            self.spinner_index = (self.spinner_index + 1) % SPINNER_FRAMES.len();
        }
    }

    pub(crate) fn status_spinner_active(&self) -> bool {
        self.status_spinner
    }

    pub(crate) fn tick_status_spinner(&mut self) {
        if !self.status_spinner {
            return;
        }
        if self.pending_detail.is_some() || self.automation_task.is_some() {
            self.status_tick();
        } else {
            self.status_spinner = false;
        }
    }

    pub(crate) fn show_help_overlay(&self) -> bool {
        self.show_help_overlay
    }

    pub(crate) fn show_projects_overlay(&self) -> bool {
        self.show_projects_overlay
    }

    pub(crate) fn show_cycles_overlay(&self) -> bool {
        self.show_cycles_overlay
    }

    pub(crate) fn palette_active(&self) -> bool {
        self.palette_active
    }

    pub(crate) fn focus(&self) -> Focus {
        self.focus
    }

    pub(crate) fn has_next_page(&self) -> bool {
        self.has_next_page
    }

    pub(crate) fn exit_palette(&mut self) {
        self.palette_active = false;
        self.palette_input.clear();
        self.set_status("Exited command mode", false);
    }

    pub(crate) fn take_palette_input(&mut self) -> String {
        let cmd = self.palette_input.clone();
        self.palette_active = false;
        self.palette_input.clear();
        cmd
    }

    pub(crate) fn pop_palette_char(&mut self) {
        self.palette_input.pop();
    }

    pub(crate) fn push_palette_char(&mut self, c: char) {
        self.palette_input.push(c);
    }

    pub(crate) fn palette_input(&self) -> &str {
        &self.palette_input
    }

    pub(crate) fn palette_history(&self) -> &[String] {
        &self.palette_history
    }

    pub(crate) fn title_contains(&self) -> Option<&str> {
        self.title_contains.as_deref()
    }

    pub(crate) fn issues(&self) -> &[IssueSummary] {
        &self.issues
    }

    pub(crate) fn selected_index(&self) -> usize {
        self.selected
    }

    pub(crate) fn selected_issue(&self) -> Option<&IssueSummary> {
        self.issues.get(self.selected)
    }

    pub(crate) fn detail(&self) -> Option<&IssueDetail> {
        self.detail.as_ref()
    }

    pub(crate) fn teams(&self) -> &[TeamSummary] {
        &self.teams
    }

    pub(crate) fn team_index(&self) -> Option<usize> {
        self.team_index
    }

    pub(crate) fn states(&self) -> &[WorkflowStateSummary] {
        &self.states
    }

    pub(crate) fn state_index(&self) -> Option<usize> {
        self.state_index
    }

    pub(crate) fn projects(&self) -> &[ProjectSummary] {
        &self.projects
    }

    pub(crate) fn cycles(&self) -> &[CycleSummary] {
        &self.cycles
    }

    pub(crate) async fn process_pending_detail(&mut self) {
        if let Some(handle) = self.pending_detail.as_mut() {
            if handle.is_finished() {
                let handle = self.pending_detail.take().unwrap();
                match handle.await {
                    Ok(Ok(Some(detail))) => {
                        self.set_status(format!("Loaded {}", detail.identifier), false);
                        self.detail = Some(detail);
                    }
                    Ok(Ok(None)) => {
                        self.set_status("Issue detail unavailable", false);
                        self.detail = None;
                    }
                    Ok(Err(err)) => {
                        self.set_status(format!("Error loading detail: {err}"), false);
                        self.detail = None;
                    }
                    Err(err) => {
                        self.set_status(format!("Task error loading detail: {err}"), false);
                        self.detail = None;
                    }
                }
            }
        }
    }

    pub(crate) async fn process_automation(&mut self) {
        if let Some(handle) = self.automation_task.as_mut() {
            if handle.is_finished() {
                let handle = self.automation_task.take().unwrap();
                match handle.await {
                    Ok(outcome) => {
                        if outcome.success {
                            self.set_status(outcome.message, false);
                        } else {
                            self.set_status(outcome.message, false);
                        }
                    }
                    Err(err) => {
                        self.set_status(format!("Automation task error: {err}"), false);
                    }
                }
            }
        }
    }

    fn reset_pagination(&mut self) {
        self.page = 0;
        self.has_next_page = false;
        self.page_cache.clear();
        self.page_cursors.clear();
    }

    pub(crate) async fn load_issues(&mut self) {
        self.abort_pending();
        self.ensure_teams().await;
        self.ensure_states().await;
        self.ensure_project_filters().await;
        self.load_issues_with_filters().await;
    }

    pub(crate) async fn open_projects_overlay(&mut self) {
        self.show_help_overlay = false;
        self.show_cycles_overlay = false;
        self.set_spinner_status("Loading projects…");
        self.ensure_project_filters().await;
        if self.project_filter_options.is_empty() {
            self.projects.clear();
            self.set_status("No projects available (press o or Esc to close)", false);
        } else {
            self.projects = self.project_filter_options.clone();
            self.show_projects_overlay = true;
            self.set_status("Projects loaded (press o or Esc to close)", false);
        }
    }

    pub(crate) fn close_projects_overlay(&mut self) {
        if self.show_projects_overlay {
            self.show_projects_overlay = false;
            self.set_status("Projects overlay closed", false);
        }
    }

    pub(crate) async fn open_cycles_overlay(&mut self) {
        self.show_help_overlay = false;
        self.show_projects_overlay = false;
        let team_id = self.current_team_id();
        self.set_spinner_status("Loading cycles…");
        let request = CycleQueryOptions {
            limit: 10,
            after: None,
            team_id,
            state: None,
            sort: Some(CycleSort::StartDesc),
        };
        match self.cycle_service.list(request).await {
            Ok(response) => {
                self.cycles = response.nodes;
                self.show_cycles_overlay = true;
                self.set_status("Cycles loaded (press y or Esc to close)", false);
            }
            Err(err) => {
                self.cycles.clear();
                self.set_status(format!("Failed to load cycles: {err}"), false);
            }
        }
    }

    pub(crate) fn close_cycles_overlay(&mut self) {
        if self.show_cycles_overlay {
            self.show_cycles_overlay = false;
            self.set_status("Cycles overlay closed", false);
        }
    }

    pub(crate) async fn next_page(&mut self) {
        if !self.has_next_page {
            self.set_status("No more issues", false);
            return;
        }
        if self
            .page_cursors
            .get(self.page)
            .and_then(|c| c.clone())
            .is_none()
            && !self.page_cache.contains_key(&(self.page + 1))
        {
            self.set_status("Next page not yet available", false);
            return;
        }
        self.selected = 0;
        self.issues.clear();
        self.detail = None;
        self.page += 1;
        self.load_issues_with_filters().await;
    }

    pub(crate) async fn previous_page(&mut self) {
        if self.page == 0 {
            self.set_status("Already at first page", false);
            return;
        }
        self.selected = 0;
        self.issues.clear();
        self.detail = None;
        self.page -= 1;
        self.load_issues_with_filters().await;
    }

    pub(crate) async fn go_to_page(&mut self, page: usize) {
        if page == self.page {
            self.set_status(format!("Already on page {}", page + 1), false);
            return;
        }
        if page > 0 && page > self.page_cursors.len() && !self.page_cache.contains_key(&page) {
            self.set_status("Page not available; advance sequentially", false);
            return;
        }
        if page > 0
            && self
                .page_cursors
                .get(page.saturating_sub(1))
                .and_then(|c| c.clone())
                .is_none()
            && !self.page_cache.contains_key(&page)
        {
            self.set_status("Page not available; advance sequentially", false);
            return;
        }
        self.selected = 0;
        self.issues.clear();
        self.detail = None;
        self.page = page;
        self.load_issues_with_filters().await;
    }

    fn current_contains(&self) -> Option<String> {
        self.title_contains.clone()
    }

    async fn load_issues_with_filters(&mut self) {
        self.abort_pending();
        self.ensure_teams().await;
        self.ensure_states().await;
        self.ensure_project_filters().await;
        let contains = self.current_contains();
        let previous_key = self
            .issues
            .get(self.selected)
            .map(|issue| issue.identifier.clone());
        let after = if self.page == 0 {
            None
        } else if let Some(cursor) = self.page_cursors.get(self.page.saturating_sub(1)) {
            cursor.clone()
        } else {
            self.set_status("Page not available; use page next sequentially", false);
            if self.page_cursors.is_empty() {
                self.page = 0;
            } else {
                self.page = self.page_cursors.len() - 1;
            }
            return;
        };

        if let Some(cached) = self.page_cache.get(&self.page).cloned() {
            self.apply_page_data(cached, previous_key.clone(), true);
            return;
        }

        match fetch_issue_summaries(
            &self.service,
            self.current_team_id(),
            self.current_state_id(),
            self.current_project_id(),
            contains,
            after,
            PAGE_SIZE,
        )
        .await
        {
            Ok(result) => {
                let page_data = PageData::from(result);
                self.page_cache.insert(self.page, page_data.clone());
                self.apply_page_data(page_data, previous_key, false);
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
        self.reset_pagination();
        self.load_issues_with_filters().await;
    }

    fn apply_page_data(&mut self, data: PageData, previous_key: Option<String>, from_cache: bool) {
        self.has_next_page = data.has_next_page;
        if self.page_cursors.len() <= self.page {
            self.page_cursors.resize(self.page + 1, None);
        }
        self.page_cursors[self.page] = data.end_cursor.clone();

        if data.issues.is_empty() {
            self.issues.clear();
            self.detail = None;
            self.selected = 0;
            self.set_status(
                format!(
                    "No issues match filters ({} , page: {})",
                    self.filter_context(),
                    self.page + 1
                ),
                false,
            );
            return;
        }

        let mut selected_index = 0;
        if let Some(prev_key) = previous_key {
            if let Some(idx) = data
                .issues
                .iter()
                .position(|issue| issue.identifier.eq_ignore_ascii_case(&prev_key))
            {
                selected_index = idx;
            }
        }

        if selected_index >= data.issues.len() {
            selected_index = 0;
        }

        let selected_identifier = data.issues[selected_index].identifier.clone();
        let detail_matches = self
            .detail
            .as_ref()
            .map(|d| d.identifier.eq_ignore_ascii_case(&selected_identifier))
            .unwrap_or(false);

        self.issues = data.issues;
        self.selected = selected_index;

        if detail_matches {
            self.set_status(
                format!(
                    "Loaded {} issues ({} , page: {})",
                    self.issues.len(),
                    self.filter_context(),
                    self.page + 1
                ),
                false,
            );
        } else {
            self.detail = None;
            let message = if from_cache {
                format!(
                    "Refreshing {} ({} , page: {})",
                    selected_identifier,
                    self.filter_context(),
                    self.page + 1
                )
            } else {
                format!(
                    "Loading {}... ({} , page: {})",
                    selected_identifier,
                    self.filter_context(),
                    self.page + 1
                )
            };
            self.set_spinner_status(message);
            self.queue_detail_fetch(selected_identifier);
        }
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

    fn jump_to_issue(&mut self, key_or_index: &str) -> bool {
        if self.issues.is_empty() {
            return false;
        }
        if let Ok(parsed) = key_or_index.parse::<usize>() {
            if parsed == 0 {
                return false;
            }
            let idx = parsed.saturating_sub(1);
            if idx < self.issues.len() {
                if idx == self.selected {
                    self.set_status(format!("Already focused on #{}", parsed), false);
                } else {
                    self.select_issue(idx);
                }
                return true;
            }
        }

        if let Some(idx) = self
            .issues
            .iter()
            .position(|issue| issue.identifier.eq_ignore_ascii_case(key_or_index))
        {
            if idx == self.selected {
                self.set_status(
                    format!("Already focused on {}", key_or_index.to_uppercase()),
                    false,
                );
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
    pub(crate) async fn clear_all_filters(&mut self) {
        self.team_index = None;
        self.state_index = None;
        self.states_team_id = None;
        self.states.clear();
        self.title_contains = None;
        self.project_filter_index = None;
        self.project_filter_options.clear();
        self.status_tab = StatusTab::All;
        self.set_status("Cleared filters", false);
        self.reset_pagination();
        self.load_issues().await;
    }
    pub(crate) async fn move_issue_selection(&mut self, delta: isize) {
        if self.issues.is_empty() {
            return;
        }
        let len = self.issues.len();
        if delta > 0 && self.selected == len - 1 {
            if self.has_next_page {
                self.next_page().await;
            } else {
                self.set_status("Already at last page", false);
            }
            return;
        }
        if delta < 0 && self.selected == 0 {
            if self.page > 0 {
                self.previous_page().await;
                if !self.issues.is_empty() {
                    let last = self.issues.len().saturating_sub(1);
                    self.abort_pending();
                    self.select_issue(last);
                }
            } else {
                self.set_status("Already at first issue", false);
            }
            return;
        }
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

    pub(crate) async fn move_team_selection(&mut self, delta: isize) {
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
        self.project_filter_index = None;
        self.project_filter_options.clear();
        self.status_tab = StatusTab::All;
        let team_label = self.current_team_label();
        self.set_status(format!("Switched to team: {}", team_label), false);
        self.reset_pagination();
        self.load_issues_with_filters().await;
    }

    fn current_team_id(&self) -> Option<String> {
        self.team_index
            .and_then(|idx| self.teams.get(idx))
            .map(|team| team.id.clone())
    }

    pub(crate) fn current_team_label(&self) -> String {
        self.team_index
            .and_then(|idx| self.teams.get(idx))
            .map(|team| team.key.clone())
            .unwrap_or_else(|| "All".into())
    }

    pub(crate) fn current_state_label(&self) -> String {
        self.state_index
            .and_then(|idx| self.states.get(idx))
            .map(|state| state.name.clone())
            .unwrap_or_else(|| "All".into())
    }

    pub(crate) fn current_project_label(&self) -> String {
        self.project_filter_index
            .and_then(|idx| self.project_filter_options.get(idx))
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "All".into())
    }

    fn current_project_id(&self) -> Option<String> {
        self.project_filter_index
            .and_then(|idx| self.project_filter_options.get(idx))
            .map(|project| project.id.clone())
    }

    async fn ensure_states(&mut self) {
        if let Some(team_id) = self.current_team_id() {
            if self.states_team_id.as_deref() != Some(&team_id) {
                match self.service.workflow_states(&team_id).await {
                    Ok(states) => {
                        self.states = states;
                        self.states_team_id = Some(team_id);
                        self.state_index = None;
                        self.apply_current_status_tab();
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
            self.status_tab = StatusTab::All;
        }
        if let Some(idx) = self.state_index {
            if idx >= self.states.len() {
                self.state_index = None;
            }
        }
    }

    async fn ensure_project_filters(&mut self) {
        let cache_key = self.current_team_id();
        if let Some(options) = self.project_filter_cache.get(&cache_key) {
            self.project_filter_options = options.clone();
            self.normalize_project_index();
            return;
        }

        let request = ProjectQueryOptions {
            limit: 50,
            after: None,
            state: None,
            status: None,
            team_id: cache_key.clone(),
            sort: Some(ProjectSort::UpdatedDesc),
        };

        match self.project_service.list(request).await {
            Ok(response) => {
                let mut projects = response.nodes;
                projects.sort_by(|a, b| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                });
                self.project_filter_cache
                    .insert(cache_key, projects.clone());
                self.project_filter_options = projects;
                self.normalize_project_index();
            }
            Err(err) => {
                self.project_filter_options.clear();
                self.project_filter_index = None;
                self.set_status(format!("Failed to load projects: {err}"), false);
            }
        }
    }

    fn normalize_project_index(&mut self) {
        if let Some(idx) = self.project_filter_index {
            if idx >= self.project_filter_options.len() {
                self.project_filter_index = None;
            }
        }
    }

    fn find_state_by_types(&self, types: &[&str]) -> Option<usize> {
        self.states.iter().enumerate().find_map(|(idx, state)| {
            let ty = state.type_name.as_deref()?.to_ascii_lowercase();
            if types.iter().any(|candidate| ty == *candidate) {
                Some(idx)
            } else {
                None
            }
        })
    }

    fn apply_current_status_tab(&mut self) {
        match self.status_tab {
            StatusTab::All => {
                self.state_index = None;
            }
            StatusTab::Todo => {
                self.state_index = self.find_state_by_types(&["backlog", "unstarted"]);
            }
            StatusTab::Doing => {
                self.state_index = self.find_state_by_types(&["started", "inprogress"]);
            }
            StatusTab::Done => {
                self.state_index = self.find_state_by_types(&["completed", "done"]);
            }
        }
    }

    fn infer_status_tab_from_state(&self) -> StatusTab {
        if let Some(idx) = self.state_index {
            if let Some(state) = self.states.get(idx) {
                if let Some(ty) = state.type_name.as_deref() {
                    let ty = ty.to_ascii_lowercase();
                    if matches!(ty.as_str(), "backlog" | "unstarted") {
                        return StatusTab::Todo;
                    }
                    if matches!(ty.as_str(), "started" | "inprogress") {
                        return StatusTab::Doing;
                    }
                    if matches!(ty.as_str(), "completed" | "done") {
                        return StatusTab::Done;
                    }
                }
            }
        }
        StatusTab::All
    }

    pub(crate) async fn move_state_selection(&mut self, delta: isize) {
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
        self.status_tab = self.infer_status_tab_from_state();
        self.set_status(format!("State filter: {}", state_label), false);
        self.reset_pagination();
        self.load_issues_with_filters().await;
    }

    pub(crate) async fn set_status_tab(&mut self, tab: StatusTab) {
        if self.status_tab == tab {
            self.set_status(format!("Status tab already {}", tab.label()), false);
            return;
        }
        self.status_tab = tab;
        self.ensure_states().await;
        self.apply_current_status_tab();
        self.reset_pagination();
        let label = tab.label();
        self.set_spinner_status(format!("Applying status tab: {}", label));
        self.load_issues_with_filters().await;
    }

    pub(crate) async fn cycle_status_tab(&mut self, delta: isize) {
        let tabs = StatusTab::all();
        if tabs.is_empty() {
            return;
        }
        let total = tabs.len() as isize;
        let current = tabs.iter().position(|t| *t == self.status_tab).unwrap_or(0) as isize;
        let mut next = current + delta;
        if total > 0 {
            next = (next % total + total) % total;
            let tab = tabs[next as usize];
            self.set_status_tab(tab).await;
        }
    }

    pub(crate) async fn cycle_project_filter(&mut self, delta: isize) {
        self.ensure_project_filters().await;
        let total = self.project_filter_options.len() as isize + 1;
        if total <= 1 {
            self.project_filter_index = None;
            self.set_status("No projects available for current team", false);
            self.reset_pagination();
            self.load_issues_with_filters().await;
            return;
        }
        let current = self
            .project_filter_index
            .map(|idx| idx as isize + 1)
            .unwrap_or(0);
        let mut next = current + delta;
        next = (next % total + total) % total;
        self.project_filter_index = if next == 0 {
            None
        } else {
            Some((next - 1) as usize)
        };
        let label = self.current_project_label();
        self.set_spinner_status(format!("Project filter: {}", label));
        self.reset_pagination();
        self.load_issues_with_filters().await;
    }

    pub(crate) async fn clear_project_filter(&mut self) {
        if self.project_filter_index.is_none() {
            self.set_status("Project filter already cleared", false);
            return;
        }
        self.project_filter_index = None;
        self.set_spinner_status("Project filter cleared");
        self.reset_pagination();
        self.load_issues_with_filters().await;
    }

    pub(crate) async fn set_project_filter_by_name(&mut self, name: &str) {
        if name.eq_ignore_ascii_case("clear") {
            self.clear_project_filter().await;
            return;
        }
        self.ensure_project_filters().await;
        if let Some(idx) = self
            .project_filter_options
            .iter()
            .position(|project| project.name.eq_ignore_ascii_case(name))
        {
            self.project_filter_index = Some(idx);
            let label = self.current_project_label();
            self.set_spinner_status(format!("Project filter: {}", label));
            self.reset_pagination();
            self.load_issues_with_filters().await;
        } else {
            self.set_status(format!("Project '{}' not found", name), false);
        }
    }

    pub(crate) fn project_filter_index(&self) -> Option<usize> {
        self.project_filter_index
    }

    pub(crate) fn trigger_cli_action(&mut self) {
        if self.automation_task.is_some() {
            self.set_status("Automation already running", false);
            return;
        }
        let Some(issue) = self.selected_issue() else {
            self.set_status("Select an issue before triggering automation", false);
            return;
        };
        let issue_key = issue.identifier.clone();
        let profile = self.profile.clone();
        self.set_spinner_status(format!("Running CLI: issue view {}", issue_key));
        self.automation_task = Some(tokio::spawn(async move {
            let binary = match env::current_exe() {
                Ok(path) => path,
                Err(err) => {
                    return AutomationOutcome {
                        success: false,
                        message: format!("CLI error resolving binary: {err}"),
                    };
                }
            };

            let mut command = Command::new(binary);
            command
                .arg("issue")
                .arg("view")
                .arg(&issue_key)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            if !profile.is_empty() {
                command.arg("--profile").arg(&profile);
            }

            command.arg("--json");

            match command.output().await {
                Ok(output) => {
                    if output.status.success() {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let mut first_line =
                            stdout.lines().next().unwrap_or("ok").trim().to_string();
                        if first_line.is_empty() {
                            first_line = "ok".into();
                        }
                        if first_line.len() > 80 {
                            first_line.truncate(77);
                            first_line.push_str("…");
                        }
                        AutomationOutcome {
                            success: true,
                            message: format!(
                                "CLI issue view {} succeeded: {}",
                                issue_key, first_line
                            ),
                        }
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let mut first_line = stderr
                            .lines()
                            .next()
                            .unwrap_or("unknown error")
                            .trim()
                            .to_string();
                        if first_line.is_empty() {
                            first_line = "unknown error".into();
                        }
                        AutomationOutcome {
                            success: false,
                            message: format!("CLI issue view {} failed: {}", issue_key, first_line),
                        }
                    }
                }
                Err(err) => AutomationOutcome {
                    success: false,
                    message: format!("CLI execution error: {err}"),
                },
            }
        }));
    }

    fn current_state_id(&self) -> Option<String> {
        self.state_index
            .and_then(|idx| self.states.get(idx))
            .map(|state| state.id.clone())
    }

    pub(crate) fn palette_suggestions(&self) -> Vec<Line<'static>> {
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
        } else if let Some(rest) = input.strip_prefix("project ") {
            let term = rest.trim();
            let mut lines = Vec::new();
            if term.is_empty() {
                lines.push(Line::from("project next"));
                lines.push(Line::from("project prev"));
                lines.push(Line::from("project clear"));
                for project in self.project_filter_options.iter().take(3) {
                    lines.push(Line::from(format!("project {}", project.name)));
                }
            } else {
                for project in self
                    .project_filter_options
                    .iter()
                    .filter(|project| project.name.to_ascii_lowercase().contains(term))
                    .take(5)
                {
                    lines.push(Line::from(format!("project {}", project.name)));
                }
                if !matches!(term, "next" | "prev" | "clear") {
                    if lines.is_empty() {
                        lines.push(Line::from("project <name>"));
                    }
                    lines.push(Line::from("project next"));
                    lines.push(Line::from("project prev"));
                }
                if term != "clear" {
                    lines.push(Line::from("project clear"));
                }
            }
            lines
        } else if let Some(rest) = input.strip_prefix("status ") {
            let term = rest.trim();
            let mut lines = Vec::new();
            for tab in StatusTab::all() {
                let label = tab.label().to_ascii_lowercase();
                if term.is_empty() || label.starts_with(term) {
                    lines.push(Line::from(format!("status {}", label)));
                }
            }
            if term.is_empty() || matches!(term, "next" | "prev") {
                lines.push(Line::from("status next"));
                lines.push(Line::from("status prev"));
            }
            lines
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
            let mut lines = Vec::new();
            if term.is_empty() {
                for (idx, issue) in self.issues.iter().enumerate().take(5) {
                    lines.push(Line::from(format!("view {}", issue.identifier)));
                    lines.push(Line::from(format!("view {}", idx + 1)));
                }
            } else {
                let needle = term.to_ascii_lowercase();
                for (idx, issue) in self
                    .issues
                    .iter()
                    .enumerate()
                    .filter(|(_, issue)| {
                        issue.identifier.to_ascii_lowercase().starts_with(&needle)
                            || issue.title.to_ascii_lowercase().contains(&needle)
                    })
                    .take(5)
                {
                    lines.push(Line::from(format!("view {}", issue.identifier)));
                    lines.push(Line::from(format!("view {}", idx + 1)));
                }
            }
            if lines.is_empty() {
                lines.push(Line::from("view <issue-key>"));
            }
            lines.push(Line::from("view next"));
            lines.push(Line::from("view prev"));
            lines.push(Line::from("view first"));
            lines.push(Line::from("view last"));
            lines
        } else if let Some(rest) = input.strip_prefix("page ") {
            let term = rest.trim();
            let mut lines = Vec::new();
            if term.is_empty() {
                lines.push(Line::from("page next"));
                lines.push(Line::from("page prev"));
                lines.push(Line::from("page refresh"));
                lines.push(Line::from(format!("page {}", self.page + 1)));
            } else if term.eq_ignore_ascii_case("refresh") {
                lines.push(Line::from("page refresh"));
            } else if term.chars().all(|c| c.is_ascii_digit()) {
                lines.push(Line::from(format!("page {}", term)));
            } else {
                lines.push(Line::from("page <number>"));
            }
            lines
        } else {
            vec![
                Line::from("team <key>"),
                Line::from("state <name>"),
                Line::from("project <name>"),
                Line::from("project next"),
                Line::from("project prev"),
                Line::from("project clear"),
                Line::from("contains <text>"),
                Line::from("contains clear"),
                Line::from("view <issue-key>"),
                Line::from("view next"),
                Line::from("view prev"),
                Line::from("view first"),
                Line::from("view last"),
                Line::from("status todo"),
                Line::from("status doing"),
                Line::from("status done"),
                Line::from("status all"),
                Line::from("status next"),
                Line::from("status prev"),
                Line::from("page next"),
                Line::from("page prev"),
                Line::from("page refresh"),
                Line::from("page <number>"),
                Line::from("clear"),
                Line::from("reload"),
                Line::from("help"),
            ]
        }
    }

    pub(crate) fn recall_palette_history(&mut self, delta: isize) {
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

    pub(crate) fn toggle_focus(&mut self) {
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

    pub(crate) fn enter_palette(&mut self) {
        self.palette_active = true;
        self.show_help_overlay = false;
        self.palette_input.clear();
        self.palette_history_index = None;
        self.set_status("Command mode (: to exit, ↑/↓ history)", false);
    }

    pub(crate) fn enter_contains_palette(&mut self) {
        self.palette_active = true;
        self.show_help_overlay = false;
        self.palette_history_index = None;
        self.palette_input = match self.current_contains() {
            Some(term) => format!("contains {}", term),
            None => "contains ".into(),
        };
        self.set_status("Contains filter (Esc to cancel, Enter to apply)", false);
    }

    pub(crate) async fn execute_command(&mut self, command: String) {
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
                self.reset_pagination();
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
                    self.reset_pagination();
                    self.load_issues_with_filters().await;
                }
            }
            return;
        }

        if let Some(project_arg) = cmd.strip_prefix("project ") {
            let project_arg = project_arg.trim();
            if project_arg.eq_ignore_ascii_case("next") {
                self.cycle_project_filter(1).await;
            } else if project_arg.eq_ignore_ascii_case("prev")
                || project_arg.eq_ignore_ascii_case("previous")
            {
                self.cycle_project_filter(-1).await;
            } else if project_arg.eq_ignore_ascii_case("clear") {
                self.clear_project_filter().await;
            } else if project_arg.is_empty() {
                self.set_status("Usage: project <name|next|prev|clear>", false);
            } else {
                self.set_project_filter_by_name(project_arg).await;
            }
            return;
        }

        if let Some(status_arg) = cmd.strip_prefix("status ") {
            let status_arg = status_arg.trim();
            if status_arg.eq_ignore_ascii_case("next") {
                self.cycle_status_tab(1).await;
            } else if status_arg.eq_ignore_ascii_case("prev")
                || status_arg.eq_ignore_ascii_case("previous")
            {
                self.cycle_status_tab(-1).await;
            } else if status_arg.eq_ignore_ascii_case("todo") {
                self.set_status_tab(StatusTab::Todo).await;
            } else if status_arg.eq_ignore_ascii_case("doing")
                || status_arg.eq_ignore_ascii_case("inprogress")
            {
                self.set_status_tab(StatusTab::Doing).await;
            } else if status_arg.eq_ignore_ascii_case("done")
                || status_arg.eq_ignore_ascii_case("completed")
            {
                self.set_status_tab(StatusTab::Done).await;
            } else if status_arg.eq_ignore_ascii_case("all") {
                self.set_status_tab(StatusTab::All).await;
            } else {
                self.set_status("Usage: status <todo|doing|done|all|next|prev>", false);
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
            "project" => {
                self.set_status("Usage: project <name|next|prev|clear>", false);
                return;
            }
            "status" => {
                self.set_status("Usage: status <todo|doing|done|all|next|prev>", false);
                return;
            }
            _ => {}
        }

        if let Some(key) = cmd.strip_prefix("view ") {
            let key = key.trim();
            if key.is_empty() {
                self.set_status("Usage: view <issue-key>", false);
            } else if self.jump_to_issue(key) {
                if let Some(current) = self.issues.get(self.selected) {
                    self.set_status(
                        format!("Jumped to {}", current.identifier.to_uppercase()),
                        false,
                    );
                } else {
                    self.set_status("Jumped", false);
                }
            } else {
                self.set_status(format!("Issue '{}' not in the current list", key), false);
            }
            return;
        }

        if matches!(cmd, "help" | "?") {
            self.open_help_overlay();
            return;
        }

        if let Some(arg) = cmd.strip_prefix("page ") {
            let arg = arg.trim();
            if arg.eq_ignore_ascii_case("next") {
                self.next_page().await;
                return;
            } else if arg.eq_ignore_ascii_case("prev") || arg.eq_ignore_ascii_case("previous") {
                self.previous_page().await;
                return;
            } else if arg.eq_ignore_ascii_case("refresh") {
                self.page_cache.remove(&self.page);
                if self.page > 0 && self.page_cursors.len() >= self.page {
                    // keep cursor history so forward navigation still works
                    self.page_cursors.resize(self.page, None);
                }
                self.selected = 0;
                self.issues.clear();
                self.detail = None;
                self.set_status(format!("Refreshing page {}…", self.page + 1), true);
                self.load_issues_with_filters().await;
                return;
            } else if let Ok(num) = arg.parse::<usize>() {
                if num == 0 {
                    self.set_status("Pages start at 1", false);
                } else {
                    let target = num - 1;
                    if target == self.page {
                        self.set_status(format!("Already on page {}", num), false);
                    } else {
                        self.go_to_page(target).await;
                    }
                }
                return;
            } else {
                self.set_status("Usage: page <next|prev|number>", false);
                return;
            }
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
                self.reset_pagination();
                self.load_issues().await;
            }
            "contains" => self.set_status("Usage: contains <term>", false),
            _ => self.set_status(format!("Unknown command: {}", cmd), false),
        }
    }

    pub(crate) fn toggle_help_overlay(&mut self) {
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
    project_id: Option<String>,
    contains: Option<String>,
    after: Option<String>,
    limit: usize,
) -> Result<IssueListResult> {
    service
        .list(IssueQueryOptions {
            limit,
            team_id,
            state_id,
            project_id,
            title_contains: contains,
            after,
            ..Default::default()
        })
        .await
        .context("failed to fetch issues")
}

async fn fetch_issue_detail(
    service: IssueService,
    key: String,
) -> Result<Option<linear_core::graphql::IssueDetail>> {
    Ok(service.get_by_key(&key).await.ok())
}

#[derive(Clone)]
struct PageData {
    issues: Vec<linear_core::graphql::IssueSummary>,
    end_cursor: Option<String>,
    has_next_page: bool,
}

struct AutomationOutcome {
    success: bool,
    message: String,
}

impl From<IssueListResult> for PageData {
    fn from(result: IssueListResult) -> Self {
        Self {
            issues: result.issues,
            end_cursor: result.end_cursor,
            has_next_page: result.has_next_page,
        }
    }
}
