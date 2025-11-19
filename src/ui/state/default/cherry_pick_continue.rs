use crate::{
    models::CherryPickStatus,
    ui::App,
    ui::state::{AppState, CherryPickState, ConflictResolutionState, StateChange},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::{
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};

pub struct CherryPickContinueState {
    output: Arc<Mutex<Vec<String>>>,
    is_complete: Arc<Mutex<bool>>,
    success: Arc<Mutex<Option<bool>>>,
    error_message: Arc<Mutex<Option<String>>>,
    conflicted_files: Vec<String>,
}

impl CherryPickContinueState {
    pub fn new(conflicted_files: Vec<String>, repo_path: std::path::PathBuf) -> Self {
        let output = Arc::new(Mutex::new(Vec::new()));
        let is_complete = Arc::new(Mutex::new(false));
        let success = Arc::new(Mutex::new(None));
        let error_message = Arc::new(Mutex::new(None));

        let output_clone = output.clone();
        let is_complete_clone = is_complete.clone();
        let success_clone = success.clone();
        let error_message_clone = error_message.clone();

        // Spawn a thread to run the git cherry-pick --continue command
        thread::spawn(move || {
            let mut child = match Command::new("git")
                .current_dir(&repo_path)
                .args(["cherry-pick", "--continue"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(e) => {
                    let mut output = output_clone.lock().unwrap();
                    output.push(format!("Failed to spawn git command: {}", e));
                    *is_complete_clone.lock().unwrap() = true;
                    *success_clone.lock().unwrap() = Some(false);
                    *error_message_clone.lock().unwrap() =
                        Some(format!("Failed to spawn git command: {}", e));
                    return;
                }
            };

            // Read stdout
            if let Some(stdout) = child.stdout.take() {
                use std::io::{BufRead, BufReader};
                let output_clone = output_clone.clone();
                thread::spawn(move || {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines().map_while(Result::ok) {
                        let mut output = output_clone.lock().unwrap();
                        output.push(line);
                    }
                });
            }

            // Read stderr
            if let Some(stderr) = child.stderr.take() {
                use std::io::{BufRead, BufReader};
                let output_clone = output_clone.clone();
                thread::spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines().map_while(Result::ok) {
                        let mut output = output_clone.lock().unwrap();
                        output.push(line);
                    }
                });
            }

            // Wait for the command to complete
            match child.wait() {
                Ok(status) => {
                    *is_complete_clone.lock().unwrap() = true;
                    let is_success = status.success();
                    *success_clone.lock().unwrap() = Some(is_success);

                    if !is_success {
                        let output = output_clone.lock().unwrap();
                        let error_msg = if output.is_empty() {
                            "Cherry-pick --continue failed with no output".to_string()
                        } else {
                            output.join("\n")
                        };
                        *error_message_clone.lock().unwrap() = Some(error_msg);
                    }
                }
                Err(e) => {
                    *is_complete_clone.lock().unwrap() = true;
                    *success_clone.lock().unwrap() = Some(false);
                    *error_message_clone.lock().unwrap() = Some(format!("Failed to wait: {}", e));
                }
            }
        });

        Self {
            output,
            is_complete,
            success,
            error_message,
            conflicted_files,
        }
    }
}

