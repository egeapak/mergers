use super::{DataLoadingState, VersionInputState};
use crate::{
    core::operations::DependencyCategory,
    models::WorkItemHistory,
    ui::apps::MergeApp,
    ui::state::default::MergeState,
    ui::state::typed::{ModeState, StateChange},
    utils::html_to_lines,
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use chrono::DateTime;
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, List, ListItem, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState,
    },
};
use std::collections::HashSet;
use std::time::Instant;

#[derive(Debug, Clone)]
enum SearchQuery {
    PullRequestTitle(String),
    WorkItemTitle(String),
    PullRequestId(i32),
    WorkItemId(i32),
}

pub struct PullRequestSelectionState {
    table_state: TableState,
    scrollbar_state: ScrollbarState,
    work_item_index: usize,
    multi_select_mode: bool,
    available_states: Vec<String>,
    selected_filter_states: HashSet<String>,
    state_selection_index: usize,
    // Search functionality
    search_mode: bool,
    search_input: String,
    search_results: Vec<usize>,
    current_search_index: usize,
    search_error_message: Option<String>,
    search_iteration_mode: bool,
    last_search_query: String, // Store the last executed search query
    // Mouse support
    last_click_time: Option<Instant>,
    last_click_row: Option<usize>,
    table_area: Option<Rect>,
    // Dependency dialog
    show_dependency_dialog: bool,
    dependency_dialog_pr_index: Option<usize>,
    dependency_dialog_scroll: usize,
    // Details pane toggle
    show_details: bool,
}

impl Default for PullRequestSelectionState {
    fn default() -> Self {
        Self::new()
    }
}

impl PullRequestSelectionState {
    pub fn new() -> Self {
        Self {
            table_state: TableState::default(),
            scrollbar_state: ScrollbarState::default(),
            work_item_index: 0,
            multi_select_mode: false,
            available_states: Vec::new(),
            selected_filter_states: HashSet::new(),
            state_selection_index: 0,
            // Search functionality
            search_mode: false,
            search_input: String::new(),
            search_results: Vec::new(),
            current_search_index: 0,
            search_error_message: None,
            search_iteration_mode: false,
            last_search_query: String::new(),
            // Mouse support
            last_click_time: None,
            last_click_row: None,
            // Dependency dialog
            show_dependency_dialog: false,
            dependency_dialog_pr_index: None,
            dependency_dialog_scroll: 0,
            table_area: None,
            // Details pane toggle
            show_details: true,
        }
    }

    fn update_scrollbar_state(&mut self, total_items: usize) {
        self.scrollbar_state = self
            .scrollbar_state
            .content_length(total_items)
            .position(self.table_state.selected().unwrap_or(0));
    }

    fn parse_search_query(input: &str) -> Result<SearchQuery> {
        let trimmed = input.trim();

        if trimmed.is_empty() {
            bail!("Search query cannot be empty");
        }

        // Handle shortcuts: !12345 for PR and #98765 for work item
        if let Some(pr_id_str) = trimmed.strip_prefix('!') {
            if let Ok(pr_id) = pr_id_str.parse::<i32>() {
                return Ok(SearchQuery::PullRequestId(pr_id));
            }
            bail!("Invalid PR ID format");
        }

        if let Some(wi_id_str) = trimmed.strip_prefix('#') {
            if let Ok(wi_id) = wi_id_str.parse::<i32>() {
                return Ok(SearchQuery::WorkItemId(wi_id));
            }
            bail!("Invalid work item ID format");
        }

        // Handle tag:query format
        if let Some(colon_pos) = trimmed.find(':') {
            let tag = trimmed[..colon_pos].to_uppercase();
            let query = trimmed[colon_pos + 1..].trim();

            if query.is_empty() {
                bail!("Query after ':' cannot be empty");
            }

            match tag.as_str() {
                "P" | "PR" => Ok(SearchQuery::PullRequestTitle(query.to_string())),
                "W" | "WI" => Ok(SearchQuery::WorkItemTitle(query.to_string())),
                _ => {
                    bail!("Invalid tag. Use 'P'/'PR' for pull requests or 'W'/'WI' for work items")
                }
            }
        } else {
            // Default to PR title search if no tag is specified
            Ok(SearchQuery::PullRequestTitle(trimmed.to_string()))
        }
    }

    fn execute_search(&mut self, app: &MergeApp) {
        self.search_results.clear();
        self.current_search_index = 0;
        self.search_error_message = None;
        self.search_iteration_mode = false;

        // Store the search query for display in status bar
        self.last_search_query = self.search_input.clone();

        let query = match Self::parse_search_query(&self.search_input) {
            Ok(q) => q,
            Err(e) => {
                self.search_error_message = Some(e.to_string());
                return;
            }
        };

        match query {
            SearchQuery::PullRequestId(pr_id) => {
                for (idx, pr_with_wi) in app.pull_requests().iter().enumerate() {
                    if pr_with_wi.pr.id == pr_id {
                        self.search_results.push(idx);
                        break; // Only one PR with this ID should exist
                    }
                }
            }
            SearchQuery::WorkItemId(wi_id) => {
                for (idx, pr_with_wi) in app.pull_requests().iter().enumerate() {
                    if pr_with_wi.work_items.iter().any(|wi| wi.id == wi_id) {
                        self.search_results.push(idx);
                    }
                }
            }
            SearchQuery::PullRequestTitle(search_term) => {
                let search_term_lower = search_term.to_lowercase();
                for (idx, pr_with_wi) in app.pull_requests().iter().enumerate() {
                    if pr_with_wi
                        .pr
                        .title
                        .to_lowercase()
                        .contains(&search_term_lower)
                    {
                        self.search_results.push(idx);
                    }
                }
            }
            SearchQuery::WorkItemTitle(search_term) => {
                let search_term_lower = search_term.to_lowercase();
                for (idx, pr_with_wi) in app.pull_requests().iter().enumerate() {
                    let has_matching_work_item = pr_with_wi.work_items.iter().any(|wi| {
                        wi.fields
                            .title
                            .as_ref()
                            .map(|title| title.to_lowercase().contains(&search_term_lower))
                            .unwrap_or(false)
                    });
                    if has_matching_work_item {
                        self.search_results.push(idx);
                    }
                }
            }
        }

        if self.search_results.is_empty() {
            self.search_error_message = Some("No matching items found".to_string());
        } else {
            // Jump to first result and enter search iteration mode
            self.search_iteration_mode = true;
            self.current_search_index = 0;
            self.table_state.select(Some(self.search_results[0]));
            self.work_item_index = 0; // Reset work item selection
        }
    }

    fn navigate_search_results(&mut self, direction: i32) {
        if self.search_results.is_empty() || !self.search_iteration_mode {
            return;
        }

        // Find the current selection in the search results
        let current_table_selection = self.table_state.selected().unwrap_or(0);
        let current_search_pos = self
            .search_results
            .iter()
            .position(|&idx| idx == current_table_selection);

        let new_search_pos = if let Some(pos) = current_search_pos {
            // We're currently on a search result, navigate from here
            if direction > 0 {
                if pos + 1 < self.search_results.len() {
                    pos + 1
                } else {
                    self.search_error_message = Some("No more results".to_string());
                    return;
                }
            } else if pos > 0 {
                pos - 1
            } else {
                self.search_error_message = Some("No previous results".to_string());
                return;
            }
        } else {
            // We're not currently on a search result, find the nearest one
            if direction > 0 {
                // Find the first search result after the current selection
                match self
                    .search_results
                    .iter()
                    .position(|&idx| idx > current_table_selection)
                {
                    Some(pos) => pos,
                    None => {
                        self.search_error_message = Some("No more results".to_string());
                        return;
                    }
                }
            } else {
                // Find the last search result before the current selection
                match self
                    .search_results
                    .iter()
                    .rposition(|&idx| idx < current_table_selection)
                {
                    Some(pos) => pos,
                    None => {
                        self.search_error_message = Some("No previous results".to_string());
                        return;
                    }
                }
            }
        };

        // Update both the current search index and table selection
        self.current_search_index = new_search_pos;
        self.table_state
            .select(Some(self.search_results[new_search_pos]));
        self.work_item_index = 0; // Reset work item selection
        self.search_error_message = None; // Clear any previous error messages
    }

    fn enter_search_mode(&mut self) {
        self.search_mode = true;
        self.search_input.clear();
        self.search_results.clear();
        self.search_error_message = None;
        self.search_iteration_mode = false;
    }

    fn exit_search_mode(&mut self) {
        self.search_mode = false;
        self.search_iteration_mode = false;
        self.search_results.clear();
        self.search_error_message = None;
    }

    fn initialize_selection(&mut self, app: &MergeApp) {
        if !app.pull_requests().is_empty() && self.table_state.selected().is_none() {
            self.table_state.select(Some(0));
        }
        self.update_scrollbar_state(app.pull_requests().len());
    }

    fn next(&mut self, app: &MergeApp) {
        if app.pull_requests().is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= app.pull_requests().len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
        self.work_item_index = 0; // Reset work item selection when PR changes
        self.update_scrollbar_state(app.pull_requests().len());
    }

    fn previous(&mut self, app: &MergeApp) {
        if app.pull_requests().is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    app.pull_requests().len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
        self.work_item_index = 0; // Reset work item selection when PR changes
        self.update_scrollbar_state(app.pull_requests().len());
    }

    fn toggle_selection(&mut self, app: &mut MergeApp) {
        if let Some(i) = self.table_state.selected()
            && let Some(pr) = app.pull_requests_mut().get_mut(i)
        {
            pr.selected = !pr.selected;
        }
    }

    fn next_work_item(&mut self, app: &MergeApp) {
        if let Some(pr_index) = self.table_state.selected()
            && let Some(pr) = app.pull_requests().get(pr_index)
            && !pr.work_items.is_empty()
        {
            self.work_item_index = (self.work_item_index + 1) % pr.work_items.len();
        }
    }

    fn previous_work_item(&mut self, app: &MergeApp) {
        if let Some(pr_index) = self.table_state.selected()
            && let Some(pr) = app.pull_requests().get(pr_index)
            && !pr.work_items.is_empty()
        {
            if self.work_item_index == 0 {
                self.work_item_index = pr.work_items.len() - 1;
            } else {
                self.work_item_index -= 1;
            }
        }
    }

    fn collect_distinct_work_item_states(&self, app: &MergeApp) -> Vec<String> {
        let mut states = HashSet::new();

        for pr in app.pull_requests() {
            for work_item in &pr.work_items {
                if let Some(state) = &work_item.fields.state {
                    states.insert(state.clone());
                }
            }
        }

        let mut sorted_states: Vec<String> = states.into_iter().collect();
        sorted_states.sort();
        sorted_states
    }

    fn select_all_with_filter_states(&self, app: &mut MergeApp) {
        if self.selected_filter_states.is_empty() {
            return;
        }

        for pr in app.pull_requests_mut() {
            if pr.work_items.is_empty() {
                continue;
            }

            let all_work_items_match = pr.work_items.iter().all(|work_item| {
                if let Some(state) = &work_item.fields.state {
                    self.selected_filter_states.contains(state)
                } else {
                    false
                }
            });

            pr.selected = all_work_items_match;
        }
    }

    fn clear_all_selections(&self, app: &mut MergeApp) {
        for pr in app.pull_requests_mut() {
            pr.selected = false;
        }
    }

    fn enter_multi_select_mode(&mut self, app: &MergeApp) {
        self.multi_select_mode = true;
        self.available_states = self.collect_distinct_work_item_states(app);
        self.selected_filter_states.clear();
        self.state_selection_index = 0;
    }

    fn exit_multi_select_mode(&mut self) {
        self.multi_select_mode = false;
        self.available_states.clear();
        self.selected_filter_states.clear();
        self.state_selection_index = 0;
    }

