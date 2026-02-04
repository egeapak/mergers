# Migration Plan: Data Loading to Wizard Pattern

## Overview

This document outlines the comprehensive migration of `data_loading.rs` from a simple progress-bar-based loading screen to a channel-based wizard pattern matching `setup_repo.rs`. The goal is to create structurally similar implementations that can be merged into a generic wizard framework in the future.

---

## Current State Analysis

### `data_loading.rs` (Current)
- **LoadingStage enum**: 6 stages (NotStarted, FetchingPullRequests, FetchingWorkItems, WaitingForWorkItems, FetchingCommitInfo, AnalyzingDependencies)
- **Progress display**: Percentage-based Gauge widget (0-100%)
- **Execution model**: Direct async calls in `process_key` method
- **Error handling**: Transitions to generic ErrorState
- **UI Layout**: Title → Progress Bar → Status Message → Help

### `setup_repo.rs` (Target Pattern)
- **WizardStep enum**: 8 steps with `display_name()` and `progress_message()` methods
- **Progress display**: Step indicator row with symbols (✓ ● ○ −)
- **Execution model**: Background task via tokio channel (mpsc)
- **Error handling**: Inline errors with recovery options ('r' retry, 'f' force, Esc back)
- **UI Layout**: Title → Step Indicator → Current Step Progress

---

## Migration Goals

1. **Structural Parity**: Match `setup_repo.rs` component structure exactly
2. **Channel-Based Execution**: Decouple UI from data fetching
3. **Granular Progress**: Visual step indicators instead of percentage bar
4. **Error Recovery**: Inline error handling with retry options
5. **Future Mergeability**: Enable extraction to generic `Wizard<S>` framework

---

## Component Mapping

| setup_repo.rs Component | data_loading.rs Equivalent |
|------------------------|---------------------------|
| `WizardStep` enum | `LoadingStep` enum |
| `StepStatus` enum | `StepStatus` enum (reuse) |
| `WizardProgress` struct | `LoadingProgress` struct |
| `SetupState` enum | `LoadingState` enum |
| `SetupError` enum | `LoadingError` enum |
| `SetupContext` struct | `LoadingContext` struct |
| `StepResult` struct | `LoadingStepResult` struct |
| `StepData` struct | `LoadingStepData` struct |
| `ProgressMessage` enum | `LoadingProgressMessage` enum |
| `SetupRepoState` struct | `DataLoadingState` struct (refactor) |
| `run_setup_task()` fn | `run_loading_task()` fn |

---

## Phase 1: Foundation Types

### 1.1 Define `LoadingStep` Enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadingStep {
    /// Fetch pull requests from Azure DevOps API
    FetchPullRequests,
    /// Fetch work items for each PR (parallel)
    FetchWorkItems,
    /// Fetch commit information for PRs missing it
    FetchCommitInfo,
    /// Analyze file dependencies using local repository (optional)
    AnalyzeDependencies,
}
```

**Methods to implement:**
- `display_name(&self) -> &'static str`
- `progress_message(&self, fetched: usize, total: usize) -> String`

### 1.2 Define `LoadingError` Enum

```rust
#[derive(Debug, Clone)]
pub enum LoadingError {
    /// No pull requests found matching criteria
    NoPullRequestsFound,
    /// API request failed (retryable)
    ApiError(String),
    /// Network timeout (retryable)
    NetworkTimeout(String),
    /// Local repository not found (for dependency analysis)
    LocalRepoNotFound(String),
    /// Generic error
    Other(String),
}
```

**Recovery options by error type:**
- `NoPullRequestsFound`: Esc only (non-recoverable, config issue)
- `ApiError`: 'r' retry, Esc back
- `NetworkTimeout`: 'r' retry, Esc back
- `LocalRepoNotFound`: 's' skip (dependency analysis is optional), Esc back
- `Other`: 'r' retry, Esc back

### 1.3 Define `LoadingProgressMessage` Enum

```rust
#[derive(Debug, Clone)]
pub enum LoadingProgressMessage {
    /// A step has started
    StepStarted(LoadingStep),
    /// A step completed successfully
    StepCompleted(LoadingStep, LoadingStepResult),
    /// Progress update within a step (for long-running parallel operations)
    StepProgress(LoadingStep, usize, usize), // step, completed, total
    /// All steps completed
    AllComplete,
    /// An error occurred
    Error(LoadingError),
}
```

### 1.4 Define `LoadingStepResult` Struct