#[async_trait]
impl AppState for CherryPickContinueState {
    fn ui(&mut self, f: &mut Frame, app: &App) {
        let is_complete = *self.is_complete.lock().unwrap();
        let success = *self.success.lock().unwrap();

        // Main layout: Title at top, content in middle, help at bottom
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Min(0),    // Content
                Constraint::Length(3), // Instructions
            ])
            .split(f.area());

        // Title
        let title_text = if is_complete {
            match success {
                Some(true) => "✅ Cherry-pick Completed Successfully",
                Some(false) => "❌ Cherry-pick Failed",
                None => "Processing Cherry-pick...",
            }
        } else {
            "⏳ Processing Cherry-pick Continue..."
        };

        let title_color = if is_complete {
            match success {
                Some(true) => Color::Green,
                Some(false) => Color::Red,
                None => Color::Yellow,
            }
        } else {
            Color::Yellow
        };

        let title = Paragraph::new(title_text)
            .style(
                Style::default()
                    .fg(title_color)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, main_chunks[0]);

        // Split content horizontally: Left and Right panes
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(main_chunks[1]);

        // Left pane: Commit output
        let output = self.output.lock().unwrap();
        let output_text: Vec<Line> = output
            .iter()
            .map(|line| Line::from(line.as_str()))
            .collect();

        let output_widget = Paragraph::new(output_text)
            .block(Block::default().borders(Borders::ALL).title("Git Output"))
            .wrap(Wrap { trim: false })
            .scroll((
                output.len().saturating_sub(main_chunks[1].height as usize) as u16,
                0,
            ));
        f.render_widget(output_widget, content_chunks[0]);

        // Right pane: Commit and PR details
        let mut details_text = vec![];

        if app.current_cherry_pick_index < app.cherry_pick_items.len() {
            let current_item = &app.cherry_pick_items[app.current_cherry_pick_index];

            // Shortened commit hash
            let short_hash = if current_item.commit_id.len() >= 8 {
                &current_item.commit_id[..8]
            } else {
                &current_item.commit_id
            };

            details_text.push(Line::from(vec![
                Span::raw("Hash: "),
                Span::styled(short_hash, Style::default().fg(Color::Yellow)),
            ]));

            details_text.push(Line::from(vec![
                Span::raw("PR #"),
                Span::styled(
                    format!("{}", current_item.pr_id),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));

            details_text.push(Line::from(""));
            details_text.push(Line::from(vec![
                Span::raw("Title: "),
                Span::raw(&current_item.pr_title),
            ]));

            if !self.conflicted_files.is_empty() {
                details_text.push(Line::from(""));
                details_text.push(Line::from(""));
                details_text.push(Line::from(vec![Span::styled(
                    "Previously conflicted files:",
                    Style::default().fg(Color::Gray),
                )]));
                for file in &self.conflicted_files {
                    details_text.push(Line::from(format!("  • {}", file)));
                }
            }
        }

        let details_widget = Paragraph::new(details_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Commit Details"),
            )
            .wrap(Wrap { trim: true });
        f.render_widget(details_widget, content_chunks[1]);

        // Bottom: Instructions
        let instructions = if is_complete {
            match success {
                Some(true) => "Press any key to continue to next commit",
                Some(false) => "r: Retry | a: Abort cherry-pick process",
                None => "Press any key to continue",
            }
        } else {
            "Processing... Please wait (pre-commit hooks may take time)"
        };

        let instructions_widget = Paragraph::new(instructions)
            .block(Block::default().borders(Borders::ALL).title("Instructions"))
            .style(Style::default().fg(Color::White));
        f.render_widget(instructions_widget, main_chunks[2]);
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        let is_complete = *self.is_complete.lock().unwrap();

        // Don't process keys until the command is complete
        if !is_complete {
            return StateChange::Keep;
        }

        let success = *self.success.lock().unwrap();

        match success {
            Some(true) => {
                // Success - mark as successful and continue to next commit
                app.cherry_pick_items[app.current_cherry_pick_index].status =
                    CherryPickStatus::Success;
                app.current_cherry_pick_index += 1;
                StateChange::Change(Box::new(CherryPickState::continue_after_conflict()))
            }
            Some(false) => {
                // Failed - allow retry or abort
                match code {
                    KeyCode::Char('r') => {
                        // Retry - go back to conflict resolution
                        StateChange::Change(Box::new(ConflictResolutionState::new(
                            self.conflicted_files.clone(),
                        )))
                    }
                    KeyCode::Char('a') => {
                        // Abort - mark as failed and continue
                        let error_msg = self
                            .error_message
                            .lock()
                            .unwrap()
                            .clone()
                            .unwrap_or_else(|| "Unknown error".to_string());
                        app.cherry_pick_items[app.current_cherry_pick_index].status =
                            CherryPickStatus::Failed(error_msg);
                        app.current_cherry_pick_index += 1;
                        StateChange::Change(Box::new(CherryPickState::continue_after_conflict()))
                    }
                    _ => StateChange::Keep,
                }
            }
            None => {
                // Should not happen, but handle it
                StateChange::Keep
            }
        }
    }
}