    fn toggle_state_in_filter(&mut self) {
        if let Some(state) = self.available_states.get(self.state_selection_index) {
            if self.selected_filter_states.contains(state) {
                self.selected_filter_states.remove(state);
            } else {
                self.selected_filter_states.insert(state.clone());
            }
        }
    }

    fn next_state(&mut self) {
        if !self.available_states.is_empty() {
            self.state_selection_index =
                (self.state_selection_index + 1) % self.available_states.len();
        }
    }

    fn previous_state(&mut self) {
        if !self.available_states.is_empty() {
            if self.state_selection_index == 0 {
                self.state_selection_index = self.available_states.len() - 1;
            } else {
                self.state_selection_index -= 1;
            }
        }
    }

    fn render_work_item_details(&self, f: &mut Frame, app: &MergeApp, area: ratatui::layout::Rect) {
        if let Some(pr_index) = self.table_state.selected() {
            if let Some(pr) = app.pull_requests().get(pr_index) {
                if pr.work_items.is_empty() {
                    let no_items =
                        Paragraph::new("No work items associated with this pull request.")
                            .style(Style::default().fg(Color::Gray))
                            .block(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .title("Work Item Details"),
                            )
                            .alignment(Alignment::Center);
                    f.render_widget(no_items, area);
                    return;
                }

                // Ensure work_item_index is within bounds
                let work_item_index = if self.work_item_index < pr.work_items.len() {
                    self.work_item_index
                } else {
                    0
                };

                if let Some(work_item) = pr.work_items.get(work_item_index) {
                    // Create layout for header, history, and content
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(4), // Header (2 lines + borders)
                            Constraint::Length(3), // History (1 line + borders)
                            Constraint::Min(0),    // Description content
                        ])
                        .split(area);

                    // Render header
                    let state = work_item.fields.state.as_deref().unwrap_or("Unknown");
                    let work_item_type = work_item
                        .fields
                        .work_item_type
                        .as_deref()
                        .unwrap_or("Unknown");
                    let assigned_to = work_item
                        .fields
                        .assigned_to
                        .as_ref()
                        .map(|user| user.display_name.as_str())
                        .unwrap_or("Unassigned");
                    let iteration_path = work_item
                        .fields
                        .iteration_path
                        .as_deref()
                        .unwrap_or("Unknown");
                    let title = work_item.fields.title.as_deref().unwrap_or("No title");

                    // Get colors for type and state
                    let type_color = match work_item_type.to_lowercase().as_str() {
                        "task" => Color::Yellow,
                        "bug" => Color::Red,
                        "user story" => Color::Blue,
                        "feature" => Color::Green,
                        _ => Color::White,
                    };

                    let state_color = get_state_color(state);

                    // Create header content with spans for different colors and proper alignment
                    use ratatui::text::{Line, Span};
                    let header_lines = vec![
                        Line::from(vec![
                            Span::styled(
                                format!("{:<11}", work_item_type), // Fixed width for type
                                Style::default().fg(type_color).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!(" #{:<6} ", work_item.id), // Fixed width for ID
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                title,
                                Style::default()
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]),
                        Line::from(vec![
                            Span::styled(
                                "●",
                                Style::default()
                                    .fg(state_color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!(" {:<15}", state), // Fixed width for state
                                Style::default().fg(state_color),
                            ),
                            Span::styled(
                                format!(" | Iteration: {}", iteration_path),
                                Style::default().fg(Color::Gray),
                            ),
                            Span::styled(
                                format!(" | Assigned: {}", assigned_to),
                                Style::default().fg(Color::Yellow),
                            ),
                        ]),
                    ];

                    let header_widget = Paragraph::new(header_lines).block(
                        Block::default().borders(Borders::ALL).title(format!(
                            "Work Item ({}/{})",
                            work_item_index + 1,
                            pr.work_items.len()
                        )),
                    );

                    f.render_widget(header_widget, chunks[0]);

                    // Render history section
                    self.render_work_item_history_linear(f, chunks[1], work_item);

                    // Render description - use repro steps for bugs, description for others
                    let (description_content, description_title) = match work_item_type
                        .to_lowercase()
                        .as_str()
                    {
                        "bug" => {
                            let content = if let Some(repro_steps) = &work_item.fields.repro_steps {
                                if !repro_steps.is_empty() {
                                    repro_steps.clone()
                                } else if let Some(description) = &work_item.fields.description {
                                    if !description.is_empty() {
                                        description.clone()
                                    } else {
                                        "No reproduction steps available.".to_string()
                                    }
                                } else {
                                    "No reproduction steps available.".to_string()
                                }
                            } else if let Some(description) = &work_item.fields.description {
                                if !description.is_empty() {
                                    description.clone()
                                } else {
                                    "No reproduction steps available.".to_string()
                                }
                            } else {
                                "No reproduction steps available.".to_string()
                            };
                            (
                                content,
                                "Reproduction Steps (use ←/→ to navigate work items)",
                            )
                        }
                        _ => {
                            let content = if let Some(description) = &work_item.fields.description {
                                if !description.is_empty() {
                                    description.clone()
                                } else {
                                    "No description available.".to_string()
                                }
                            } else {
                                "No description available.".to_string()
                            };
                            (content, "Description (use ←/→ to navigate work items)")
                        }
                    };

                    // Convert HTML content to ratatui spans
                    let description_lines = html_to_lines(&description_content);

                    let description_widget = Paragraph::new(description_lines)
                        .style(Style::default().fg(Color::White))
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title(description_title),
                        )
                        .wrap(ratatui::widgets::Wrap { trim: true });

                    f.render_widget(description_widget, chunks[2]);
                }
            }
        } else {
            let no_selection = Paragraph::new("No pull request selected.")
                .style(Style::default().fg(Color::Gray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Work Item Details"),
                )
                .alignment(Alignment::Center);
            f.render_widget(no_selection, area);
        }
    }

    fn render_work_item_history_linear(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        work_item: &crate::models::WorkItem,
    ) {
        use ratatui::text::{Line, Span};

        let mut history_spans = vec![];

        if work_item.history.is_empty() {
            history_spans.push(Span::styled(
                "No history available",
                Style::default().fg(Color::Gray),
            ));
        } else {
            // Sort history by date (most recent first) and filter to only state changes
            let mut state_changes: Vec<_> = work_item
                .history
                .iter()
                .filter(|h| {
                    h.fields
                        .as_ref()
                        .and_then(|f| f.state.as_ref())
                        .and_then(|s| s.new_value.as_ref())
                        .is_some()
                })
                .cloned()
                .collect();

            // Sort by date from earliest to latest (left to right chronologically)
            // Use System.ChangedDate as primary date source, fall back to revisedDate
            state_changes.sort_by(|a, b| {
                let get_date_string = |entry: &WorkItemHistory| -> Option<String> {
                    // First try System.ChangedDate
                    if let Some(fields) = &entry.fields
                        && let Some(changed_date) = &fields.changed_date
                        && let Some(new_date) = &changed_date.new_value
                        && !new_date.starts_with("9999-01-01")
                    {
                        return Some(new_date.clone());
                    }
                    // Fall back to revisedDate if not a placeholder
                    if !entry.revised_date.starts_with("9999-01-01") {
                        Some(entry.revised_date.clone())
                    } else {
                        None // No valid date found
                    }
                };

                let a_date = get_date_string(a);
                let b_date = get_date_string(b);

                match (a_date, b_date) {
                    (Some(a_d), Some(b_d)) => a_d.cmp(&b_d), // Normal chronological order
                    (None, Some(_)) => std::cmp::Ordering::Less, // a has unknown date, goes first
                    (Some(_), None) => std::cmp::Ordering::Greater, // b has unknown date, goes first
                    (None, None) => a.rev.cmp(&b.rev), // Both unknown, use revision order
                }
            });

            if state_changes.is_empty() {
                history_spans.push(Span::styled(
                    "No state changes in history",
                    Style::default().fg(Color::Gray),
                ));
            } else {
                // Show first 5 and last 1 entries (Azure DevOps style) in chronological order
                let total_count = state_changes.len();
                let entries_to_show = if total_count <= 6 {
                    state_changes
                } else {
                    let mut entries = Vec::new();
                    entries.extend(state_changes[..5].iter().cloned()); // First 5 (earliest)
                    entries.push(state_changes[total_count - 1].clone()); // Last 1 (latest)
                    entries
                };

                for (i, history_entry) in entries_to_show.iter().enumerate() {
                    // Add separator for omitted entries (after showing first 5, before showing last 1)
                    if i == 5 && total_count > 6 {
                        if !history_spans.is_empty() {
                            history_spans
                                .push(Span::styled(" → ", Style::default().fg(Color::Gray)));
                        }
                        history_spans.push(Span::styled(
                            format!("... ({} omitted)", total_count - 6),
                            Style::default().fg(Color::Gray),
                        ));
                    }

                    if let Some(fields) = &history_entry.fields
                        && let Some(state_change) = &fields.state
                        && let Some(new_state) = &state_change.new_value
                    {
                        // Add arrow separator between entries (showing chronological flow)
                        if !history_spans.is_empty() {
                            history_spans
                                .push(Span::styled(" → ", Style::default().fg(Color::Gray)));
                        }

                        // Format date - use System.ChangedDate as primary source
                        let date_str = {
                            // First try System.ChangedDate
                            if let Some(fields) = &history_entry.fields {
                                if let Some(changed_date) = &fields.changed_date {
                                    if let Some(new_date) = &changed_date.new_value {
                                        if !new_date.starts_with("9999-01-01") {
                                            // Extract date part from System.ChangedDate
                                            if let Some(t_pos) = new_date.find('T') {
                                                &new_date[..t_pos]
                                            } else {
                                                new_date
                                            }
                                        } else {
                                            "Unknown date"
                                        }
                                    } else {
                                        "Unknown date"
                                    }
                                } else {
                                    // No System.ChangedDate, try revisedDate
                                    if !history_entry.revised_date.starts_with("9999-01-01") {
                                        if let Some(t_pos) = history_entry.revised_date.find('T') {
                                            &history_entry.revised_date[..t_pos]
                                        } else {
                                            &history_entry.revised_date
                                        }
                                    } else {
                                        "Unknown date"
                                    }
                                }
                            } else {
                                // No fields, try revisedDate
                                if !history_entry.revised_date.starts_with("9999-01-01") {
                                    if let Some(t_pos) = history_entry.revised_date.find('T') {
                                        &history_entry.revised_date[..t_pos]
                                    } else {
                                        &history_entry.revised_date
                                    }
                                } else {
                                    "Unknown date"
                                }
                            }
                        };

                        // Get color for the state
                        let state_color = get_state_color(new_state);

                        history_spans.push(Span::styled(
                            "●",
                            Style::default()
                                .fg(state_color)
                                .add_modifier(Modifier::BOLD),
                        ));
                        history_spans.push(Span::raw(" "));
                        history_spans.push(Span::styled(
                            new_state.clone(),
                            Style::default().fg(state_color),
                        ));
                        history_spans.push(Span::styled(
                            format!(" ({})", date_str),
                            Style::default().fg(Color::Gray),
                        ));
                    }
                }
            }
        }

        let history_line = Line::from(history_spans);
        let history_widget = Paragraph::new(vec![history_line])
            .style(Style::default().fg(Color::White))
            .block(Block::default().borders(Borders::ALL).title("History"))
            .wrap(ratatui::widgets::Wrap { trim: true });

        f.render_widget(history_widget, area);
    }

