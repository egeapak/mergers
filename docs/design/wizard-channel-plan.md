# Channel-Based Wizard Step Execution Plan

## Overview

This plan details refactoring the repository setup wizard from tick-based step execution to a channel-based background task approach. This provides cleaner separation between execution logic and UI updates, with more natural async flow.

## Problem Statement

### Current Implementation (Tick-Based)

The current implementation executes one step per event loop tick:

```rust
// typed_run.rs event loop
loop {
    terminal.draw(|f| AppState::ui(&mut current_state, f, app))?;  // Render

    if event_source.poll(Duration::from_millis(50))? {
        // Process key
    } else {
        // KeyCode::Null - triggers next step execution
    }
}
```

**Issues:**
1. Steps execute only when `process_key` is called (every 50ms tick)
2. Step execution is interleaved with UI state management
3. `next_step` field represents "what to execute on next tick" - confusing naming
4. Coupling between execution logic and state machine transitions

### Proposed Solution (Channel-Based)

Spawn a background task that executes all steps sequentially, sending progress updates through a channel. The UI polls for updates and applies them.

```
┌─────────────────┐     mpsc::channel     ┌─────────────────┐
│ Background Task │ ──────────────────▶   │   UI State      │
│                 │   ProgressMessage     │                 │
│ - Execute steps │                       │ - Poll channel  │
│ - Send updates  │                       │ - Update display│
└─────────────────┘                       └─────────────────┘
```

## Architecture

### Message Types

```rust
/// Messages sent from background task to UI
pub enum ProgressMessage {
    /// A step has started executing
    StepStarted(WizardStep),

    /// A step completed successfully with optional result data
    StepCompleted {
        step: WizardStep,
        result: StepResult,
    },

    /// All steps completed successfully
    AllComplete,

    /// An error occurred during step execution
    Error {
        step: WizardStep,
        error: SetupError,
    },
}

/// Result data that needs to be applied to MergeApp after step completion
pub enum StepResult {
    /// SSH URL fetched from Azure DevOps
    FetchDetails { ssh_url: String },

    /// Repository cloned to temporary directory
    CloneComplete {
        path: PathBuf,
        temp_dir: TempDir,
    },

    /// Worktree created
    WorktreeComplete {
        path: PathBuf,
        base_path: PathBuf,
    },

    /// Patch branch created
    BranchCreated { branch_name: String },

    /// Cherry-pick items prepared
    CherryPicksPrepared { items: Vec<CherryPickItem> },

    /// Step completed with no data to apply
    None,
}

/// Errors that can occur during setup
pub enum SetupError {
    /// Specific setup errors that support force-resolution
    Setup(git::RepositorySetupError),

    /// General errors that require retry
    General(String),
}
```

### Context Extraction

Since `&mut MergeApp` cannot be shared across threads, we extract necessary data before spawning:

```rust
/// Data extracted from MergeApp for background task execution
pub struct SetupContext {
    /// Whether to clone (true) or create worktree (false)
    pub is_clone_mode: bool,

    /// Local repository path (worktree mode only)
    pub local_repo: Option<PathBuf>,

    /// Target branch name
    pub target_branch: String,

    /// Version string for branch naming
    pub version: String,

    /// Whether to run git hooks
    pub run_hooks: bool,

    /// Selected PRs data for cherry-pick preparation
    pub selected_prs: Vec<SelectedPrData>,
}

/// Minimal PR data needed for cherry-pick preparation
pub struct SelectedPrData {
    pub pr_id: i32,
    pub pr_title: String,
    pub commit_id: Option<String>,
}

impl SetupContext {
    pub fn from_app(app: &MergeApp) -> Self {
        Self {
            is_clone_mode: app.local_repo().is_none(),
            local_repo: app.local_repo().map(PathBuf::from),
            target_branch: app.target_branch().to_string(),
            version: app.version().as_ref().unwrap().to_string(),
            run_hooks: app.run_hooks(),
            selected_prs: app.get_selected_prs()
                .iter()
                .map(|pr| SelectedPrData {
                    pr_id: pr.pr.id,
                    pr_title: pr.pr.title.clone(),
                    commit_id: pr.pr.last_merge_commit
                        .as_ref()
                        .map(|c| c.commit_id.clone()),
                })
                .collect(),
        }
    }
}
```

### State Structure

```rust
pub struct SetupRepoState {
    /// Current setup state
    state: SetupState,

    /// Cached mode detection
    is_clone_mode: Option<bool>,
}

pub enum SetupState {
    /// Initial state, waiting to start
    Idle,

    /// Background task is running
    Running {
        /// Progress tracker for UI display
        progress: WizardProgress,

        /// Channel receiver for progress updates
        progress_rx: mpsc::Receiver<ProgressMessage>,

        /// Task handle for cleanup/abort
        task_handle: JoinHandle<()>,

        /// Pending results to apply to app
        pending_results: VecDeque<StepResult>,
    },

    /// All steps completed successfully
    Complete,

    /// Error occurred, showing error UI
    Error {
        error: SetupError,
        message: String,
        progress: Option<WizardProgress>,
    },
}
```