```rust
#[derive(Debug, Clone, Default)]
pub struct LoadingStepResult {
    /// Pull requests fetched (FetchPullRequests step)
    pub pull_requests: Option<Vec<PullRequestWithWorkItems>>,
    /// Work items results (FetchWorkItems step) - maps PR index to work items
    pub work_items_updates: Option<Vec<WorkItemsResult>>,
    /// Commit info fetched count (FetchCommitInfo step)
    pub commits_fetched: Option<usize>,
    /// Dependency graph (AnalyzeDependencies step)
    pub dependency_graph: Option<PRDependencyGraph>,
}
```

### 1.5 Define `LoadingStepData` Struct

```rust
#[derive(Debug, Clone, Default)]
pub struct LoadingStepData {
    /// Total PRs fetched
    pub total_prs: usize,
    /// Work items fetch progress
    pub work_items_fetched: usize,
    pub work_items_total: usize,
    /// Commit info fetch progress
    pub commits_fetched: usize,
    pub commits_total: usize,
    /// Dependency analysis done
    pub dependencies_analyzed: bool,
}
```

---

## Phase 2: Progress Tracking

### 2.1 Reuse `StepStatus` Enum

Import from a shared location or define identically:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Skipped,
}
```

### 2.2 Define `LoadingProgress` Struct

```rust
#[derive(Debug, Clone)]
pub struct LoadingProgress {
    /// Status of each step
    fetch_pull_requests: StepStatus,
    fetch_work_items: StepStatus,
    fetch_commit_info: StepStatus,
    analyze_dependencies: StepStatus,

    /// Current step being executed
    current_step: Option<LoadingStep>,

    /// Progress counters for parallel operations
    work_items_fetched: usize,
    work_items_total: usize,
    commits_fetched: usize,
    commits_total: usize,

    /// Whether dependency analysis is available (local repo exists)
    dependency_analysis_available: bool,
}
```

**Methods to implement:**
- `new(dependency_analysis_available: bool) -> Self`
- `steps(&self) -> Vec<(LoadingStep, StepStatus)>`
- `start_step(&mut self, step: LoadingStep)`
- `complete_step(&mut self, step: LoadingStep)`
- `update_progress(&mut self, step: LoadingStep, fetched: usize, total: usize)`
- `current_message(&self) -> String`
- `skip_step(&mut self, step: LoadingStep)`

---

## Phase 3: State Machine

### 3.1 Define `LoadingState` Enum

```rust
#[derive(Debug)]
pub enum LoadingState {
    /// Initial state before starting
    Initializing,

    /// Background task is running
    Running {
        progress: LoadingProgress,
        step_data: LoadingStepData,
    },

    /// All steps completed successfully
    Complete {
        step_data: LoadingStepData,
    },

    /// An error occurred
    Error {
        error: LoadingError,
        message: String,
        /// Progress at time of error (to show which step failed)
        progress: Option<LoadingProgress>,
        /// Whether error is recoverable via retry
        can_retry: bool,
        /// Whether error can be skipped (optional step)
        can_skip: bool,
    },
}
```

### 3.2 Refactor `DataLoadingState` Struct

```rust
pub struct DataLoadingState {
    /// Internal state machine
    state: LoadingState,

    /// Channel receiver for progress messages
    receiver: Option<LoadingProgressReceiver>,

    /// Cached: whether local repo is available for dependency analysis
    has_local_repo: Option<bool>,
}
```

---

## Phase 4: Context Extraction

### 4.1 Define `LoadingContext` Struct

```rust
pub struct LoadingContext {
    /// API client for Azure DevOps
    pub client: AzureDevOpsClient,

    /// Development branch to fetch PRs from
    pub dev_branch: String,

    /// Date filter for PRs
    pub since: Option<String>,

    /// Local repository path (for dependency analysis)
    pub local_repo: Option<String>,

    /// Network throttling limits
    pub max_concurrent_network: usize,
    pub max_concurrent_processing: usize,
}