    fn render_search_status(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        use ratatui::text::{Line, Span};

        let search_query = if let Ok(query) = Self::parse_search_query(&self.last_search_query) {
            match query {
                SearchQuery::PullRequestId(id) => format!("PR ID: {}", id),
                SearchQuery::WorkItemId(id) => format!("Work Item ID: {}", id),
                SearchQuery::PullRequestTitle(title) => format!("PR Title: \"{}\"", title),
                SearchQuery::WorkItemTitle(title) => format!("Work Item Title: \"{}\"", title),
            }
        } else {
            self.last_search_query.clone()
        };

        let results_info = if !self.search_results.is_empty() {
            format!(
                "Result {} of {}",
                self.current_search_index + 1,
                self.search_results.len()
            )
        } else {
            "No results".to_string()
        };

        let status_line = Line::from(vec![
            Span::styled(
                "Search: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(search_query, Style::default().fg(Color::White)),
            Span::styled(" | ", Style::default().fg(Color::Gray)),
            Span::styled(
                results_info,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);

        let search_status = Paragraph::new(vec![status_line])
            .style(Style::default())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Search Status"),
            )
            .alignment(Alignment::Left);

        f.render_widget(search_status, area);
    }

    fn render_search_overlay(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        use ratatui::text::{Line, Span};
        use ratatui::widgets::Clear;

        // Create a centered popup area - smaller than state selection overlay
        let popup_area = {
            let vertical_margin = area.height / 3;
            let horizontal_margin = area.width / 3;
            ratatui::layout::Rect {
                x: area.x + horizontal_margin,
                y: area.y + vertical_margin,
                width: area.width - 2 * horizontal_margin,
                height: std::cmp::min(10, area.height - 2 * vertical_margin), // Fixed height
            }
        };

        // Clear the area first to ensure no transparency
        f.render_widget(Clear, popup_area);

        // Then render a block with solid background
        let background_block = Block::default()
            .style(Style::default().bg(Color::Black))
            .borders(Borders::NONE);
        f.render_widget(background_block, popup_area);

        // Create layout for title, input field, status, and help
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Length(3), // Input field
                Constraint::Length(2), // Status/Error message
                Constraint::Length(2), // Help text
            ])
            .split(popup_area);

        // Render title
        let title_widget = Paragraph::new("Search Pull Requests and Work Items")
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center);
        f.render_widget(title_widget, chunks[0]);

        // Render input field
        let input_widget = Paragraph::new(self.search_input.as_str())
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Query")
                    .title_style(Style::default().fg(Color::Yellow)),
            );
        f.render_widget(input_widget, chunks[1]);

        // Render status/error message or search results info
        let status_text = if let Some(error) = &self.search_error_message {
            Line::from(Span::styled(error, Style::default().fg(Color::Red)))
        } else if self.search_iteration_mode {
            let results_count = self.search_results.len();
            let current_pos = self.current_search_index + 1;
            Line::from(Span::styled(
                format!("Result {} of {}", current_pos, results_count),
                Style::default().fg(Color::Green),
            ))
        } else if !self.search_results.is_empty() {
            Line::from(Span::styled(
                format!("{} results found", self.search_results.len()),
                Style::default().fg(Color::Green),
            ))
        } else {
            Line::from(Span::styled(
                "Enter search query and press Enter",
                Style::default().fg(Color::Gray),
            ))
        };

        let status_widget = Paragraph::new(vec![status_text])
            .style(Style::default())
            .alignment(Alignment::Center);
        f.render_widget(status_widget, chunks[2]);

        // Render help
        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let help_lines = if self.search_iteration_mode {
            vec![Line::from(vec![
                Span::styled("n", key_style),
                Span::raw(": Next | "),
                Span::styled("N", key_style),
                Span::raw(": Previous | "),
                Span::styled("Esc", key_style),
                Span::raw("/"),
                Span::styled("Enter", key_style),
                Span::raw(": Exit search"),
            ])]
        } else {
            vec![Line::from(vec![
                Span::styled("!", key_style),
                Span::raw("123: PR ID | "),
                Span::styled("#", key_style),
                Span::raw("456: Work Item ID | "),
                Span::styled("PR:", key_style),
                Span::raw("text | "),
                Span::styled("WI:", key_style),
                Span::raw("text | "),
                Span::styled("Esc", key_style),
                Span::raw(": Cancel"),
            ])]
        };
        let help_widget = Paragraph::new(help_lines)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center);
        f.render_widget(help_widget, chunks[3]);
    }

    /// Convert mouse y-coordinate to table row index
    fn mouse_y_to_row(&self, y: u16, pr_count: usize) -> Option<usize> {
        let area = self.table_area?;

        // Table structure: border (1) + header (1) + data rows
        // So first data row starts at area.y + 2
        let first_row_y = area.y + 2;
        let last_row_y = area.y + area.height.saturating_sub(2); // -1 for bottom border, -1 for 0-indexing

        if y < first_row_y || y > last_row_y {
            return None;
        }

        let row = (y - first_row_y) as usize;

        // Account for table scroll offset
        let offset = self.table_state.offset();
        let actual_row = row + offset;

        if actual_row < pr_count {
            Some(actual_row)
        } else {
            None
        }
    }

    /// Check if coordinates are within table bounds
    fn is_in_table(&self, x: u16, y: u16) -> bool {
        if let Some(area) = self.table_area {
            x >= area.x && x < area.x + area.width && y >= area.y && y < area.y + area.height
        } else {
            false
        }
    }

    fn render_state_selection_overlay(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        use ratatui::text::{Line, Span};
        use ratatui::widgets::Clear;

        // Create a centered popup area
        let popup_area = {
            let vertical_margin = area.height / 4;
            let horizontal_margin = area.width / 4;
            ratatui::layout::Rect {
                x: area.x + horizontal_margin,
                y: area.y + vertical_margin,
                width: area.width - 2 * horizontal_margin,
                height: area.height - 2 * vertical_margin,
            }
        };

        // Clear the area first to ensure no transparency
        f.render_widget(Clear, popup_area);

        // Then render a block with solid background
        let background_block = Block::default()
            .style(Style::default().bg(Color::Black))
            .borders(Borders::NONE);
        f.render_widget(background_block, popup_area);

        // Create layout for title, states list, and help
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Min(5),    // States list
                Constraint::Length(4), // Help text
            ])
            .split(popup_area);

        // Render title
        let title_text = format!(
            "Select Work Item States ({} selected)",
            self.selected_filter_states.len()
        );
        let title_widget = Paragraph::new(title_text)
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center);
        f.render_widget(title_widget, chunks[0]);

        // Render states list
        let state_items: Vec<ListItem> = self
            .available_states
            .iter()
            .enumerate()
            .map(|(i, state)| {
                let checkbox = if self.selected_filter_states.contains(state) {
                    "✓"
                } else {
                    "☐"
                };

                let line = Line::from(vec![
                    Span::styled(
                        format!("{} ", checkbox),
                        Style::default()
                            .fg(if self.selected_filter_states.contains(state) {
                                Color::Green
                            } else {
                                Color::White
                            })
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        state.clone(),
                        Style::default().fg(if i == self.state_selection_index {
                            Color::Yellow
                        } else if self.selected_filter_states.contains(state) {
                            Color::Green
                        } else {
                            Color::White
                        }),
                    ),
                ]);

                ListItem::new(line).style(if i == self.state_selection_index {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                })
            })
            .collect();

        let states_list = List::new(state_items)
            .block(Block::default().borders(Borders::ALL).title("States"))
            .highlight_style(Style::default().bg(Color::DarkGray))
            .highlight_symbol("→ ");

        f.render_widget(states_list, chunks[1]);

        // Render help
        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let help_lines = vec![
            Line::from(vec![
                Span::styled("↑/↓", key_style),
                Span::raw(": Navigate | "),
                Span::styled("Space", key_style),
                Span::raw(": Toggle state | "),
                Span::styled("Enter", key_style),
                Span::raw(": Apply filter"),
            ]),
            Line::from(vec![
                Span::styled("c", key_style),
                Span::raw(": Clear & apply | "),
                Span::styled("a", key_style),
                Span::raw(": Select all states | "),
                Span::styled("Esc", key_style),
                Span::raw(": Cancel"),
            ]),
        ];
        let help_widget = Paragraph::new(help_lines)
            .style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::ALL).title("Help"))
            .alignment(Alignment::Center);
        f.render_widget(help_widget, chunks[2]);
    }

    /// Renders the dependency dialog overlay.
    fn render_dependency_dialog(&self, f: &mut Frame, area: Rect, app: &MergeApp) {
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Clear, Wrap};

        // Get the PR for this dialog
        let pr_index = match self.dependency_dialog_pr_index {
            Some(idx) => idx,
            None => return,
        };

        let pr_with_wi = match app.pull_requests().get(pr_index) {
            Some(pr) => pr,
            None => return,
        };

        let pr_id = pr_with_wi.pr.id;
        let pr_title = &pr_with_wi.pr.title;

        // Calculate popup dimensions (made larger)
        let popup_width = (area.width as f32 * 0.85).min(100.0) as u16;
        let popup_height = (area.height as f32 * 0.85).min(35.0) as u16;
        let popup_x = (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

        // Clear the area
        f.render_widget(Clear, popup_area);

        // Collect dependency information
        let mut lines: Vec<Line> = Vec::new();

        // Get dependency graph
        if let Some(graph) = app.dependency_graph() {
            // Build dependency trees
            let deps_tree = build_dependency_tree(graph, pr_id, app, true);
            let dependents_tree = build_dependency_tree(graph, pr_id, app, false);

            // Dependencies section
            lines.push(Line::from(Span::styled(
                "◀ Dependencies (PRs this PR depends on):",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )));

            if deps_tree.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  No dependencies",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                render_dependency_tree(&deps_tree, &mut lines, "", true);
            }

            lines.push(Line::from("")); // Spacer

            // Dependents section
            lines.push(Line::from(Span::styled(
                "▶ Dependents (PRs that depend on this PR):",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )));

            if dependents_tree.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  No dependents",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                render_dependency_tree(&dependents_tree, &mut lines, "", true);
            }
        } else {
            lines.push(Line::from(Span::styled(
                "Dependency graph not available",
                Style::default().fg(Color::Yellow),
            )));
            lines.push(Line::from(Span::styled(
                "(Requires local_repo to be configured)",
                Style::default().fg(Color::DarkGray),
            )));
        }

        // Apply scroll offset (reserve space for legend at bottom)
        let visible_height = popup_height.saturating_sub(5) as usize; // Account for borders, title, and legend
        let max_scroll = lines.len().saturating_sub(visible_height);
        let scroll = self.dependency_dialog_scroll.min(max_scroll);
        let visible_lines: Vec<Line> = lines
            .into_iter()
            .skip(scroll)
            .take(visible_height)
            .collect();

        // Render the dialog
        let title = format!(
            "Dependencies for PR #{} - {}",
            pr_id,
            truncate_title(pr_title, 40)
        );
        let dialog = Paragraph::new(visible_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .title_style(
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    )
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false });

        f.render_widget(dialog, popup_area);

        // Add legend at bottom (fixed, not scrollable) - two columns
        let legend_area = Rect::new(
            popup_x + 1,
            popup_y + popup_height.saturating_sub(2),
            popup_width.saturating_sub(2),
            1,
        );
        let legend = Paragraph::new(Line::from(vec![
            Span::styled("Direct: ", Style::default().fg(Color::DarkGray)),
            Span::styled("Cyan", Style::default().fg(Color::Cyan)),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled("Transitive: ", Style::default().fg(Color::DarkGray)),
            Span::styled("Gray", Style::default().fg(Color::DarkGray)),
            Span::styled("  •  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "[F]",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                ": Overlapping lines | ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                "[P]",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                ": Same files, different lines",
                Style::default().fg(Color::DarkGray),
            ),
        ]))
        .alignment(Alignment::Center);
        f.render_widget(legend, legend_area);

        // Add help line at bottom
        let help_area = Rect::new(
            popup_x,
            popup_y + popup_height.saturating_sub(1),
            popup_width,
            1,
        );
        let help = Paragraph::new("Press Esc/g/q to close, ↑/↓ to scroll")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        f.render_widget(help, help_area);
    }
}