### Background Task

```rust
async fn run_setup_task(
    tx: mpsc::Sender<ProgressMessage>,
    context: SetupContext,
    client: AzureDevOpsClient,
) {
    // Helper macro for sending messages
    macro_rules! send {
        ($msg:expr) => {
            if tx.send($msg).await.is_err() {
                return; // Receiver dropped, abort
            }
        };
    }

    // Step 1: FetchDetails (clone mode only)
    if context.is_clone_mode {
        send!(ProgressMessage::StepStarted(WizardStep::FetchDetails));

        match client.fetch_repo_details().await {
            Ok(details) => {
                send!(ProgressMessage::StepCompleted {
                    step: WizardStep::FetchDetails,
                    result: StepResult::FetchDetails {
                        ssh_url: details.ssh_url
                    },
                });
            }
            Err(e) => {
                send!(ProgressMessage::Error {
                    step: WizardStep::FetchDetails,
                    error: SetupError::General(e.to_string()),
                });
                return;
            }
        }
    }

    // Step 2: CheckPrerequisites
    send!(ProgressMessage::StepStarted(WizardStep::CheckPrerequisites));

    if let Err(e) = validate_prerequisites(&context).await {
        send!(ProgressMessage::Error {
            step: WizardStep::CheckPrerequisites,
            error: e,
        });
        return;
    }

    send!(ProgressMessage::StepCompleted {
        step: WizardStep::CheckPrerequisites,
        result: StepResult::None,
    });

    // Step 3: FetchTargetBranch (worktree mode only)
    // ... similar pattern ...

    // Step 4: CloneOrWorktree
    // ... similar pattern ...

    // Step 5: CreateBranch
    // ... similar pattern ...

    // Step 6: PrepareCherryPicks
    // ... similar pattern ...

    // Step 7: InitializeState
    // Note: State file creation happens in UI after results applied
    send!(ProgressMessage::StepStarted(WizardStep::InitializeState));
    send!(ProgressMessage::StepCompleted {
        step: WizardStep::InitializeState,
        result: StepResult::None,
    });

    // All done
    send!(ProgressMessage::AllComplete);
}
```

### UI Event Handling

```rust
impl ModeState for SetupRepoState {
    async fn process_key(
        &mut self,
        code: KeyCode,
        app: &mut MergeApp
    ) -> StateChange<MergeState> {
        match &mut self.state {
            SetupState::Idle => {
                // Start background task on first call
                let context = SetupContext::from_app(app);
                let client = app.client().clone();
                let (tx, rx) = mpsc::channel(32);

                let is_clone_mode = context.is_clone_mode;
                let handle = tokio::spawn(run_setup_task(tx, context, client));

                self.is_clone_mode = Some(is_clone_mode);
                self.state = SetupState::Running {
                    progress: WizardProgress::new(is_clone_mode),
                    progress_rx: rx,
                    task_handle: handle,
                    pending_results: VecDeque::new(),
                };

                StateChange::Keep
            }

            SetupState::Running {
                progress,
                progress_rx,
                pending_results,
                ..
            } => {
                // Poll for progress updates (non-blocking)
                while let Ok(msg) = progress_rx.try_recv() {
                    match msg {
                        ProgressMessage::StepStarted(step) => {
                            progress.start_step(step);
                        }
                        ProgressMessage::StepCompleted { step, result } => {
                            progress.complete_step(step);
                            if !matches!(result, StepResult::None) {
                                pending_results.push_back(result);
                            }
                        }
                        ProgressMessage::AllComplete => {
                            // Apply all pending results
                            while let Some(result) = pending_results.pop_front() {
                                self.apply_result(result, app);
                            }

                            // Create state file (needs app access)
                            self.create_state_file(app);

                            return StateChange::Change(
                                MergeState::CherryPick(CherryPickState::new())
                            );
                        }
                        ProgressMessage::Error { step, error } => {
                            self.set_error(step, error, progress.clone());
                            return StateChange::Keep;
                        }
                    }
                }

                // Apply any pending results
                while let Some(result) = pending_results.pop_front() {
                    self.apply_result(result, app);
                }

                StateChange::Keep
            }

            SetupState::Complete => {
                StateChange::Change(MergeState::CherryPick(CherryPickState::new()))
            }

            SetupState::Error { error, .. } => {
                match code {
                    KeyCode::Char('r' | 'R') => {
                        // Reset to idle for retry
                        self.state = SetupState::Idle;
                        StateChange::Keep
                    }
                    KeyCode::Char('f' | 'F') => {
                        // Force resolve and retry
                        self.force_resolve_error(app, error.clone()).await
                    }
                    KeyCode::Esc => {
                        StateChange::Change(MergeState::Error(ErrorState::new()))
                    }
                    _ => StateChange::Keep,
                }
            }
        }
    }
}
```