impl LoadingContext {
    pub fn from_app(app: &MergeApp) -> Self {
        Self {
            client: app.client().clone(),
            dev_branch: app.dev_branch().to_string(),
            since: app.since().map(String::from),
            local_repo: app.local_repo().map(String::from),
            max_concurrent_network: app.max_concurrent_network(),
            max_concurrent_processing: app.max_concurrent_processing(),
        }
    }
}
```

---

## Phase 5: Background Task

### 5.1 Implement `run_loading_task`

```rust
async fn run_loading_task(
    ctx: LoadingContext,
    tx: mpsc::Sender<LoadingProgressMessage>,
) {
    // Step 1: Fetch Pull Requests
    send_or_return!(tx, LoadingProgressMessage::StepStarted(LoadingStep::FetchPullRequests));

    let prs = match fetch_pull_requests_impl(&ctx).await {
        Ok(prs) => prs,
        Err(e) => {
            let _ = tx.send(LoadingProgressMessage::Error(e)).await;
            return;
        }
    };

    let pr_count = prs.len();
    send_or_return!(tx, LoadingProgressMessage::StepCompleted(
        LoadingStep::FetchPullRequests,
        LoadingStepResult { pull_requests: Some(prs), ..Default::default() }
    ));

    // Step 2: Fetch Work Items (parallel with progress updates)
    send_or_return!(tx, LoadingProgressMessage::StepStarted(LoadingStep::FetchWorkItems));
    // ... implementation with StepProgress messages ...

    // Step 3: Fetch Commit Info
    // ...

    // Step 4: Analyze Dependencies (if local repo available)
    // ...

    let _ = tx.send(LoadingProgressMessage::AllComplete).await;
}
```

### 5.2 Implement Step Functions

```rust
async fn fetch_pull_requests_impl(ctx: &LoadingContext) -> Result<Vec<PullRequestWithWorkItems>, LoadingError>;
async fn fetch_work_items_impl(ctx: &LoadingContext, prs: &[PullRequestWithWorkItems], tx: &mpsc::Sender<LoadingProgressMessage>) -> Result<Vec<WorkItemsResult>, LoadingError>;
async fn fetch_commit_info_impl(ctx: &LoadingContext, prs: &mut [PullRequestWithWorkItems], tx: &mpsc::Sender<LoadingProgressMessage>) -> Result<usize, LoadingError>;
fn analyze_dependencies_impl(ctx: &LoadingContext, prs: &[PullRequestWithWorkItems]) -> Result<Option<PRDependencyGraph>, LoadingError>;
```

---

## Phase 6: UI Rendering

### 6.1 Update Layout

```rust
fn ui(&mut self, f: &mut Frame, app: &MergeApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Length(3), // Step indicator
            Constraint::Min(5),    // Current step progress
        ])
        .split(f.area());

    // Render based on state...
}
```

### 6.2 Implement `render_loading_step_indicator`

Matches `render_step_indicator` from setup_repo.rs:
- Shows all steps horizontally with arrows between
- Uses symbols: ✓ (green), ● (yellow), ○ (gray), − (gray)
- Includes step number and name

### 6.3 Implement `render_current_loading_progress`

Matches `render_current_step_progress` from setup_repo.rs:
- Shows current step message
- For parallel operations, shows "Fetching work items (5/10)..."
- "Please wait..." indicator

### 6.4 Implement Error Rendering

- Show step indicator with error state
- Display error message with styled hotkeys
- Show available options based on error type

---

## Phase 7: Message Handling

### 7.1 Implement `handle_progress_message`

```rust
fn handle_progress_message(&mut self, msg: LoadingProgressMessage, app: &mut MergeApp) {
    match msg {
        LoadingProgressMessage::StepStarted(step) => {
            self.start_step(step);
        }
        LoadingProgressMessage::StepCompleted(step, result) => {
            self.complete_step(step);
            self.apply_step_result(&result, app);
        }
        LoadingProgressMessage::StepProgress(step, fetched, total) => {
            self.update_progress(step, fetched, total);
        }
        LoadingProgressMessage::AllComplete => {
            self.transition_to_complete();
        }
        LoadingProgressMessage::Error(err) => {
            self.set_error(err);
        }
    }
}
```

### 7.2 Implement Key Handling

```rust
async fn process_key(&mut self, code: KeyCode, app: &mut MergeApp) -> StateChange<MergeState> {
    // 1. Poll channel for messages
    let messages = self.drain_messages();
    for msg in messages {
        self.handle_progress_message(msg, app);
    }

    // 2. Handle state-specific key presses
    match &self.state {
        LoadingState::Initializing => {
            if code == KeyCode::Null {
                self.start_background_task(app);
            }
        }
        LoadingState::Running { .. } => {
            // Only 'q' to quit during loading
        }
        LoadingState::Complete { .. } => {
            // Auto-transition to PR selection
        }
        LoadingState::Error { can_retry, can_skip, .. } => {
            match code {
                KeyCode::Char('r') if *can_retry => self.retry(app),
                KeyCode::Char('s') if *can_skip => self.skip_current_step(app),
                KeyCode::Esc => return StateChange::Exit,
                _ => {}
            }
        }
    }

    StateChange::Keep
}
```

---

## Phase 8: Testing Strategy

### 8.1 Unit Tests

#### Type Tests
- [ ] `LoadingStep::display_name()` returns correct names
- [ ] `LoadingStep::progress_message()` formats correctly with counts
- [ ] `LoadingError` variants have correct recovery options
- [ ] `LoadingProgress::new()` initializes correctly
- [ ] `LoadingProgress::steps()` returns correct steps (with/without dependency analysis)
- [ ] `LoadingProgress::start_step()` / `complete_step()` work correctly
- [ ] `LoadingProgress::update_progress()` updates counters
- [ ] `LoadingProgress::skip_step()` marks step as skipped
- [ ] `LoadingStepData::merge_result()` accumulates correctly
- [ ] `LoadingContext::from_app()` extracts all fields

#### State Machine Tests
- [ ] Initial state is `Initializing`
- [ ] First Null key starts background task
- [ ] Messages update state correctly
- [ ] 'q' key exits during loading
- [ ] 'r' key retries on retryable error
- [ ] 's' key skips on skippable error
- [ ] Esc key goes back on error
- [ ] Complete state triggers transition to PR selection

### 8.2 Snapshot Tests

#### Initializing State
- [ ] `test_loading_initializing` - Initial state before loading starts
- [ ] `test_loading_initializing_with_local_repo` - Shows dependency step available
- [ ] `test_loading_initializing_without_local_repo` - Dependency step skipped

#### Step Progress States
- [ ] `test_loading_step_fetch_prs_started` - First step in progress
- [ ] `test_loading_step_fetch_prs_completed` - First step done, second pending
- [ ] `test_loading_step_fetch_work_items_started` - Second step started
- [ ] `test_loading_step_fetch_work_items_progress_0` - 0/10 work items
- [ ] `test_loading_step_fetch_work_items_progress_50` - 5/10 work items
- [ ] `test_loading_step_fetch_work_items_progress_100` - 10/10 work items
- [ ] `test_loading_step_fetch_work_items_completed` - Second step done
- [ ] `test_loading_step_fetch_commits_started` - Third step started
- [ ] `test_loading_step_fetch_commits_progress` - 2/3 commits
- [ ] `test_loading_step_fetch_commits_completed` - Third step done
- [ ] `test_loading_step_analyze_deps_started` - Fourth step started
- [ ] `test_loading_step_analyze_deps_completed` - All steps done
- [ ] `test_loading_step_analyze_deps_skipped` - Fourth step skipped (no local repo)

#### Error States
- [ ] `test_loading_error_no_prs_found` - No PRs found error
- [ ] `test_loading_error_api_error_step1` - API error on fetch PRs
- [ ] `test_loading_error_api_error_step2` - API error on work items
- [ ] `test_loading_error_network_timeout` - Network timeout error
- [ ] `test_loading_error_local_repo_not_found` - Missing local repo (skippable)
- [ ] `test_loading_error_generic` - Generic error message

#### Complete State
- [ ] `test_loading_complete_all_steps` - All 4 steps completed
- [ ] `test_loading_complete_deps_skipped` - 3 steps completed, 1 skipped

---

## Phase 9: Implementation Order

### Step 1: Foundation Types (Phase 1)
1. Create `LoadingStep` enum with methods
2. Create `LoadingError` enum with recovery flags
3. Create `LoadingProgressMessage` enum
4. Create `LoadingStepResult` struct
5. Create `LoadingStepData` struct
6. **Verify**: Unit tests for all type methods

### Step 2: Progress Tracking (Phase 2)
1. Add/reuse `StepStatus` enum
2. Create `LoadingProgress` struct with all methods
3. **Verify**: Unit tests for progress tracking
4. **Verify**: Snapshot tests for step indicator rendering (isolated)

### Step 3: State Machine (Phase 3)
1. Create `LoadingState` enum
2. Refactor `DataLoadingState` struct
3. Add state transition methods
4. **Verify**: Unit tests for state transitions

### Step 4: Context & Background Task (Phases 4-5)
1. Create `LoadingContext` struct
2. Implement `run_loading_task` function
3. Implement step execution functions
4. **Verify**: Integration tests with mock API

### Step 5: UI Rendering (Phase 6)
1. Update layout constraints
2. Implement `render_loading_step_indicator`
3. Implement `render_current_loading_progress`
4. Implement error rendering
5. **Verify**: Full snapshot test suite

### Step 6: Message Handling (Phase 7)
1. Implement channel polling
2. Implement `handle_progress_message`
3. Implement key handling for all states
4. **Verify**: Async tests for message flow

### Step 7: Final Integration
1. Remove old `LoadingStage` enum
2. Update state machine in `process_key`
3. Full end-to-end testing
4. **Verify**: All snapshot tests pass
5. **Verify**: Manual testing in TUI

---

## Verification Checklist

### Code Quality
- [ ] All clippy warnings resolved
- [ ] Code formatted with `cargo fmt`
- [ ] No unused imports or dead code
- [ ] Documentation comments on public items

### Test Coverage
- [ ] Unit tests for all public methods
- [ ] Snapshot tests for all UI states
- [ ] Error path coverage
- [ ] Edge cases (empty PRs, 0 work items, etc.)

### Structural Parity with setup_repo.rs
- [ ] Same component naming pattern (Loading* vs Setup*)
- [ ] Same method signatures where applicable
- [ ] Same UI rendering patterns
- [ ] Same error handling patterns
- [ ] Same channel message patterns

### Manual Testing
- [ ] Normal flow: all steps complete
- [ ] Error recovery: retry on API error
- [ ] Skip optional: skip dependency analysis
- [ ] Cancel: 'q' during loading
- [ ] Back: Esc on error

---

## Future: Generic Wizard Framework

After both `data_loading.rs` and `setup_repo.rs` use the same pattern, we can extract:

```rust
pub trait WizardStep: Debug + Clone + Copy + Eq {
    fn display_name(&self) -> &'static str;
    fn progress_message(&self) -> String;
}