/// Tree node representing a PR and its dependencies
#[derive(Debug, Clone)]
struct DependencyTreeNode {
    pr_id: i32,
    title: String,
    category: DependencyCategory,
    closed_date: Option<String>,
    children: Vec<DependencyTreeNode>,
}

/// Builds a dependency tree for display
fn build_dependency_tree(
    graph: &crate::core::operations::PRDependencyGraph,
    pr_id: i32,
    app: &MergeApp,
    build_dependencies: bool,
) -> Vec<DependencyTreeNode> {
    use std::collections::HashSet;

    let get_pr_info = |id: i32| -> (String, Option<String>) {
        app.pull_requests()
            .iter()
            .find(|pr| pr.pr.id == id)
            .map(|pr| (pr.pr.title.clone(), pr.pr.closed_date.clone()))
            .unwrap_or_else(|| (format!("PR #{}", id), None))
    };

    let mut visited = HashSet::new();
    visited.insert(pr_id);

    fn build_subtree(
        graph: &crate::core::operations::PRDependencyGraph,
        current_id: i32,
        visited: &mut HashSet<i32>,
        get_pr_info: &dyn Fn(i32) -> (String, Option<String>),
        build_dependencies: bool,
    ) -> Vec<DependencyTreeNode> {
        let node = match graph.get_node(current_id) {
            Some(n) => n,
            None => return Vec::new(),
        };

        if build_dependencies {
            // Build dependency tree (PRs this PR depends on)
            let mut children: Vec<DependencyTreeNode> = node
                .dependencies
                .iter()
                .filter_map(|dep| {
                    if visited.contains(&dep.to_pr_id) {
                        return None;
                    }
                    visited.insert(dep.to_pr_id);

                    let children = build_subtree(graph, dep.to_pr_id, visited, get_pr_info, true);
                    let (title, closed_date) = get_pr_info(dep.to_pr_id);

                    Some(DependencyTreeNode {
                        pr_id: dep.to_pr_id,
                        title,
                        category: dep.category.clone(),
                        closed_date,
                        children,
                    })
                })
                .collect();

            // Sort children by closed_date (newest first)
            children.sort_by(|a, b| match (&b.closed_date, &a.closed_date) {
                (Some(da), Some(db)) => da.cmp(db),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => b.pr_id.cmp(&a.pr_id),
            });
            children
        } else {
            // Build dependent tree (PRs that depend on this PR)
            let mut children: Vec<DependencyTreeNode> = node
                .dependents
                .iter()
                .filter_map(|&dependent_id| {
                    if visited.contains(&dependent_id) {
                        return None;
                    }

                    let dependent_node = graph.get_node(dependent_id)?;
                    let dep = dependent_node
                        .dependencies
                        .iter()
                        .find(|d| d.to_pr_id == current_id)?;

                    visited.insert(dependent_id);

                    let children = build_subtree(graph, dependent_id, visited, get_pr_info, false);
                    let (title, closed_date) = get_pr_info(dependent_id);

                    Some(DependencyTreeNode {
                        pr_id: dependent_id,
                        title,
                        category: dep.category.clone(),
                        closed_date,
                        children,
                    })
                })
                .collect();

            // Sort children by closed_date (newest first)
            children.sort_by(|a, b| match (&b.closed_date, &a.closed_date) {
                (Some(da), Some(db)) => da.cmp(db),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => b.pr_id.cmp(&a.pr_id),
            });
            children
        }
    }

    // Check if the PR exists in the graph
    if graph.get_node(pr_id).is_none() {
        return Vec::new();
    }

    let mut roots = build_subtree(graph, pr_id, &mut visited, &get_pr_info, build_dependencies);

    // Sort roots by closed_date (newest first, same as PR list)
    roots.sort_by(|a, b| match (&b.closed_date, &a.closed_date) {
        (Some(da), Some(db)) => da.cmp(db),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.pr_id.cmp(&a.pr_id),
    });

    roots
}

/// Renders a dependency tree with proper indentation and connectors
fn render_dependency_tree(
    nodes: &[DependencyTreeNode],
    lines: &mut Vec<Line>,
    prefix: &str,
    is_root: bool,
) {
    for (idx, node) in nodes.iter().enumerate() {
        let is_last = idx == nodes.len() - 1;

        let (cat_prefix, cat_color) = match &node.category {
            DependencyCategory::Dependent { .. } => ("[F] ", Color::Red),
            DependencyCategory::PartiallyDependent { .. } => ("[P] ", Color::Yellow),
            DependencyCategory::Independent => ("    ", Color::Green),
        };

        // Determine tree characters
        let (connector, child_prefix) = if is_last {
            ("└─ ", "   ")
        } else {
            ("├─ ", "│  ")
        };

        // Direct dependencies are cyan, transitive are gray
        let color = if is_root {
            Color::Cyan // Direct dependency
        } else {
            Color::DarkGray // Transitive dependency
        };

        lines.push(Line::from(vec![
            Span::styled(
                cat_prefix,
                Style::default().fg(cat_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}{}", prefix, connector),
                Style::default().fg(color),
            ),
            Span::styled(format!("#{} ", node.pr_id), Style::default().fg(color)),
            Span::styled(truncate_title(&node.title, 60), Style::default().fg(color)),
        ]));

        // Render children with updated prefix
        if !node.children.is_empty() {
            let new_prefix = format!("{}{}", prefix, child_prefix);
            render_dependency_tree(&node.children, lines, &new_prefix, false);
        }
    }
}

/// Truncates a title to fit within a given width.
fn truncate_title(title: &str, max_len: usize) -> String {
    if title.len() <= max_len {
        title.to_string()
    } else {
        format!("{}...", &title[..max_len.saturating_sub(3)])
    }
}

// ============================================================================
// ModeState Implementation
// ============================================================================

#[async_trait]
impl ModeState for PullRequestSelectionState {
    type Mode = MergeState;

    fn ui(&mut self, f: &mut Frame, app: &MergeApp) {
        // Initialize selection if not already set
        self.initialize_selection(app);

        // Always sync scrollbar state with current selection
        self.update_scrollbar_state(app.pull_requests().len());

        // Handle empty PR list
        if app.pull_requests().is_empty() {
            let empty_message =
                Paragraph::new("No pull requests found without merged tags.\n\nPress 'q' to quit.")
                    .style(Style::default().fg(Color::Yellow))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("No Pull Requests"),
                    )
                    .alignment(Alignment::Center);
            f.render_widget(empty_message, f.area());
            return;
        }