### Result Application

```rust
impl SetupRepoState {
    fn apply_result(&self, result: StepResult, app: &mut MergeApp) {
        match result {
            StepResult::FetchDetails { ssh_url } => {
                // Store for later use in clone step
                // Note: We need a place to store this - add to Running state
            }
            StepResult::CloneComplete { path, temp_dir } => {
                app.set_repo_path(Some(path));
                app.worktree.set_temp_dir(Some(temp_dir));
            }
            StepResult::WorktreeComplete { path, base_path } => {
                app.worktree.base_repo_path = Some(base_path);
                app.set_repo_path(Some(path));
            }
            StepResult::BranchCreated { branch_name: _ } => {
                // Branch name stored for reference, no app update needed
            }
            StepResult::CherryPicksPrepared { items } => {
                *app.cherry_pick_items_mut() = items;
            }
            StepResult::None => {}
        }
    }

    fn create_state_file(&self, app: &mut MergeApp) {
        if let Some(repo_path) = app.repo_path() {
            let version = app.version().as_ref().unwrap().to_string();
            let base_repo_path = app.worktree.base_repo_path.clone();
            let is_worktree = base_repo_path.is_some();

            if app.create_state_file(
                repo_path.to_path_buf(),
                base_repo_path,
                is_worktree,
                &version,
            ).is_ok() {
                let _ = app.update_state_phase(MergePhase::CherryPicking);
            }
        }
    }
}
```

## File Changes Summary

| File | Changes |
|------|---------|
| `src/ui/state/default/setup_repo.rs` | Complete rewrite with channel-based architecture |
| `src/ui/state/default/mod.rs` | No changes needed |
| `Cargo.toml` | Already has `tokio` with `sync` feature |

## Implementation Phases

### Phase 1: Message Types and Context Extraction
- Define `ProgressMessage`, `StepResult`, `SetupError` enums
- Implement `SetupContext` and `SelectedPrData` structs
- Add `SetupContext::from_app()` method

### Phase 2: State Restructuring
- Refactor `SetupState` enum to new structure
- Update `SetupRepoState` fields
- Implement state transition helpers

### Phase 3: Background Task Implementation
- Implement `run_setup_task()` async function
- Port each step from current `execute_step()` to task
- Handle errors with proper message sending

### Phase 4: UI Integration
- Update `process_key()` for new state structure
- Implement channel polling logic
- Implement `apply_result()` method
- Handle task spawning and cleanup

### Phase 5: Error Handling
- Implement error state transitions
- Port `force_resolve_error()` logic
- Add task abort on retry/force

### Phase 6: Testing
- Update existing snapshot tests
- Add unit tests for message handling
- Add integration tests for full flow

## Trade-offs Analysis

### Advantages

1. **Clean Separation**: Execution logic isolated in background task
2. **Natural Async Flow**: Steps execute sequentially without manual state tracking
3. **Better Cancellation**: Task can be aborted on user action
4. **Extensibility**: Easy to add timeouts, progress bars, etc.

### Disadvantages

1. **Complexity**: More types and indirection
2. **Data Extraction**: Must extract all needed data upfront
3. **Result Serialization**: Results passed through channel, applied later
4. **Error Handling**: Must handle task panics and channel disconnects

### Comparison

| Aspect | Tick-Based (Current) | Channel-Based (Proposed) |
|--------|---------------------|--------------------------|
| Step execution | One per `process_key` | All in background task |
| Progress tracking | `next_step` field | Messages through channel |
| Flow control | State machine per tick | Task runs, UI observes |
| Coupling | Steps in UI logic | Clean separation |
| Cancellation | Set `next_step = None` | Abort task handle |
| Error recovery | Reset and retry | Reset, abort task, retry |

## Design Decisions

### Why mpsc over watch/broadcast?
- Multiple distinct messages (steps, errors, completion)
- Order matters
- Don't need to replay missed messages

### Why not oneshot for each step?
- Would require spawning multiple tasks or complex continuation
- Single task with channel is simpler

### Why extract context vs Arc<Mutex<App>>?
- Avoids contention and deadlocks
- Clearer data flow
- MergeApp has complex interior state

### Channel buffer size?
- 32 messages is sufficient
- Steps don't produce faster than UI can consume
- Backpressure not a concern

## Testing Strategy

### Unit Tests
- `ProgressMessage` serialization (if needed)
- `SetupContext::from_app()` correctness
- `apply_result()` for each `StepResult` variant

### Integration Tests
- Full flow with mock API client
- Error scenarios (network failure, git errors)
- Retry and force-resolve flows

### Snapshot Tests
- Each step in progress
- Error states with progress preserved
- Completion state

## Rollback Plan

If issues arise, the tick-based implementation can be restored:

1. Revert `setup_repo.rs` to previous version
2. No other files affected
3. Snapshots will need updating

The current tick-based implementation is preserved in git history for reference.