pub struct Wizard<S: WizardStep, R, E> {
    state: WizardState<S, R, E>,
    progress: WizardProgress<S>,
    receiver: Option<mpsc::Receiver<WizardMessage<S, R, E>>>,
}

pub enum WizardMessage<S, R, E> {
    StepStarted(S),
    StepCompleted(S, R),
    StepProgress(S, usize, usize),
    AllComplete,
    Error(E),
}
```

This migration prepares both implementations for this eventual unification.

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| Breaking existing functionality | Comprehensive snapshot tests before refactoring |
| Channel deadlocks | Use bounded channels with proper error handling |
| State machine bugs | Exhaustive unit tests for all transitions |
| UI rendering issues | Snapshot tests for every state combination |
| Performance regression | Profile parallel operations, maintain throttling |

---

## Timeline Estimate

| Phase | Complexity | Dependencies |
|-------|------------|--------------|
| Phase 1: Foundation Types | Low | None |
| Phase 2: Progress Tracking | Low | Phase 1 |
| Phase 3: State Machine | Medium | Phases 1-2 |
| Phase 4: Context | Low | Phase 3 |
| Phase 5: Background Task | High | Phases 1-4 |
| Phase 6: UI Rendering | Medium | Phases 2-3 |
| Phase 7: Message Handling | Medium | Phases 5-6 |
| Final Integration | Medium | All phases |

---

## Appendix: Snapshot Test Matrix

### State × Step × Progress Combinations

| State | Current Step | Progress | Snapshot Name |
|-------|-------------|----------|---------------|
| Initializing | - | - | `initializing` |
| Initializing | - | deps_available | `initializing_with_deps` |
| Running | FetchPRs | started | `fetch_prs_started` |
| Running | FetchPRs | completed | `fetch_prs_completed` |
| Running | FetchWorkItems | started | `fetch_work_items_started` |
| Running | FetchWorkItems | 0/10 | `fetch_work_items_0_of_10` |
| Running | FetchWorkItems | 5/10 | `fetch_work_items_5_of_10` |
| Running | FetchWorkItems | 10/10 | `fetch_work_items_10_of_10` |
| Running | FetchWorkItems | completed | `fetch_work_items_completed` |
| Running | FetchCommitInfo | started | `fetch_commits_started` |
| Running | FetchCommitInfo | 2/5 | `fetch_commits_2_of_5` |
| Running | FetchCommitInfo | completed | `fetch_commits_completed` |
| Running | AnalyzeDeps | started | `analyze_deps_started` |
| Running | AnalyzeDeps | completed | `analyze_deps_completed` |
| Running | AnalyzeDeps | skipped | `analyze_deps_skipped` |
| Complete | all | all_done | `complete_all_steps` |
| Complete | 3 | deps_skipped | `complete_deps_skipped` |
| Error | FetchPRs | no_prs | `error_no_prs_at_step1` |
| Error | FetchPRs | api_error | `error_api_at_step1` |
| Error | FetchWorkItems | api_error | `error_api_at_step2` |
| Error | FetchWorkItems | timeout | `error_timeout_at_step2` |
| Error | FetchCommitInfo | api_error | `error_api_at_step3` |
| Error | AnalyzeDeps | repo_not_found | `error_repo_not_found_skippable` |
| Error | any | generic | `error_generic` |

Total: ~25 snapshot tests covering all meaningful UI states.