        // Add search status line if in search iteration mode
        // Adjust layout based on whether details pane is visible
        let chunks = match (self.search_iteration_mode, self.show_details) {
            (true, true) => Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3),      // Search status line
                    Constraint::Percentage(50), // PR table
                    Constraint::Min(0),         // Work item details (fills remaining)
                    Constraint::Length(3),      // Help section
                ])
                .split(f.area()),
            (true, false) => Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3), // Search status line
                    Constraint::Min(0),    // PR table (full height)
                    Constraint::Length(3), // Help section
                ])
                .split(f.area()),
            (false, true) => Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Percentage(50), // Top half for PR table
                    Constraint::Min(0), // Bottom half for work item details (fills remaining)
                    Constraint::Length(3), // Help section
                ])
                .split(f.area()),
            (false, false) => Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Min(0),    // PR table (full height)
                    Constraint::Length(3), // Help section
                ])
                .split(f.area()),
        };

        let mut chunk_idx = 0;

        // Render search status line if in search iteration mode
        if self.search_iteration_mode {
            self.render_search_status(f, chunks[chunk_idx]);
            chunk_idx += 1;
        }
        // Create table headers
        let header_cells = ["", "PR #", "Date", "Title", "Author", "Deps", "Work Items"]
            .iter()
            .map(|h| {
                Cell::from(*h).style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
            });
        let header = Row::new(header_cells).height(1);

        // Compute unselected dependencies (PRs that selected PRs depend on but aren't selected)
        let unselected_deps = compute_unselected_dependencies(app);
        let missing_deps_count = unselected_deps.len();

        // Create table rows
        let rows: Vec<Row> = app
            .pull_requests()
            .iter()
            .enumerate()
            .map(|(pr_index, pr_with_wi)| {
                let selected = if pr_with_wi.selected { "✓" } else { " " };

                let date = if let Some(closed_date) = &pr_with_wi.pr.closed_date {
                    if let Ok(date) = DateTime::parse_from_rfc3339(closed_date) {
                        date.format("%Y-%m-%d").to_string()
                    } else {
                        "Active".to_string()
                    }
                } else {
                    "Active".to_string()
                };

                let work_items = if !pr_with_wi.work_items.is_empty() {
                    pr_with_wi
                        .work_items
                        .iter()
                        .map(|wi| {
                            let state = wi.fields.state.as_deref().unwrap_or("Unknown");
                            format!("#{} ({})", wi.id, state)
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                } else {
                    String::new()
                };

                // Check if this row is a search result
                let is_search_result = self.search_results.contains(&pr_index);
                let is_current_search_result = self.search_iteration_mode
                    && !self.search_results.is_empty()
                    && self.search_results.get(self.current_search_index) == Some(&pr_index);

                // Check if this PR is an unselected dependency (missing dependency warning)
                let is_unselected_dep = unselected_deps.contains(&pr_with_wi.pr.id);

                // Apply background highlighting for selected items, unselected deps, and search results
                // Priority: Selected (green) > Unselected dep (orange/amber) > Search results (blue)
                let row_style = if pr_with_wi.selected {
                    Style::default().bg(Color::Rgb(0, 60, 0)) // Dark green
                } else if is_unselected_dep {
                    Style::default().bg(Color::Rgb(80, 40, 0)) // Orange/amber for missing deps
                } else if is_current_search_result {
                    Style::default().bg(Color::Blue)
                } else if is_search_result {
                    Style::default().bg(Color::Rgb(0, 0, 139)) // Dark blue
                } else {
                    Style::default()
                };

                // Get dependency counts for this PR
                let (partial_deps, full_deps) = get_dependency_counts(app, pr_with_wi.pr.id);
                let deps_text = format_deps_count(partial_deps, full_deps);
                let deps_style = get_deps_style(partial_deps, full_deps, pr_with_wi.selected);

                let cells = vec![
                    Cell::from(selected).style(if pr_with_wi.selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD)
                    }),
                    Cell::from(format!("{:<6}", pr_with_wi.pr.id)) // Left-aligned with fixed width
                        .style(if pr_with_wi.selected {
                            Style::default().fg(Color::White)
                        } else {
                            Style::default().fg(Color::Cyan)
                        }),
                    Cell::from(date).style(if pr_with_wi.selected {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default()
                    }),
                    Cell::from(pr_with_wi.pr.title.clone()).style(if pr_with_wi.selected {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default()
                    }),
                    Cell::from(pr_with_wi.pr.created_by.display_name.clone()).style(
                        if pr_with_wi.selected {
                            Style::default().fg(Color::White)
                        } else {
                            Style::default().fg(Color::Yellow)
                        },
                    ),
                    Cell::from(deps_text).style(deps_style),
                    Cell::from(work_items).style(if pr_with_wi.selected {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(get_work_items_color(&pr_with_wi.work_items))
                    }),
                ];

                Row::new(cells).height(1).style(row_style)
            })
            .collect();

        let table = Table::new(
            rows,
            vec![
                Constraint::Length(3),      // Selection checkbox
                Constraint::Length(8),      // PR # (fixed width)
                Constraint::Length(12),     // Date
                Constraint::Percentage(25), // Title (reduced from 30%)
                Constraint::Percentage(15), // Author (reduced from 20%)
                Constraint::Length(5),      // Deps (P/D format)
                Constraint::Percentage(25), // Work Items
            ],
        )
        .header(header)
        .block({
            let title = if missing_deps_count > 0 {
                format!("Pull Requests (⚠ {} missing deps)", missing_deps_count)
            } else {
                "Pull Requests".to_string()
            };
            let block = Block::default().borders(Borders::ALL).title(title);
            if missing_deps_count > 0 {
                block.border_style(Style::default().fg(Color::Yellow))
            } else {
                block
            }
        })
        .row_highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol("→ ");

        // Store the table area for mouse hit-testing
        let table_area = chunks[chunk_idx];
        self.table_area = Some(table_area);
        f.render_stateful_widget(table, table_area, &mut self.table_state);

        // Render scrollbar for the PR list
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));

        // Position scrollbar inside the table's right border
        let scrollbar_area = Rect {
            x: table_area.x + table_area.width.saturating_sub(1),
            y: table_area.y + 1,
            width: 1,
            height: table_area.height.saturating_sub(2),
        };

        f.render_stateful_widget(scrollbar, scrollbar_area, &mut self.scrollbar_state);
        chunk_idx += 1;

        // Render work item details if enabled
        if self.show_details {
            self.render_work_item_details(f, app, chunks[chunk_idx]);
            chunk_idx += 1;
        }

        let help_text = if self.search_iteration_mode {
            "↑/↓: Navigate PRs | ←/→: Navigate Work Items | n: Next result | N: Previous result | Esc: Exit search | Space: Toggle | Enter: Exit search | d: Details | r: Refresh | q: Quit"
        } else {
            "↑/↓: Navigate PRs | ←/→: Navigate Work Items | /: Search | Space: Toggle | Enter: Confirm | p: Open PR | w: Open Work Items | d: Details | g: Graph | s: Multi-select by states | r: Refresh | q: Quit"
        };

        // Build status summary for Help title
        let selected_count = app.pull_requests().iter().filter(|pr| pr.selected).count();
        let help_title = if selected_count > 0 {
            if missing_deps_count > 0 {
                format!(
                    "Help | Selected: {} | ⚠ Missing deps: {}",
                    selected_count, missing_deps_count
                )
            } else {
                format!("Help | Selected: {}", selected_count)
            }
        } else {
            "Help".to_string()
        };

        let help = List::new(vec![ListItem::new(help_text)])
            .block(Block::default().borders(Borders::ALL).title(help_title));

        f.render_widget(help, chunks[chunk_idx]);

        // Render state selection overlay if in multi-select mode
        if self.multi_select_mode {
            self.render_state_selection_overlay(f, f.area());
        }

        // Render search overlay if in search mode
        if self.search_mode {
            self.render_search_overlay(f, f.area());
        }

        // Render dependency dialog if open
        if self.show_dependency_dialog {
            self.render_dependency_dialog(f, f.area(), app);
        }
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut MergeApp) -> StateChange<MergeState> {
        // Handle dependency dialog mode first
        if self.show_dependency_dialog {
            match code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('g') => {
                    self.show_dependency_dialog = false;
                    self.dependency_dialog_pr_index = None;
                    self.dependency_dialog_scroll = 0;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.dependency_dialog_scroll = self.dependency_dialog_scroll.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.dependency_dialog_scroll = self.dependency_dialog_scroll.saturating_add(1);
                }
                _ => {}
            }
            return StateChange::Keep;
        }

        // Handle search iteration mode first (even when search_mode is false)
        if self.search_iteration_mode && !self.search_mode {
            match code {
                KeyCode::Char('n') => {
                    self.navigate_search_results(1);
                    return StateChange::Keep;
                }
                KeyCode::Char('N') => {
                    self.navigate_search_results(-1);
                    return StateChange::Keep;
                }
                KeyCode::Esc => {
                    self.exit_search_mode();
                    return StateChange::Keep;
                }
                KeyCode::Enter => {
                    // In search iteration mode, Enter should NOT go to version input
                    // but should exit search mode
                    self.exit_search_mode();
                    return StateChange::Keep;
                }
                _ => {
                    // For other keys, fall through to normal handling
                }
            }
        }

        if self.search_mode {
            // Handle search mode keys
            if self.search_iteration_mode {
                // In search result navigation mode
                match code {
                    KeyCode::Char('n') => {
                        self.navigate_search_results(1);
                        StateChange::Keep
                    }
                    KeyCode::Char('N') => {
                        self.navigate_search_results(-1);
                        StateChange::Keep
                    }
                    KeyCode::Esc | KeyCode::Enter => {
                        self.exit_search_mode();
                        StateChange::Keep
                    }
                    _ => StateChange::Keep,
                }
            } else {
                // In search input mode
                match code {
                    KeyCode::Char(c) => {
                        self.search_input.push(c);
                        StateChange::Keep
                    }
                    KeyCode::Backspace => {
                        self.search_input.pop();
                        StateChange::Keep
                    }
                    KeyCode::Enter => {
                        if !self.search_input.trim().is_empty() {
                            self.execute_search(app);
                            if !self.search_results.is_empty() {
                                // Close search dialog if we found results and entered iteration mode
                                self.search_mode = false;
                            }
                        }
                        StateChange::Keep
                    }
                    KeyCode::Esc => {
                        self.exit_search_mode();
                        StateChange::Keep
                    }
                    _ => StateChange::Keep,
                }
            }
        } else if self.multi_select_mode {
            // Handle multi-select mode keys
            match code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    self.exit_multi_select_mode();
                    StateChange::Keep
                }
                KeyCode::Up => {
                    self.previous_state();
                    StateChange::Keep
                }
                KeyCode::Down => {
                    self.next_state();
                    StateChange::Keep
                }
                KeyCode::Char(' ') => {
                    self.toggle_state_in_filter();
                    StateChange::Keep
                }
                KeyCode::Enter => {
                    self.select_all_with_filter_states(app);
                    self.exit_multi_select_mode();
                    StateChange::Keep
                }
                KeyCode::Char('c') => {
                    self.clear_all_selections(app);
                    self.exit_multi_select_mode();
                    StateChange::Keep
                }
                KeyCode::Char('a') => {
                    // Select all available states
                    for state in &self.available_states.clone() {
                        self.selected_filter_states.insert(state.clone());
                    }
                    StateChange::Keep
                }
                _ => StateChange::Keep,
            }
        } else {
            // Handle normal mode keys
            match code {
                KeyCode::Char('q') => StateChange::Exit,
                KeyCode::Up => {
                    self.previous(app);
                    StateChange::Keep
                }
                KeyCode::Down => {
                    self.next(app);
                    StateChange::Keep
                }
                KeyCode::Left => {
                    self.previous_work_item(app);
                    StateChange::Keep
                }
                KeyCode::Right => {
                    self.next_work_item(app);
                    StateChange::Keep
                }
                KeyCode::Char(' ') => {
                    self.toggle_selection(app);
                    StateChange::Keep
                }
                KeyCode::Char('s') => {
                    self.enter_multi_select_mode(app);
                    StateChange::Keep
                }
                KeyCode::Char('/') => {
                    self.enter_search_mode();
                    StateChange::Keep
                }
                KeyCode::Char('p') => {
                    if let Some(i) = self.table_state.selected()
                        && let Some(pr) = app.pull_requests().get(i)
                    {
                        app.open_pr_in_browser(pr.pr.id);
                    }
                    StateChange::Keep
                }
                KeyCode::Char('w') => {
                    if let Some(pr_index) = self.table_state.selected()
                        && let Some(pr) = app.pull_requests().get(pr_index)
                        && !pr.work_items.is_empty()
                    {
                        // Ensure work_item_index is within bounds
                        let work_item_index = if self.work_item_index < pr.work_items.len() {
                            self.work_item_index
                        } else {
                            0
                        };

                        if let Some(work_item) = pr.work_items.get(work_item_index) {
                            // Open only the currently displayed work item
                            app.open_work_items_in_browser(std::slice::from_ref(work_item));
                        }
                    }
                    StateChange::Keep
                }
                KeyCode::Char('d') => {
                    // Toggle details pane
                    self.show_details = !self.show_details;
                    StateChange::Keep
                }
                KeyCode::Char('g') => {
                    // Open dependency graph dialog for highlighted PR
                    if let Some(selected_idx) = self.table_state.selected() {
                        self.show_dependency_dialog = true;
                        self.dependency_dialog_pr_index = Some(selected_idx);
                        self.dependency_dialog_scroll = 0;
                    }
                    StateChange::Keep
                }
                KeyCode::Enter => {
                    if app.get_selected_prs().is_empty() {
                        StateChange::Keep
                    } else {
                        StateChange::Change(MergeState::VersionInput(VersionInputState::new()))
                    }
                }
                KeyCode::Char('r') => {
                    // Refresh: go back to data loading state to re-fetch PRs
                    StateChange::Change(MergeState::DataLoading(DataLoadingState::new()))
                }
                _ => StateChange::Keep,
            }
        }
    }

    async fn process_mouse(
        &mut self,
        event: MouseEvent,
        app: &mut MergeApp,
    ) -> StateChange<MergeState> {
        // Don't process mouse events in search mode or multi-select mode
        if self.search_mode || self.multi_select_mode {
            return StateChange::Keep;
        }

        match event.kind {
            MouseEventKind::ScrollUp => {
                if self.is_in_table(event.column, event.row) {
                    self.previous(app);
                }
                StateChange::Keep
            }
            MouseEventKind::ScrollDown => {
                if self.is_in_table(event.column, event.row) {
                    self.next(app);
                }
                StateChange::Keep
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(row) = self.mouse_y_to_row(event.row, app.pull_requests().len()) {
                    let now = Instant::now();
                    let is_double_click = self
                        .last_click_time
                        .map(|t| now.duration_since(t).as_millis() < 500)
                        .unwrap_or(false)
                        && self.last_click_row == Some(row);

                    if is_double_click {
                        // Double-click: toggle selection
                        self.table_state.select(Some(row));
                        self.work_item_index = 0;
                        self.toggle_selection(app);
                        // Reset for next double-click detection
                        self.last_click_time = None;
                        self.last_click_row = None;
                    } else {
                        // Single click: highlight (select) the row
                        self.table_state.select(Some(row));
                        self.work_item_index = 0;
                        self.last_click_time = Some(now);
                        self.last_click_row = Some(row);
                    }
                }
                StateChange::Keep
            }
            _ => StateChange::Keep,
        }
    }

    fn name(&self) -> &'static str {
        "PullRequestSelection"
    }
}

fn get_work_items_color(work_items: &[crate::models::WorkItem]) -> Color {
    if work_items.is_empty() {
        return Color::Gray;
    }

    // Return color based on the most important state
    for wi in work_items {
        if let Some(state) = &wi.fields.state {
            match state.as_str() {
                "Next Merged" | "Next Closed" => return get_state_color(state),
                _ => {}
            }
        }
    }

    work_items
        .iter()
        .filter_map(|wi| wi.fields.state.as_deref())
        .next()
        .map(get_state_color)
        .unwrap_or(Color::White)
}

fn get_state_color(state: &str) -> Color {
    match state {
        "Dev Closed" => Color::LightGreen,
        "Closed" => Color::Green,
        "Resolved" => Color::Rgb(255, 165, 0),
        "In Review" => Color::Yellow,
        "New" => Color::Gray,
        "Active" => Color::Blue,
        "Next Merged" => Color::Red,
        "Next Closed" => Color::Magenta,
        "Hold" => Color::Cyan,
        _ => Color::White,
    }
}

