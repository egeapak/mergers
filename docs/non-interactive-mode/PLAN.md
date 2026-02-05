# Non-Interactive Merge Mode Implementation Plan

## Overview

This document describes the implementation plan for adding non-interactive mode to the merge workflow, enabling AI agents and CI systems to perform stateful merge operations with support for conflict resolution, resume, and explicit completion.

**Branch:** `claude/add-noninteractive-merge-mode-nGPeX`
**Created:** 2024-12-22
**Status:** Planning Complete

---

## Table of Contents

1. [Design Decisions](#design-decisions)
2. [Architecture Overview](#architecture-overview)
3. [CLI Structure](#cli-structure)
4. [Module Structure](#module-structure)
5. [Core Components](#core-components)
6. [State File Design](#state-file-design)
7. [Workflow Diagrams](#workflow-diagrams)
8. [Implementation Phases](#implementation-phases)
9. [Exit Codes](#exit-codes)
10. [Dependencies](#dependencies)

---

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| **PR Selection** | ALL work items must match specified states AND PR must have ≥1 work item | Ensures consistent selection criteria |
| **State File Location** | Per-repository using path hash in XDG state dir, with `MERGERS_STATE_DIR` override | Isolation between repos, standard location |
| **State File Lifecycle** | Keep all files for history (cleanup commands later) | Enables history/audit trail |
| **Worktree vs Clone** | Same as interactive - worktree if `local_repo` set | Consistency between modes |
| **Conflict Timeout** | Infinite wait | AI agents/users need unlimited time |
| **Post-merge Tagging** | NOT automatic - user must run `complete` subcommand | User validation required before tagging |
| **Locking** | Simple lockfile with PID | Prevents concurrent operations |
| **CLI Structure** | Subcommands for each operation | Clean argument separation |
| **State in Both Modes** | Yes - both interactive and non-interactive generate state files | Enables cross-mode resume |
| **Git Hooks** | Disabled by default via `core.hooksPath=/dev/null`, enable with `--run-hooks` | Prevents commit hook failures during cherry-pick |
| **Config Type Safety** | Use `MergeConfig` type directly (not `AppConfig` enum) | Leverages new associated type system |

---

## Architecture Overview

### Typed Configuration

The codebase uses a typed configuration system with associated types:

```rust
// AppMode trait with associated Config type
pub trait AppMode: Send + Sync {
    type Config: AppModeConfig + Send + Sync;
    fn base(&self) -> &AppBase<Self::Config>;
    fn base_mut(&mut self) -> &mut AppBase<Self::Config>;
}

// MergeConfig contains merge-specific settings
pub struct MergeConfig {
    pub shared: SharedConfig,
    pub work_item_state: ParsedProperty<String>,
    pub run_hooks: ParsedProperty<bool>,  // Controls git hook execution
}

// AppBase is generic over config type
pub struct AppBase<C: AppModeConfig> {
    pub config: Arc<C>,
    pub pull_requests: Vec<PullRequestWithWorkItems>,
    pub client: AzureDevOpsClient,
    // ...
}
```

Core operations should work with `MergeConfig` directly for type safety.

### Module Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           CLI Entry Point                                    │
│                        (src/bin/mergers.rs)                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│  merge -n            │  merge continue  │  merge abort  │  merge complete   │
│  --non-interactive   │                  │               │  --next-state     │
│  --version           │                  │               │                   │
│  --select-by-state   │                  │               │                   │
│  --run-hooks         │                  │               │                   │
└──────────┬───────────┴────────┬─────────┴───────┬───────┴─────────┬─────────┘
           │                    │                 │                 │
           ▼                    ▼                 ▼                 ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Core Module (NEW)                                    │
│                         src/core/                                            │
├─────────────────────────────────────────────────────────────────────────────┤
│  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────────┐          │
│  │ operations/      │  │ state/           │  │ runner/          │          │
│  │                  │  │                  │  │                  │          │
│  │ • data_loading   │  │ • file.rs        │  │ • traits.rs      │          │
│  │ • pr_selection   │  │   MergeStateFile │  │   MergeRunner    │          │
│  │ • repository_    │  │   LockGuard      │  │ • merge_engine   │          │
│  │   setup          │  │ • conversion.rs  │  │ • non_           │          │
│  │ • cherry_pick    │  │                  │  │   interactive    │          │
│  │ • post_merge     │  │                  │  │                  │          │
│  └──────────────────┘  └──────────────────┘  └──────────────────┘          │
│                                                                              │
│  ┌──────────────────┐                                                       │
│  │ output/          │                                                       │
│  │ • format.rs      │                                                       │
│  │ • events.rs      │                                                       │
│  └──────────────────┘                                                       │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ Reuses
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Existing Modules                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│  src/git.rs          │  src/api/        │  src/models.rs                    │
│  • cherry_pick_      │  • client.rs     │  • CherryPickItem                 │
│    commit            │  • fetch_prs     │  • CherryPickStatus               │
│  • setup_repository  │  • add_label     │  • PullRequestWithWorkItems       │
│  • cleanup_          │  • update_wi     │                                   │
│    cherry_pick       │                  │                                   │
│  (Also: AbortingState logic)           │                                   │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## CLI Structure

### Command Hierarchy

```
mergers merge                                    # Interactive TUI (default)
mergers merge -n [OPTIONS]                       # Non-interactive start

mergers merge continue [OPTIONS]                 # Continue after conflict
mergers merge abort [OPTIONS]                    # Abort in-progress merge
mergers merge status [OPTIONS]                   # Show current state
mergers merge complete [OPTIONS]                 # Tag PRs and update work items
```

### Subcommand Arguments

#### `merge -n` (Start New Merge Non-Interactively)

```
USAGE:
    mergers merge -n [OPTIONS]

OPTIONS:
    --non-interactive, -n       Run without TUI (for CI/AI agents)
    --version <VERSION>         Merge branch version (required with -n)
    --select-by-state <STATES>  Comma-separated work item states for PR filtering
    --work-item-state <STATE>   State for work items after completion
    --run-hooks                 Enable git hooks during cherry-pick (disabled by default)
    --output <FORMAT>           Output format: text, json, ndjson [default: text]
    --quiet, -q                 Suppress progress output

    [Standard shared options: --organization, --project, etc.]
```

**Note on `--run-hooks`:** By default, git hooks are disabled during cherry-pick operations
by setting `core.hooksPath=/dev/null` in the worktree. This prevents commit hooks from
failing during automated cherry-picks. Use `--run-hooks` to enable hooks if needed.

#### `merge continue` (Resume After Conflict)

```
USAGE:
    mergers merge continue [OPTIONS]

OPTIONS:
    --repo <PATH>        Repository path (auto-detected if in repo)
    --output <FORMAT>    Output format: text, json, ndjson [default: text]
    --quiet, -q          Suppress progress output
```

#### `merge abort` (Cancel In-Progress Merge)

```
USAGE:
    mergers merge abort [OPTIONS]

OPTIONS:
    --repo <PATH>        Repository path (auto-detected if in repo)
    --output <FORMAT>    Output format: text, json, ndjson [default: text]
```

#### `merge status` (Show Current State)

```
USAGE:
    mergers merge status [OPTIONS]

OPTIONS:
    --repo <PATH>        Repository path (auto-detected if in repo)
    --output <FORMAT>    Output format: text, json, ndjson [default: text]
```

#### `merge complete` (Finalize Merge)

```
USAGE:
    mergers merge complete [OPTIONS]

OPTIONS:
    --repo <PATH>          Repository path (auto-detected if in repo)
    --next-state <STATE>   State to set work items to (required)
    --output <FORMAT>      Output format: text, json, ndjson [default: text]
    --quiet, -q            Suppress progress output
```

---

## Module Structure

```
src/
├── core/                               # NEW: Core abstractions
│   ├── mod.rs                          # ExitCode, module exports
│   │
│   ├── operations/                     # UI-independent operations
│   │   ├── mod.rs                      # Module exports
│   │   ├── data_loading.rs             # fetch_prs, fetch_work_items
│   │   ├── pr_selection.rs             # filter_by_work_item_states
│   │   ├── repository_setup.rs         # setup worktree/clone
│   │   ├── cherry_pick.rs              # cherry_pick_commit, continue, abort
│   │   └── post_merge.rs               # tag_pr, update_work_item
│   │
│   ├── state/                          # State persistence
│   │   ├── mod.rs                      # Module exports
│   │   ├── file.rs                     # MergeStateFile, LockGuard, paths
│   │   └── conversion.rs               # To/from CherryPickItem conversions
│   │
│   ├── runner/                         # Execution engines
│   │   ├── mod.rs                      # Module exports
│   │   ├── traits.rs                   # MergeRunner trait definition
│   │   ├── merge_engine.rs             # Core merge orchestration logic
│   │   └── non_interactive.rs          # CLI runner implementation
│   │
│   └── output/                         # Output formatting
│       ├── mod.rs                      # Module exports
│       ├── format.rs                   # text/json/ndjson helpers
│       └── events.rs                   # ProgressEvent enum definitions
│
├── ui/                                 # MODIFIED: Integrates with core
│   ├── state/default/
│   │   ├── data_loading.rs             # Delegates to core::operations
│   │   ├── cherry_pick.rs              # Delegates to core::operations
│   │   ├── aborting.rs                 # Already exists - reuse cleanup logic
│   │   ├── post_completion.rs          # Delegates to core::operations
│   │   └── ...
│   └── ...
│
├── bin/
│   └── mergers.rs                      # Entry point with subcommand routing
│
└── models.rs                           # Add MergeSubcommand enum
```

---

## Core Components

### 1. State File (`src/core/state/file.rs`)

```rust
pub struct MergeStateFile {
    pub schema_version: u32,

    // Timestamps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // Repository Identity
    pub repo_path: PathBuf,
    pub base_repo_path: Option<PathBuf>,
    pub is_worktree: bool,

    // Azure DevOps Context
    pub organization: String,
    pub project: String,
    pub repository: String,

    // Branch Configuration
    pub dev_branch: String,
    pub target_branch: String,
    pub merge_version: String,

    // Cherry-pick State
    pub cherry_pick_items: Vec<StateCherryPickItem>,
    pub current_index: usize,

    // Current Phase
    pub phase: MergePhase,
    pub conflicted_files: Option<Vec<String>>,

    // Settings
    pub work_item_state: String,
    pub tag_prefix: String,
    pub run_hooks: bool,  // Whether git hooks are enabled for this merge

    // Completion Info
    pub completed_at: Option<DateTime<Utc>>,
    pub final_status: Option<MergeStatus>,
}
```

**Note on `run_hooks`:** This field captures the `--run-hooks` setting at merge start time.
When resuming with `merge continue`, the saved setting is used to ensure consistent behavior.

### 2. Merge Phases

```rust
pub enum MergePhase {
    Loading,                    // Loading data from Azure DevOps
    Setup,                      // Repository setup (worktree/clone)
    CherryPicking,             // Cherry-picking in progress
    AwaitingConflictResolution, // Waiting for conflict resolution
    ReadyForCompletion,        // Cherry-picks done, awaiting 'complete'
    Completing,                // Running post-merge tasks
    Completed,                 // All done
    Aborted,                   // Aborted by user
}
```

### 3. Lock Guard

```rust
pub struct LockGuard {
    path: PathBuf,
}

impl MergeStateFile {
    pub fn acquire_lock(repo_path: &Path) -> Result<LockGuard>;
    pub fn lock_path_for_repo(repo_path: &Path) -> PathBuf;
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Remove lock file
    }
}
```

### 4. PR Selection Logic

```rust
/// Filter PRs by work item states.
///
/// Rules:
/// 1. PR must have at least one work item
/// 2. ALL work items must be in one of the specified states
pub fn filter_prs_by_work_item_states(
    prs: &[PullRequestWithWorkItems],
    states: &[String],
) -> Vec<&PullRequestWithWorkItems>;

pub fn select_prs_by_work_item_states(
    prs: &mut [PullRequestWithWorkItems],
    states: &[String],
) -> usize;  // Returns count of selected PRs
```

### 5. Output Events

```rust
#[derive(Serialize)]
#[serde(tag = "event")]
pub enum ProgressEvent {
    Start { total_prs: usize, version: String, target_branch: String },
    CherryPickStart { pr_id: i32, commit_id: String, index: usize, total: usize },
    CherryPickSuccess { pr_id: i32, commit_id: String },
    CherryPickConflict { pr_id: i32, conflicted_files: Vec<String>, repo_path: String },
    CherryPickFailed { pr_id: i32, error: String },
    CherryPickSkipped { pr_id: i32 },
    PostMergeStart { task_count: usize },
    PostMergeProgress { task_type: String, target_id: i32, status: String },
    Complete { successful: usize, failed: usize, skipped: usize },
}
```

---

## State File Design

### File Location

```
Default: ~/.local/state/mergers/merge-{hash}.json
Override: $MERGERS_STATE_DIR/merge-{hash}.json

Lock:    ~/.local/state/mergers/merge-{hash}.lock
```

Where `{hash}` is the first 16 characters of SHA-256 of the canonical repository path.

### State Transitions

```
              ┌──────────────┐
              │   Loading    │
              └──────┬───────┘
                     │
                     ▼
              ┌──────────────┐
              │    Setup     │
              └──────┬───────┘
                     │
                     ▼
              ┌──────────────┐
         ┌───►│ CherryPicking│◄───┐
         │    └──────┬───────┘    │
         │           │            │
         │    ┌──────▼───────┐    │
         │    │   Conflict   │────┘ (after resolution + continue)
         │    └──────┬───────┘
         │           │ (skip)
         └───────────┘
                     │ (all done)
                     ▼
              ┌──────────────────┐
              │ReadyForCompletion│
              └──────┬───────────┘
                     │ (merge complete --next-state)
                     ▼
              ┌──────────────┐
              │  Completing  │
              └──────┬───────┘
                     │
                     ▼
              ┌──────────────┐
              │  Completed   │
              └──────────────┘

At any point:
  - User can run `merge abort` → Aborted state
```

---

## Workflow Diagrams

### Non-Interactive Workflow

```
   User/AI Agent                          mergers                    Repository
        │                                    │                           │
        │  mergers merge -n                  │                           │
        │  --version v1.2.3                  │                           │
        │  --select-by-state "Ready"         │                           │
        │ ──────────────────────────────────►│                           │
        │                                    │  1. Acquire lock          │
        │                                    │  2. Load PRs from ADO     │
        │                                    │  3. Filter by WI states   │
        │                                    │  4. Create state file     │
        │                                    │  5. Setup worktree        │
        │                                    │─────────────────────────►│
        │                                    │  6. Cherry-pick commits   │
        │                                    │─────────────────────────►│
        │                                    │                           │
        │         ┌──────────────────────────┤  CONFLICT!               │
        │         │ Exit code 2              │  Phase: AwaitingConflict │
        │         │ State saved              │  Lock released           │
        │◄────────┴──────────────────────────│                           │
        │                                    │                           │
        │  (User resolves conflict)          │                           │
        │                                    │                           │
        │  mergers merge continue            │                           │
        │ ──────────────────────────────────►│                           │
        │                                    │  7. Acquire lock          │
        │                                    │  8. Load state file       │
        │                                    │  9. Check conflicts done  │
        │                                    │  10. Continue cherry-pick │
        │                                    │─────────────────────────►│
        │                                    │  ... repeat until done    │
        │                                    │                           │
        │         ┌──────────────────────────┤  Exit code 0              │
        │         │ Phase: ReadyForCompletion│  Lock released            │
        │◄────────┴──────────────────────────│                           │
        │                                    │                           │
        │  (User validates: tests, review)   │                           │
        │  (User pushes branch)              │                           │
        │                                    │                           │
        │  mergers merge complete            │                           │
        │  --next-state "Done"               │                           │
        │ ──────────────────────────────────►│                           │
        │                                    │  11. Acquire lock         │
        │                                    │  12. Load state file      │
        │                                    │  13. Tag PRs in ADO       │
        │                                    │  14. Update work items    │
        │                                    │  15. Phase: Completed     │
        │         ┌──────────────────────────┤  Exit code 0              │
        │◄────────┴──────────────────────────│  Lock released            │
        │                                    │                           │
```

### Interactive Mode Integration

Interactive mode will also use state files:

1. State file created at `SetupRepo` phase
2. Updated after each cherry-pick
3. On conflict, user can exit TUI and use CLI commands
4. `merge complete` can be run after TUI exits

---

## Implementation Phases

### Phase 1: Foundation (Core Infrastructure)

**Files to create/modify:**
- `src/core/mod.rs` - Module structure, ExitCode enum
- `src/core/state/mod.rs` - Module exports
- `src/core/state/file.rs` - MergeStateFile, LockGuard, path helpers
- `src/models.rs` - Add MergeSubcommand, MergeRunArgs, etc.
- `Cargo.toml` - Add dependencies (sha2, dirs)

**Key deliverables:**
- [ ] Create `src/core/` module structure
- [ ] Implement `MergeStateFile` struct with serialization
- [ ] Implement per-repo path hashing
- [ ] Implement PID-based locking with `LockGuard`
- [ ] Add `MergeSubcommand` enum to models
- [ ] Add argument structs for each subcommand
- [ ] Define `ExitCode` enum

**Tests:**
- [ ] State file serialization/deserialization
- [ ] Path hashing produces consistent results
- [ ] Lock acquisition and release
- [ ] Lock detects stale processes

### Phase 2: Core Operations (Extract from UI)

**Files to create/modify:**
- `src/core/operations/mod.rs` - Module exports
- `src/core/operations/data_loading.rs` - Extracted from UI
- `src/core/operations/pr_selection.rs` - New filtering logic
- `src/core/operations/repository_setup.rs` - Extracted from UI
- `src/core/operations/cherry_pick.rs` - Extracted from UI
- `src/core/operations/post_merge.rs` - Extracted from UI
- `src/core/state/conversion.rs` - Type conversions

**Key deliverables:**
- [ ] Extract `fetch_pull_requests()` from DataLoadingState
- [ ] Extract `fetch_work_items_parallel()` from DataLoadingState
- [ ] Implement `filter_prs_by_work_item_states()` (ALL WIs must match)
- [ ] Implement `select_prs_by_work_item_states()` (in-place selection)
- [ ] Extract repository setup logic from SetupRepoState
- [ ] Extract `process_next_commit()` logic from CherryPickState
- [ ] Extract continue/abort helpers (reuse AbortingState logic)
- [ ] Extract `tag_pr()` and `update_work_item()` from PostCompletionState
- [ ] Implement state file ↔ CherryPickItem conversions

**Tests:**
- [ ] PR filtering: ALL work items must match
- [ ] PR filtering: PRs without work items excluded
- [ ] PR filtering: case-insensitive state matching
- [ ] Cherry-pick result handling
- [ ] State conversion round-trips

### Phase 3: Output System

**Files to create/modify:**
- `src/core/output/mod.rs` - Module exports
- `src/core/output/events.rs` - ProgressEvent enum
- `src/core/output/format.rs` - Formatters for text/json/ndjson

**Key deliverables:**
- [ ] Define `ProgressEvent` enum with all event types
- [ ] Implement text formatter (human-readable)
- [ ] Implement JSON formatter (summary at end)
- [ ] Implement NDJSON formatter (streaming)
- [ ] Implement conflict output structure
- [ ] Implement summary output structure

**Tests:**
- [ ] JSON serialization of all event types
- [ ] Text output formatting
- [ ] NDJSON line-by-line output

### Phase 4: Non-Interactive Runner

**Files to create/modify:**
- `src/core/runner/mod.rs` - Module exports
- `src/core/runner/traits.rs` - MergeRunner trait
- `src/core/runner/merge_engine.rs` - Core orchestration
- `src/core/runner/non_interactive.rs` - CLI runner

**Key deliverables:**
- [ ] Define `MergeRunner` trait
- [ ] Implement `MergeEngine` for core orchestration
- [ ] Implement `NonInteractiveRunner`:
  - [ ] `run()` - start new merge
  - [ ] `continue_merge()` - resume after conflict
  - [ ] `abort()` - cleanup and cancel
  - [ ] `status()` - show current state
  - [ ] `complete()` - tag PRs and update WIs
- [ ] Handle all exit codes properly

**Tests:**
- [ ] Run starts merge and saves state
- [ ] Conflict detection saves state and exits
- [ ] Continue resumes from saved state
- [ ] Abort cleans up properly
- [ ] Complete tags PRs and updates WIs
- [ ] Status shows correct state

### Phase 5: Entry Point Integration

**Files to create/modify:**
- `src/bin/mergers.rs` - Subcommand routing

**Key deliverables:**
- [ ] Add subcommand routing for merge mode
- [ ] Handle `merge -n` (non-interactive mode)
- [ ] Handle `merge continue`
- [ ] Handle `merge abort`
- [ ] Handle `merge status`
- [ ] Handle `merge complete`
- [ ] Proper exit code handling

**Tests:**
- [ ] CLI argument parsing for all subcommands
- [ ] Correct routing to handlers

### Phase 6: Interactive Mode Integration

**Files to create/modify:**
- `src/ui/state/default/setup_repo.rs` - Create state file
- `src/ui/state/default/cherry_pick.rs` - Update state file
- `src/ui/state/default/conflict_resolution.rs` - Update state file
- `src/ui/state/default/completion.rs` - Mark ready for completion
- `src/ui/state/default/post_completion.rs` - Use core operations

**Key deliverables:**
- [ ] Create state file on SetupRepo
- [ ] Update state file after each cherry-pick
- [ ] Update state file on conflict
- [ ] Mark ReadyForCompletion when cherry-picks done
- [ ] Integrate PostCompletion with core operations
- [ ] Enable `merge complete` from CLI after TUI exit

**Tests:**
- [ ] State file created in interactive mode
- [ ] State file updated correctly
- [ ] Can resume with CLI after TUI conflict

### Phase 7: Testing & Documentation

**Files to create/modify:**
- Integration tests
- Documentation updates

**Key deliverables:**
- [ ] Integration tests for full non-interactive workflow
- [ ] Integration tests for conflict + continue
- [ ] Integration tests for abort
- [ ] Integration tests for complete
- [ ] Update CLAUDE.md with new commands
- [ ] Update README if exists

---

## Exit Codes

| Code | Name | Meaning |
|------|------|---------|
| 0 | `Success` | All operations completed successfully |
| 1 | `GeneralError` | General error (config, network, git, etc.) |
| 2 | `Conflict` | Conflict detected - user must resolve and run 'continue' |
| 3 | `PartialSuccess` | Some PRs succeeded, some failed/skipped |
| 4 | `NoStateFile` | No state file found for the repository |
| 5 | `InvalidPhase` | State file exists but operation not valid for current phase |
| 6 | `NoPRsMatched` | No PRs matched selection criteria |
| 7 | `Locked` | Another merge is in progress (locked) |

---

## Dependencies

### New Crate Dependencies

```toml
[dependencies]
sha2 = "0.10"          # For path hashing
dirs = "5.0"           # For XDG directories

# Already present:
# serde, serde_json   # Serialization
# chrono              # Timestamps
# tokio               # Async runtime
```

### Existing Dependencies to Leverage

- `anyhow` - Error handling
- `clap` - CLI parsing (already has subcommand support)
- `serde` / `serde_json` - State file serialization
- `chrono` - Timestamps in state file

---

## Example Usage

```bash
# === START NEW MERGE ===
mergers merge -n \
  --version v1.2.3 \
  --select-by-state "Ready for Next,Approved" \
  --output json

# === IF CONFLICT (exit code 2) ===
# Resolve conflicts in the worktree
cd /path/to/worktree
git status
# ... edit files ...
git add .

# Continue the merge
mergers merge continue

# === WHEN READY (all cherry-picks done) ===
# Validate the branch
git log --oneline
cargo test
git push origin patch/next-v1.2.3

# Complete: tag PRs and update work items
mergers merge complete --next-state "Next Merged"

# === OTHER COMMANDS ===
# Check status
mergers merge status --output json

# Abort and clean up
mergers merge abort
```

---

## Notes

### Existing Code to Leverage

1. **AbortingState** (`src/ui/state/default/aborting.rs`)
   - Already implements background cleanup with `git::cleanup_cherry_pick`
   - Uses `Arc<Mutex<>>` pattern for async status
   - Can be reused for non-interactive abort

2. **git::cleanup_cherry_pick** (`src/git.rs`)
   - Already handles worktree removal
   - Already handles branch deletion
   - Already handles cherry-pick abort

3. **git::setup_repository** (`src/git.rs`)
   - Accepts `run_hooks: bool` parameter
   - When `false`, sets `core.hooksPath=/dev/null` in the worktree
   - Both `shallow_clone_repo` and `create_worktree` support this

4. **PostCompletionState** (`src/ui/state/default/post_completion.rs`)
   - Task tracking pattern already exists
   - Tag PR and update WI logic already implemented

5. **Typed Configuration** (`src/models.rs`, `src/ui/app_mode.rs`)
   - `MergeConfig` struct with direct access to merge settings
   - `AppBase<C>` generic over config type
   - `AppModeConfig` trait for shared config access

### Cross-Mode Resume

The state file format is designed to support:
- Starting in TUI, exiting on conflict, continuing with CLI
- Starting with CLI, switching to TUI for complex situations
- History tracking for audit purposes

### State File Cleanup (Future)

Future commands to add:
- `mergers merge history` - list past merges
- `mergers merge cleanup [--older-than 30d]` - clean old state files