/// Returns the dependency counts (partial, full) for a PR.
///
/// Returns (0, 0) if dependency graph is not available.
fn get_dependency_counts(app: &MergeApp, pr_id: i32) -> (usize, usize) {
    if let Some(graph) = app.dependency_graph()
        && let Some(node) = graph.get_node(pr_id)
    {
        let mut partial = 0;
        let mut full = 0;
        for dep in &node.dependencies {
            match &dep.category {
                DependencyCategory::PartiallyDependent { .. } => partial += 1,
                DependencyCategory::Dependent { .. } => full += 1,
                DependencyCategory::Independent => {}
            }
        }
        return (partial, full);
    }
    (0, 0)
}

/// Returns the style for the dependency column based on counts.
fn get_deps_style(partial: usize, full: usize, is_selected: bool) -> Style {
    if is_selected {
        Style::default().fg(Color::White)
    } else if full > 0 {
        Style::default().fg(Color::Red)
    } else if partial > 0 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Green)
    }
}

/// Formats the dependency count as "P/D".
fn format_deps_count(partial: usize, full: usize) -> String {
    format!("{}/{}", partial, full)
}

/// Computes the set of PR IDs that are dependencies of selected PRs but are not selected.
///
/// This function finds all PRs that any currently selected PR depends on,
/// but which are not themselves selected. These represent "missing" dependencies
/// that should be highlighted to warn the user.
fn compute_unselected_dependencies(app: &MergeApp) -> HashSet<i32> {
    let mut unselected_deps = HashSet::new();

    let Some(graph) = app.dependency_graph() else {
        return unselected_deps;
    };

    // Collect IDs of selected PRs
    let selected_ids: HashSet<i32> = app
        .pull_requests()
        .iter()
        .filter(|pr| pr.selected)
        .map(|pr| pr.pr.id)
        .collect();

    // For each selected PR, find its dependencies that are not selected
    for selected_id in &selected_ids {
        if let Some(node) = graph.get_node(*selected_id) {
            for dep in &node.dependencies {
                // Only include PRs that are in our list (not already merged)
                // and are not currently selected
                if !selected_ids.contains(&dep.to_pr_id) {
                    // Verify this PR is in our list
                    if app
                        .pull_requests()
                        .iter()
                        .any(|pr| pr.pr.id == dep.to_pr_id)
                    {
                        unselected_deps.insert(dep.to_pr_id);
                    }
                }
            }
        }
    }

    unselected_deps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        snapshot_testing::with_settings_and_module_path,
        state::typed::AppState,
        testing::{TuiTestHarness, create_test_config_default, create_test_pull_requests},
    };
    use insta::assert_snapshot;

    /// # PR Selection State - Normal Display
    ///
    /// Tests the pull request selection screen with normal data.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state
    /// - Loads test pull requests into the app
    /// - Renders the PR table and work item details
    ///
    /// ## Expected Outcome
    /// - Should display PR table with columns: checkbox, PR#, Date, Title, Author, Work Items
    /// - Should show work item details panel
    /// - Should display help text at bottom
    #[test]
    fn test_pr_selection_normal() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let mut state = MergeState::PullRequestSelection(PullRequestSelectionState::new());
            harness.render_merge_state(&mut state);

            assert_snapshot!("normal_display", harness.backend());
        });
    }

    /// # PR Selection State - Empty List
    ///
    /// Tests the PR selection screen with no pull requests.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state
    /// - Does not load any pull requests
    /// - Renders the empty state
    ///
    /// ## Expected Outcome
    /// - Should display "No pull requests found" message
    /// - Should show quit instruction
    #[test]
    fn test_pr_selection_empty() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            // Leave pull_requests empty

            let mut state = MergeState::PullRequestSelection(PullRequestSelectionState::new());
            harness.render_merge_state(&mut state);

            assert_snapshot!("empty_list", harness.backend());
        });
    }

    /// # PR Selection State - With Selections
    ///
    /// Tests the PR selection screen with some PRs selected.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state
    /// - Loads test pull requests
    /// - Marks some PRs as selected
    /// - Renders the display
    ///
    /// ## Expected Outcome
    /// - Should display checkmarks for selected PRs
    /// - Selected rows should have different background color
    /// - Text color should change for selected items
    #[test]
    fn test_pr_selection_with_selections() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut prs = create_test_pull_requests();
            prs[0].selected = true;
            prs[2].selected = true;
            *harness.app.pull_requests_mut() = prs;

            let mut state = MergeState::PullRequestSelection(PullRequestSelectionState::new());
            harness.render_merge_state(&mut state);

            assert_snapshot!("with_selections", harness.backend());
        });
    }

    /// # PR Selection State - Search Mode
    ///
    /// Tests the PR selection screen in search mode.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state
    /// - Loads test pull requests
    /// - Enters search mode with query
    /// - Renders the display
    ///
    /// ## Expected Outcome
    /// - Should display search status line
    /// - Should show search results highlighting
    /// - Should display search-specific help text
    #[test]
    fn test_pr_selection_search_mode() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let mut inner_state = PullRequestSelectionState::new();
            inner_state.search_iteration_mode = true;
            inner_state.last_search_query = "login".to_string();
            inner_state.search_results = vec![0];
            inner_state.current_search_index = 0;
            let mut state = MergeState::PullRequestSelection(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("search_mode", harness.backend());
        });
    }

    /// # PR Selection State - State Dialog With Selections
    ///
    /// Tests the state selection dialog overlay with multiple states selected.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state
    /// - Enters multi-select mode (state filter dialog)
    /// - Multiple work item states available: Active, Resolved, Closed, New, Removed
    /// - Some states are selected (Resolved, Closed)
    /// - Renders the state selection overlay
    ///
    /// ## Expected Outcome
    /// - Should display state selection dialog overlay
    /// - Should show checkboxes for all available states
    /// - Should mark selected states with checkmarks
    /// - Should display help text for state selection
    #[test]
    fn test_pr_selection_state_dialog_with_selections() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let mut inner_state = PullRequestSelectionState::new();
            inner_state.multi_select_mode = true;
            inner_state.available_states = crate::ui::testing::create_test_work_item_states();
            inner_state.selected_filter_states = ["Resolved".to_string(), "Closed".to_string()]
                .iter()
                .cloned()
                .collect();
            inner_state.state_selection_index = 1;

            let mut state = MergeState::PullRequestSelection(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("state_dialog_with_selections", harness.backend());
        });
    }

    /// # PR Selection State - State Dialog No Selections
    ///
    /// Tests the state selection dialog overlay with no states selected.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state
    /// - Enters multi-select mode (state filter dialog)
    /// - Multiple work item states available
    /// - No states are selected (all checkboxes empty)
    /// - Renders the state selection overlay
    ///
    /// ## Expected Outcome
    /// - Should display state selection dialog overlay
    /// - Should show empty checkboxes for all states
    /// - Should highlight currently focused state
    /// - Should display help text for selection
    #[test]
    fn test_pr_selection_state_dialog_no_selections() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let mut inner_state = PullRequestSelectionState::new();
            inner_state.multi_select_mode = true;
            inner_state.available_states = crate::ui::testing::create_test_work_item_states();
            inner_state.selected_filter_states = std::collections::HashSet::new();
            inner_state.state_selection_index = 0;

            let mut state = MergeState::PullRequestSelection(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("state_dialog_no_selections", harness.backend());
        });
    }

    /// # PR Selection State - Scrollable Many Items
    ///
    /// Tests the PR selection screen with 50+ items to verify scrolling.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state
    /// - Loads 60 pull requests (exceeds terminal height of 30)
    /// - Some PRs selected in the middle of the list
    /// - Renders the display
    ///
    /// ## Expected Outcome
    /// - Should display scrollable table
    /// - Should show scroll indicators
    /// - Should handle large number of items gracefully
    /// - Should properly highlight selected items
    #[test]
    fn test_pr_selection_scrollable_many_items() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.pull_requests_mut() = crate::ui::testing::create_large_pr_list();

            let mut inner_state = PullRequestSelectionState::new();
            // Scroll to middle of list to show scrolling behavior
            inner_state.table_state.select(Some(25));

            let mut state = MergeState::PullRequestSelection(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("scrollable_many_items", harness.backend());
        });
    }

    /// # PR Selection State - State Dialog All Selected
    ///
    /// Tests the state selection dialog overlay with all states selected.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state
    /// - Enters multi-select mode (state filter dialog)
    /// - All available work item states are selected
    /// - Simulates pressing 'a' to select all
    /// - Renders the state selection overlay
    ///
    /// ## Expected Outcome
    /// - Should display state selection dialog overlay
    /// - Should show all checkboxes marked
    /// - Should display selection count in title
    /// - Should show help text for clearing selection
    #[test]
    fn test_pr_selection_state_dialog_all_selected() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let states = crate::ui::testing::create_test_work_item_states();
            let mut inner_state = PullRequestSelectionState::new();
            inner_state.multi_select_mode = true;
            inner_state.available_states = states.clone();
            inner_state.selected_filter_states = states.iter().cloned().collect();
            inner_state.state_selection_index = 2;

            let mut state = MergeState::PullRequestSelection(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("state_dialog_all_selected", harness.backend());
        });
    }

    /// # Scrollbar State - Navigation Next
    ///
    /// Tests that scrollbar state is properly updated when navigating to next item.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state with pull requests
    /// - Calls next() to navigate down
    /// - Verifies selection updates correctly
    ///
    /// ## Expected Outcome
    /// - Table selection should update on navigation
    /// - Scrollbar state sync should not panic
    #[test]
    fn test_scrollbar_state_updates_on_next() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.initialize_selection(harness.merge_app());

        // Initial position should be 0
        assert_eq!(state.table_state.selected(), Some(0));

        // Navigate to next
        state.next(harness.merge_app());
        assert_eq!(state.table_state.selected(), Some(1));

        // Navigate again
        state.next(harness.merge_app());
        assert_eq!(state.table_state.selected(), Some(2));

        // Wrap around to beginning
        state.next(harness.merge_app());
        assert_eq!(state.table_state.selected(), Some(0));
    }

    /// # Scrollbar State - Navigation Previous
    ///
    /// Tests that scrollbar state is properly updated when navigating to previous item.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state with pull requests
    /// - Calls previous() to navigate up
    /// - Verifies selection updates correctly
    ///
    /// ## Expected Outcome
    /// - Table selection should update on navigation
    /// - Should wrap to end when at beginning
    #[test]
    fn test_scrollbar_state_updates_on_previous() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.initialize_selection(harness.merge_app());

        // Initial position should be 0
        assert_eq!(state.table_state.selected(), Some(0));

        // Navigate to previous (should wrap to end)
        state.previous(harness.merge_app());
        let last_idx = harness.app.pull_requests().len() - 1;
        assert_eq!(state.table_state.selected(), Some(last_idx));

        // Navigate previous again
        state.previous(harness.merge_app());
        assert_eq!(state.table_state.selected(), Some(last_idx - 1));
    }

    /// # Scrollbar State - Initialize Selection
    ///
    /// Tests that scrollbar state is properly initialized.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state
    /// - Initializes selection with pull requests
    /// - Verifies initial selection
    ///
    /// ## Expected Outcome
    /// - Selection should start at first item
    /// - Scrollbar state sync should not panic
    #[test]
    fn test_scrollbar_state_initialized_correctly() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.initialize_selection(harness.merge_app());

        assert_eq!(state.table_state.selected(), Some(0));
    }

    /// # Scrollbar State - Empty List
    ///
    /// Tests scrollbar state behavior with empty PR list.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state with no pull requests
    /// - Attempts navigation
    ///
    /// ## Expected Outcome
    /// - Should handle empty list gracefully
    /// - No panics on navigation
    #[test]
    fn test_scrollbar_state_empty_list() {
        let config = create_test_config_default();
        let harness = TuiTestHarness::with_config(config);
        // Leave pull_requests empty

        let mut state = PullRequestSelectionState::new();
        state.initialize_selection(harness.merge_app());

        // Should not panic
        state.next(harness.merge_app());
        state.previous(harness.merge_app());

        // Selection should remain None for empty list
        assert_eq!(state.table_state.selected(), None);
    }

    /// # PR Selection State - Mouse Scroll Down
    ///
    /// Tests that scrolling down with mouse wheel moves selection to next item.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state with PRs loaded
    /// - First renders to populate table_area
    /// - Simulates mouse scroll down event within table bounds
    ///
    /// ## Expected Outcome
    /// - Selection should move from first item (0) to second item (1)
    /// - Display should show second PR highlighted
    #[test]
    fn test_mouse_scroll_down() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let mut inner_state = PullRequestSelectionState::new();
            inner_state.table_state.select(Some(0));

            let mut state = MergeState::PullRequestSelection(inner_state);

            // First render to populate table_area
            harness.render_merge_state(&mut state);

            // Simulate scroll down within table area
            let event = MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 10,
                row: 5, // Within table area
                modifiers: crossterm::event::KeyModifiers::NONE,
            };

            // Process mouse event
            tokio_test::block_on(async {
                let app = harness.merge_app_mut();
                state.process_mouse(event, app).await;
            });

            // Re-render after mouse event
            harness.render_merge_state(&mut state);

            assert_snapshot!("mouse_scroll_down", harness.backend());
        });
    }

    /// # PR Selection State - Mouse Scroll Up
    ///
    /// Tests that scrolling up with mouse wheel moves selection to previous item.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state with PRs loaded
    /// - Starts with second item selected
    /// - Simulates mouse scroll up event within table bounds
    ///
    /// ## Expected Outcome
    /// - Selection should move from second item (1) to first item (0)
    /// - Display should show first PR highlighted
    #[test]
    fn test_mouse_scroll_up() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let mut inner_state = PullRequestSelectionState::new();
            inner_state.table_state.select(Some(1)); // Start at second item

            let mut state = MergeState::PullRequestSelection(inner_state);

            // First render to populate table_area
            harness.render_merge_state(&mut state);

            // Simulate scroll up within table area
            let event = MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 10,
                row: 5,
                modifiers: crossterm::event::KeyModifiers::NONE,
            };

            tokio_test::block_on(async {
                let app = harness.merge_app_mut();
                state.process_mouse(event, app).await;
            });

            harness.render_merge_state(&mut state);

            assert_snapshot!("mouse_scroll_up", harness.backend());
        });
    }

    /// # PR Selection State - Mouse Click Highlight
    ///
    /// Tests that clicking on a row highlights (selects) that row.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state with PRs loaded
    /// - First item is initially selected
    /// - Simulates mouse click on third row
    ///
    /// ## Expected Outcome
    /// - Selection should move to the clicked row (index 2)
    /// - Third PR should be highlighted
    #[test]
    fn test_mouse_click_highlight() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let mut inner_state = PullRequestSelectionState::new();
            inner_state.table_state.select(Some(0));

            let mut state = MergeState::PullRequestSelection(inner_state);

            // First render to populate table_area
            harness.render_merge_state(&mut state);

            // Calculate row position: table starts at y=1 (after margin), +2 for border+header
            // So first data row is at y=3, second at y=4, third at y=5
            let click_y = 5; // Third row (index 2)

            let event = MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 10,
                row: click_y,
                modifiers: crossterm::event::KeyModifiers::NONE,
            };

            tokio_test::block_on(async {
                let app = harness.merge_app_mut();
                state.process_mouse(event, app).await;
            });

            harness.render_merge_state(&mut state);

            assert_snapshot!("mouse_click_highlight", harness.backend());
        });
    }

    /// # PR Selection State - Mouse Double Click Toggle
    ///
    /// Tests that double-clicking on a row toggles its selection state.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state with PRs loaded
    /// - Simulates two rapid mouse clicks on the same row (double-click)
    ///
    /// ## Expected Outcome
    /// - The clicked row should be toggled (selected with checkmark)
    /// - Display should show the PR with selection indicator
    #[test]
    fn test_mouse_double_click_toggle() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let mut inner_state = PullRequestSelectionState::new();
            inner_state.table_state.select(Some(0));

            let mut state = MergeState::PullRequestSelection(inner_state);

            // First render to populate table_area
            harness.render_merge_state(&mut state);

            let click_y = 3; // First row (index 0)

            // First click
            let event1 = MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 10,
                row: click_y,
                modifiers: crossterm::event::KeyModifiers::NONE,
            };

            tokio_test::block_on(async {
                let app = harness.merge_app_mut();
                state.process_mouse(event1, app).await;
            });

            // Second click (same position, within 500ms - simulated by not changing last_click_time)
            let event2 = MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 10,
                row: click_y,
                modifiers: crossterm::event::KeyModifiers::NONE,
            };

            tokio_test::block_on(async {
                let app = harness.merge_app_mut();
                state.process_mouse(event2, app).await;
            });

            harness.render_merge_state(&mut state);

            assert_snapshot!("mouse_double_click_toggle", harness.backend());
        });
    }

    /// # PR Selection State - Quit Key
    ///
    /// Tests 'q' key to exit.
    ///
    /// ## Test Scenario
    /// - Processes 'q' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Exit
    #[tokio::test]
    async fn test_pr_selection_quit() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('q'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Exit));
    }

    /// # PR Selection State - Navigate Up Key
    ///
    /// Tests up arrow key navigation.
    ///
    /// ## Test Scenario
    /// - Processes Up key
    ///
    /// ## Expected Outcome
    /// - Should move selection up
    #[tokio::test]
    async fn test_pr_selection_navigate_up() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.table_state.select(Some(1));

        let result = ModeState::process_key(&mut state, KeyCode::Up, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        assert_eq!(state.table_state.selected(), Some(0));
    }

    /// # PR Selection State - Navigate Down Key
    ///
    /// Tests down arrow key navigation.
    ///
    /// ## Test Scenario
    /// - Processes Down key
    ///
    /// ## Expected Outcome
    /// - Should move selection down
    #[tokio::test]
    async fn test_pr_selection_navigate_down() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.table_state.select(Some(0));

        let result =
            ModeState::process_key(&mut state, KeyCode::Down, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        assert_eq!(state.table_state.selected(), Some(1));
    }

    /// # PR Selection State - Space Toggle Selection
    ///
    /// Tests space key to toggle selection.
    ///
    /// ## Test Scenario
    /// - Processes Space key
    ///
    /// ## Expected Outcome
    /// - Should toggle the current PR's selection
    #[tokio::test]
    async fn test_pr_selection_toggle_space() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.table_state.select(Some(0));

        assert!(!harness.app.pull_requests()[0].selected);

        let result =
            ModeState::process_key(&mut state, KeyCode::Char(' '), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        assert!(harness.app.pull_requests()[0].selected);

        // Toggle again
        let result =
            ModeState::process_key(&mut state, KeyCode::Char(' '), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        assert!(!harness.app.pull_requests()[0].selected);
    }

    /// # PR Selection State - Enter Without Selection
    ///
    /// Tests Enter key when no PRs are selected.
    ///
    /// ## Test Scenario
    /// - Processes Enter key with no PRs selected
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep (don't proceed)
    #[tokio::test]
    async fn test_pr_selection_enter_no_selection() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();

        let result =
            ModeState::process_key(&mut state, KeyCode::Enter, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
    }

    /// # PR Selection State - Enter With Selection
    ///
    /// Tests Enter key when PRs are selected.
    ///
    /// ## Test Scenario
    /// - Selects a PR
    /// - Processes Enter key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Change to VersionInputState
    #[tokio::test]
    async fn test_pr_selection_enter_with_selection() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut prs = create_test_pull_requests();
        prs[0].selected = true;
        *harness.app.pull_requests_mut() = prs;

        let mut state = PullRequestSelectionState::new();

        let result =
            ModeState::process_key(&mut state, KeyCode::Enter, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # PR Selection State - Refresh Key
    ///
    /// Tests 'r' key to refresh data.
    ///
    /// ## Test Scenario
    /// - Processes 'r' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Change to DataLoadingState
    #[tokio::test]
    async fn test_pr_selection_refresh() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('r'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Change(_)));
    }

    /// # PR Selection State - Enter Search Mode
    ///
    /// Tests '/' key to enter search mode.
    ///
    /// ## Test Scenario
    /// - Processes '/' key
    ///
    /// ## Expected Outcome
    /// - Should enter search mode
    #[tokio::test]
    async fn test_pr_selection_enter_search() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        assert!(!state.search_mode);

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('/'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        assert!(state.search_mode);
    }

    /// # PR Selection State - Enter Multi-Select Mode
    ///
    /// Tests 's' key to enter multi-select mode.
    ///
    /// ## Test Scenario
    /// - Processes 's' key
    ///
    /// ## Expected Outcome
    /// - Should enter multi-select mode
    #[tokio::test]
    async fn test_pr_selection_enter_multi_select() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        assert!(!state.multi_select_mode);

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('s'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        assert!(state.multi_select_mode);
    }

    /// # PR Selection State - Search Mode Input
    ///
    /// Tests typing in search mode.
    ///
    /// ## Test Scenario
    /// - Enters search mode
    /// - Types characters
    ///
    /// ## Expected Outcome
    /// - Should build search input
    #[tokio::test]
    async fn test_pr_selection_search_input() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.search_mode = true;

        ModeState::process_key(&mut state, KeyCode::Char('t'), harness.merge_app_mut()).await;
        ModeState::process_key(&mut state, KeyCode::Char('e'), harness.merge_app_mut()).await;
        ModeState::process_key(&mut state, KeyCode::Char('s'), harness.merge_app_mut()).await;
        ModeState::process_key(&mut state, KeyCode::Char('t'), harness.merge_app_mut()).await;

        assert_eq!(state.search_input, "test");

        // Test backspace
        ModeState::process_key(&mut state, KeyCode::Backspace, harness.merge_app_mut()).await;
        assert_eq!(state.search_input, "tes");
    }

    /// # PR Selection State - Search Mode Escape
    ///
    /// Tests escaping from search mode.
    ///
    /// ## Test Scenario
    /// - Enters search mode
    /// - Presses Escape
    ///
    /// ## Expected Outcome
    /// - Should exit search mode
    #[tokio::test]
    async fn test_pr_selection_search_escape() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.search_mode = true;

        let result =
            ModeState::process_key(&mut state, KeyCode::Esc, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        assert!(!state.search_mode);
    }

    /// # PR Selection State - Multi-Select Navigate States
    ///
    /// Tests navigating states in multi-select mode.
    ///
    /// ## Test Scenario
    /// - Enters multi-select mode
    /// - Presses Up/Down
    ///
    /// ## Expected Outcome
    /// - Should navigate state selection
    #[tokio::test]
    async fn test_pr_selection_multi_select_navigate() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.multi_select_mode = true;
        state.available_states = crate::ui::testing::create_test_work_item_states();
        state.state_selection_index = 0;

        ModeState::process_key(&mut state, KeyCode::Down, harness.merge_app_mut()).await;
        assert_eq!(state.state_selection_index, 1);

        ModeState::process_key(&mut state, KeyCode::Up, harness.merge_app_mut()).await;
        assert_eq!(state.state_selection_index, 0);
    }

    /// # PR Selection State - Multi-Select Toggle State
    ///
    /// Tests toggling state selection in multi-select mode.
    ///
    /// ## Test Scenario
    /// - Enters multi-select mode
    /// - Presses Space to toggle state
    ///
    /// ## Expected Outcome
    /// - Should toggle state in filter
    #[tokio::test]
    async fn test_pr_selection_multi_select_toggle() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.multi_select_mode = true;
        state.available_states = crate::ui::testing::create_test_work_item_states();
        state.state_selection_index = 0;

        assert!(state.selected_filter_states.is_empty());

        ModeState::process_key(&mut state, KeyCode::Char(' '), harness.merge_app_mut()).await;
        assert!(
            state
                .selected_filter_states
                .contains(&state.available_states[0])
        );
    }

    /// # PR Selection State - Multi-Select Exit
    ///
    /// Tests exiting multi-select mode.
    ///
    /// ## Test Scenario
    /// - Enters multi-select mode
    /// - Presses Escape
    ///
    /// ## Expected Outcome
    /// - Should exit multi-select mode
    #[tokio::test]
    async fn test_pr_selection_multi_select_exit() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.multi_select_mode = true;

        let result =
            ModeState::process_key(&mut state, KeyCode::Esc, harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
        assert!(!state.multi_select_mode);
    }

    /// # PR Selection State - Work Item Navigation
    ///
    /// Tests left/right navigation for work items.
    ///
    /// ## Test Scenario
    /// - Processes Left/Right keys
    ///
    /// ## Expected Outcome
    /// - Should navigate through work items
    #[tokio::test]
    async fn test_pr_selection_work_item_navigation() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.table_state.select(Some(2)); // Select PR with multiple work items
        state.work_item_index = 0;

        ModeState::process_key(&mut state, KeyCode::Right, harness.merge_app_mut()).await;
        assert_eq!(state.work_item_index, 1);

        ModeState::process_key(&mut state, KeyCode::Left, harness.merge_app_mut()).await;
        assert_eq!(state.work_item_index, 0);
    }

    /// # PR Selection State - Open PR Key
    ///
    /// Tests 'p' key to open PR in browser.
    ///
    /// ## Test Scenario
    /// - Processes 'p' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep
    #[tokio::test]
    async fn test_pr_selection_open_pr() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.table_state.select(Some(0));

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('p'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
    }

    /// # PR Selection State - Open Work Items Key
    ///
    /// Tests 'w' key to open work items in browser.
    ///
    /// ## Test Scenario
    /// - Processes 'w' key
    ///
    /// ## Expected Outcome
    /// - Should return StateChange::Keep
    #[tokio::test]
    async fn test_pr_selection_open_work_items() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.table_state.select(Some(0));

        let result =
            ModeState::process_key(&mut state, KeyCode::Char('w'), harness.merge_app_mut()).await;
        assert!(matches!(result, StateChange::Keep));
    }

    /// # PR Selection State - Search Dialog Display
    ///
    /// Tests rendering of the search overlay dialog.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state in search mode
    /// - Enters a search query
    /// - Renders the search dialog
    ///
    /// ## Expected Outcome
    /// - Should display search dialog with input field
    /// - Should show help text for search
    #[test]
    fn test_pr_selection_search_dialog() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let mut inner_state = PullRequestSelectionState::new();
            inner_state.search_mode = true;
            inner_state.search_input = "login".to_string();

            let mut state = MergeState::PullRequestSelection(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("search_dialog", harness.backend());
        });
    }

    /// # PR Selection State - Search Results Status
    ///
    /// Tests search dialog with results found.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state in search mode
    /// - Sets search results
    /// - Renders the search dialog
    ///
    /// ## Expected Outcome
    /// - Should show results count in status
    #[test]
    fn test_pr_selection_search_with_results() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let mut inner_state = PullRequestSelectionState::new();
            inner_state.search_mode = true;
            inner_state.search_input = "login".to_string();
            inner_state.search_results = vec![0, 1];

            let mut state = MergeState::PullRequestSelection(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("search_with_results", harness.backend());
        });
    }

    /// # PR Selection State - Search Error Display
    ///
    /// Tests search dialog with error message.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state in search mode
    /// - Sets an error message
    /// - Renders the search dialog
    ///
    /// ## Expected Outcome
    /// - Should show error message in red
    #[test]
    fn test_pr_selection_search_error() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let mut inner_state = PullRequestSelectionState::new();
            inner_state.search_mode = true;
            inner_state.search_input = "!abc".to_string();
            inner_state.search_error_message = Some("Invalid PR ID format".to_string());

            let mut state = MergeState::PullRequestSelection(inner_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("search_error", harness.backend());
        });
    }

    /// # PullRequestSelectionState Default Implementation
    ///
    /// Tests the Default trait implementation.
    ///
    /// ## Test Scenario
    /// - Creates PullRequestSelectionState using Default::default()
    ///
    /// ## Expected Outcome
    /// - Should match PullRequestSelectionState::new()
    #[test]
    fn test_pr_selection_default() {
        let state = PullRequestSelectionState::default();
        assert!(!state.search_mode);
        assert!(!state.multi_select_mode);
        assert!(!state.search_iteration_mode);
        assert!(state.search_input.is_empty());
    }

    /// # PR Selection - Search Iteration Mode Navigation
    ///
    /// Tests navigation in search iteration mode (after search is complete).
    ///
    /// ## Test Scenario
    /// - Sets up search iteration mode with results
    /// - Tests 'n' for next and 'N' for previous
    ///
    /// ## Expected Outcome
    /// - Should navigate through search results
    #[tokio::test]
    async fn test_pr_selection_search_iteration_navigation() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.search_iteration_mode = true;
        state.search_results = vec![0, 1, 2];
        state.current_search_index = 0;

        // Navigate next
        ModeState::process_key(&mut state, KeyCode::Char('n'), harness.merge_app_mut()).await;
        assert_eq!(state.current_search_index, 1);

        // Navigate previous
        ModeState::process_key(&mut state, KeyCode::Char('N'), harness.merge_app_mut()).await;
        assert_eq!(state.current_search_index, 0);
    }

    /// # PR Selection - Search Iteration Escape
    ///
    /// Tests exiting search iteration mode with Escape.
    ///
    /// ## Test Scenario
    /// - Sets up search iteration mode
    /// - Presses Escape
    ///
    /// ## Expected Outcome
    /// - Should exit search iteration mode
    #[tokio::test]
    async fn test_pr_selection_search_iteration_escape() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.search_iteration_mode = true;
        state.search_results = vec![0];

        ModeState::process_key(&mut state, KeyCode::Esc, harness.merge_app_mut()).await;
        assert!(!state.search_iteration_mode);
    }

    /// # PR Selection - Multi-Select Select All States
    ///
    /// Tests 'a' key to select all states in multi-select mode.
    ///
    /// ## Test Scenario
    /// - Enters multi-select mode
    /// - Presses 'a' to select all
    ///
    /// ## Expected Outcome
    /// - All states should be selected
    #[tokio::test]
    async fn test_pr_selection_multi_select_all() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.multi_select_mode = true;
        state.available_states = crate::ui::testing::create_test_work_item_states();

        ModeState::process_key(&mut state, KeyCode::Char('a'), harness.merge_app_mut()).await;
        assert_eq!(
            state.selected_filter_states.len(),
            state.available_states.len()
        );
    }

    /// # PR Selection - Multi-Select Clear
    ///
    /// Tests 'c' key to clear all selections in multi-select mode.
    ///
    /// ## Test Scenario
    /// - Enters multi-select mode with selections
    /// - Presses 'c' to clear
    ///
    /// ## Expected Outcome
    /// - Should clear selections and exit multi-select mode
    #[tokio::test]
    async fn test_pr_selection_multi_select_clear() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let mut prs = create_test_pull_requests();
        prs[0].selected = true;
        *harness.app.pull_requests_mut() = prs;

        let mut state = PullRequestSelectionState::new();
        state.multi_select_mode = true;

        ModeState::process_key(&mut state, KeyCode::Char('c'), harness.merge_app_mut()).await;
        assert!(!state.multi_select_mode);
        assert!(!harness.app.pull_requests()[0].selected);
    }

    /// # PR Selection State - Details Hidden
    ///
    /// Tests the PR selection screen with details pane hidden.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state with show_details = false
    /// - Loads test pull requests
    /// - Renders the display
    ///
    /// ## Expected Outcome
    /// - Should display PR table taking full height
    /// - Should NOT show work item details panel
    /// - Help text should still include 'd: Details' hotkey
    #[test]
    fn test_pr_selection_details_hidden() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let mut selection_state = PullRequestSelectionState::new();
            selection_state.show_details = false;
            let mut state = MergeState::PullRequestSelection(selection_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("details_hidden", harness.backend());
        });
    }

    /// # PR Selection State - Dependency Dialog
    ///
    /// Tests the PR selection screen with dependency dialog open.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state with dependency dialog open
    /// - Loads test pull requests
    /// - Renders the display
    ///
    /// ## Expected Outcome
    /// - Should display dependency dialog overlay
    /// - Help text should show "Esc/g/q to close" (not "d")
    #[test]
    fn test_pr_selection_dependency_dialog() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            *harness.app.pull_requests_mut() = create_test_pull_requests();

            let mut selection_state = PullRequestSelectionState::new();
            selection_state.show_dependency_dialog = true;
            selection_state.dependency_dialog_pr_index = Some(0);
            let mut state = MergeState::PullRequestSelection(selection_state);
            harness.render_merge_state(&mut state);

            assert_snapshot!("dependency_dialog", harness.backend());
        });
    }

    /// # PR Selection - Toggle Details with 'd' Key
    ///
    /// Tests that pressing 'd' toggles the details pane visibility.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state (details visible by default)
    /// - Presses 'd' key
    /// - Verifies show_details is toggled
    ///
    /// ## Expected Outcome
    /// - show_details should toggle from true to false
    /// - Pressing again should toggle back to true
    #[tokio::test]
    async fn test_pr_selection_toggle_details() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        assert!(state.show_details); // Default is true

        // Press 'd' to hide details
        ModeState::process_key(&mut state, KeyCode::Char('d'), harness.merge_app_mut()).await;
        assert!(!state.show_details);

        // Press 'd' again to show details
        ModeState::process_key(&mut state, KeyCode::Char('d'), harness.merge_app_mut()).await;
        assert!(state.show_details);
    }

    /// # PR Selection - Open Dependency Dialog with 'g' Key
    ///
    /// Tests that pressing 'g' opens the dependency dialog.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state
    /// - Loads test pull requests
    /// - Presses 'g' key
    /// - Verifies dialog is opened
    ///
    /// ## Expected Outcome
    /// - show_dependency_dialog should be true
    /// - dependency_dialog_pr_index should be set to selected row
    #[tokio::test]
    async fn test_pr_selection_open_dependency_dialog() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.table_state.select(Some(0)); // Select first row
        assert!(!state.show_dependency_dialog);

        // Press 'g' to open dependency dialog
        ModeState::process_key(&mut state, KeyCode::Char('g'), harness.merge_app_mut()).await;
        assert!(state.show_dependency_dialog);
        assert_eq!(state.dependency_dialog_pr_index, Some(0));
    }

    /// # PR Selection - Close Dependency Dialog with 'g' Key
    ///
    /// Tests that pressing 'g' closes the dependency dialog when open.
    ///
    /// ## Test Scenario
    /// - Creates a PR selection state with dialog open
    /// - Presses 'g' key
    /// - Verifies dialog is closed
    ///
    /// ## Expected Outcome
    /// - show_dependency_dialog should be false
    /// - dependency_dialog_pr_index should be None
    #[tokio::test]
    async fn test_pr_selection_close_dependency_dialog() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        *harness.app.pull_requests_mut() = create_test_pull_requests();

        let mut state = PullRequestSelectionState::new();
        state.show_dependency_dialog = true;
        state.dependency_dialog_pr_index = Some(0);

        // Press 'g' to close dependency dialog
        ModeState::process_key(&mut state, KeyCode::Char('g'), harness.merge_app_mut()).await;
        assert!(!state.show_dependency_dialog);
        assert_eq!(state.dependency_dialog_pr_index, None);
    }
}
