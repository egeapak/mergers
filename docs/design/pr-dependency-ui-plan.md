# PR Dependency Analysis UI Integration Plan

## Overview

This plan details moving PR dependency analysis to run before the PR list is displayed, making analysis parallel with rayon, and adding UI features to visualize dependencies in the PR selection screen.

## Current State

### Where Dependency Analysis Runs Now
- **Location**: `src/core/runner/non_interactive.rs:131-171`
- **Timing**: After repository setup, before cherry-picking
- **Problem**: In TUI mode, users select PRs without knowing dependencies

### Current Data Structures
- `PRDependencyGraph` - DAG of all PRs and their relationships
- `PRDependencyNode` - Per-PR node with dependencies/dependents
- `DependencyCategory` - Independent, PartiallyDependent, Dependent
- `DependencyWarning` - Warnings for unselected dependencies
- `DependencyAnalysisResult` - Contains graph + warnings

### Current UI State
- PR list rendered in `src/ui/state/default/pr_selection.rs`
- 6 columns: Selection, PR #, Date, Title, Author, Work Items
- Highlighting: Selected PRs get dark green background

## Proposed Changes

### Phase 1: Move Analysis to Data Loading Stage

#### 1.1 Add New Loading Stage
**File**: `src/ui/state/default/data_loading.rs`

```rust
enum LoadingStage {
    NotStarted,
    FetchingPullRequests,
    FetchingWorkItems,
    WaitingForWorkItems,
    FetchingCommitInfo,
    AnalyzingDependencies,  // NEW
    Complete,
}
```

#### 1.2 Store Dependency Graph in MergeApp
**File**: `src/ui/apps/merge_app.rs`

Add field:
```rust
pub struct MergeApp {
    // ... existing fields ...

    /// Cached dependency analysis result.
    /// Populated during data loading, before PR selection.
    dependency_graph: Option<PRDependencyGraph>,
}
```

Add methods:
```rust
impl MergeApp {
    pub fn dependency_graph(&self) -> Option<&PRDependencyGraph> { ... }
    pub fn set_dependency_graph(&mut self, graph: PRDependencyGraph) { ... }
}
```

#### 1.3 Implement Parallel Analysis with Rayon
**File**: `src/core/operations/dependency_analysis.rs`

Current analysis is O(n^2) comparing each PR pair. With rayon:

```rust
use rayon::prelude::*;

impl DependencyAnalyzer {
    pub fn analyze_parallel(&self) -> DependencyAnalysisResult {
        // Parallelize file change fetching per PR
        let changes: Vec<_> = self.pr_infos
            .par_iter()
            .map(|pr| (pr.id, self.get_file_changes(pr)))
            .collect();

        // Parallelize pairwise comparisons
        // Each (i, j) pair where j < i can be computed independently
        let dependencies: Vec<_> = (0..self.pr_infos.len())
            .into_par_iter()
            .flat_map(|i| {
                (0..i).into_par_iter().filter_map(|j| {
                    self.categorize_dependency(&changes[i], &changes[j])
                })
            })
            .collect();

        // Build graph from collected dependencies
        self.build_graph_from_dependencies(dependencies)
    }
}
```

**Note**: Requires adding `rayon` to Cargo.toml dependencies.

#### 1.4 Integrate Analysis into Data Loading
**File**: `src/ui/state/default/data_loading.rs`

After commit info fetching, before transitioning to PR selection:

```rust
LoadingStage::FetchingCommitInfo => {
    // ... existing commit fetch logic ...
    self.loading_stage = LoadingStage::AnalyzingDependencies;
}
LoadingStage::AnalyzingDependencies => {
    // Run parallel dependency analysis
    let graph = self.analyze_dependencies_parallel(app)?;
    app.set_dependency_graph(graph);
    self.loading_stage = LoadingStage::Complete;
}
```

### Phase 2: Add Dependency Column to PR List

#### 2.1 New Column: "Deps"
**File**: `src/ui/state/default/pr_selection.rs`

Add 7th column showing dependency counts:

```
| Sel | PR #   | Date       | Title              | Author    | Deps  | Work Items     |
| ✓   | 12345  | 2024-01-15 | Fix login bug      | user1     | 2/1   | #111 (Active)  |
|     | 12346  | 2024-01-16 | Add feature X      | user2     | 0/0   | #222 (New)     |
```

Format: `P/D` where:
- `P` = Partial dependency count (yellow)
- `D` = Full dependency count (red if > 0)

Column constraints:
```rust
vec![
    Constraint::Length(3),      // Selection
    Constraint::Length(8),      // PR #
    Constraint::Length(12),     // Date
    Constraint::Percentage(25), // Title (reduced from 30%)
    Constraint::Percentage(15), // Author (reduced from 20%)
    Constraint::Length(7),      // Deps (NEW)
    Constraint::Percentage(25), // Work Items
]
```

#### 2.2 Color Coding for Dependency Column
```rust
fn get_deps_style(partial: usize, dependent: usize) -> Style {
    if dependent > 0 {
        Style::default().fg(Color::Red)
    } else if partial > 0 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Green)
    }
}
```

### Phase 3: Dependency Graph Dialog

#### 3.1 New Shortcut: 'g' for Dependency Graph Dialog
**File**: `src/ui/state/default/pr_selection.rs`

Add to key handler:
```rust
KeyCode::Char('g') => {
    if let Some(selected_idx) = self.table_state.selected() {
        self.show_dependency_dialog = true;
        self.dialog_pr_index = Some(selected_idx);
    }
    StateChange::Keep
}
```

#### 3.2 Dialog UI Structure (Full Tree with Color Differentiation)

The dialog shows the complete transitive dependency tree with visual distinction:
- **Direct dependencies**: Cyan/default color
- **Transitive dependencies**: Gray/dimmed color

```
┌─────────────── Dependencies for PR #12345 ───────────────┐
│                                                          │
│ ◀ Dependencies (PRs this PR depends on):                 │
│   ├─ #12340 "Initial API setup" [FULL]           (direct)│
│   │   └─ Files: src/api/client.rs (lines 10-50)          │
│   │   └─ #12335 "Base types" [PARTIAL]       (transitive)│
│   └─ #12342 "Add auth module" [PARTIAL]          (direct)│
│       └─ Files: src/auth/mod.rs                          │
│       └─ #12338 "Config module" [FULL]       (transitive)│
│                                                          │
│ ▶ Dependents (PRs that depend on this PR):               │
│   ├─ #12348 "Extend API client" [FULL]           (direct)│
│   │   └─ #12355 "API v2" [PARTIAL]           (transitive)│
│   └─ #12350 "Add caching" [PARTIAL]              (direct)│
│                                                          │
│ Colors: Direct=Cyan  Transitive=Gray                     │
│ Legend: [FULL] = Overlapping lines  [PARTIAL] = Same file│
│                                                          │
│ Press 'Esc' to close, '↑/↓' to scroll                    │
└──────────────────────────────────────────────────────────┘
```

#### 3.3 Dialog State
```rust
struct DependencyDialogState {
    pr_index: usize,
    scroll_offset: usize,
}
```

#### 3.4 Transitive Dependency Computation
```rust
fn compute_transitive_dependencies(
    graph: &PRDependencyGraph,
    pr_id: i32,
) -> Vec<(PRDependency, bool)> {  // (dependency, is_transitive)
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    // Add direct dependencies
    if let Some(node) = graph.get_node(pr_id) {
        for dep in &node.dependencies {
            result.push((dep.clone(), false));  // direct
            queue.push_back(dep.to_pr_id);
            visited.insert(dep.to_pr_id);
        }
    }

    // BFS for transitive dependencies
    while let Some(current_id) = queue.pop_front() {
        if let Some(node) = graph.get_node(current_id) {
            for dep in &node.dependencies {
                if !visited.contains(&dep.to_pr_id) {
                    result.push((dep.clone(), true));  // transitive
                    queue.push_back(dep.to_pr_id);
                    visited.insert(dep.to_pr_id);
                }
            }
        }
    }

    result
}
```

#### 3.5 Color Scheme for Dialog
```rust
const DIRECT_DEP_COLOR: Color = Color::Cyan;
const TRANSITIVE_DEP_COLOR: Color = Color::DarkGray;
const FULL_DEP_INDICATOR: Color = Color::Red;
const PARTIAL_DEP_INDICATOR: Color = Color::Yellow;
```

**Note**: Dialog is view-only. Navigation to jump to dependency PRs is deferred for future implementation.

### Phase 4: Unselected Dependency Highlighting

#### 4.1 Compute Unselected Dependencies
When selection changes, compute:
```rust
fn compute_unselected_dependencies(&self, app: &MergeApp) -> HashSet<i32> {
    let selected_ids: HashSet<_> = app.pull_requests()
        .iter()
        .filter(|pr| pr.selected)
        .map(|pr| pr.pr.id)
        .collect();

    let mut unselected_deps = HashSet::new();

    if let Some(graph) = app.dependency_graph() {
        for &selected_id in &selected_ids {
            if let Some(node) = graph.get_node(selected_id) {
                for dep in &node.dependencies {
                    if !selected_ids.contains(&dep.to_pr_id) {
                        unselected_deps.insert(dep.to_pr_id);
                    }
                }
            }
        }
    }

    unselected_deps
}
```

#### 4.2 Row Highlighting for Unselected Dependencies
```rust
let row_style = if pr_with_wi.selected {
    Style::default().bg(Color::Rgb(0, 60, 0))  // Dark green (existing)
} else if unselected_deps.contains(&pr_with_wi.pr.id) {
    Style::default().bg(Color::Rgb(80, 40, 0))  // Orange/amber
} else if is_current_search_result {
    Style::default().bg(Color::Blue)
} else {
    Style::default()
};
```

#### 4.3 Border Color Warning
When there are unselected dependencies:
```rust
let border_style = if !unselected_deps.is_empty() {
    Style::default().fg(Color::Yellow)
} else {
    Style::default().fg(Color::White)
};

let table = Table::new(rows, constraints)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(
                "Pull Requests{}",
                if !unselected_deps.is_empty() {
                    format!(" (⚠ {} missing dependencies)", unselected_deps.len())
                } else {
                    String::new()
                }
            ))
            .border_style(border_style),
    );
```

### Phase 5: Status Bar Dependency Summary

Add to bottom status bar:
```
Selected: 5 | Missing deps: 2 | Full deps: 3 | Partial deps: 7
```

## File Changes Summary

| File | Changes |
|------|---------|
| `Cargo.toml` | Add `rayon` dependency |
| `src/ui/apps/merge_app.rs` | Add `dependency_graph` field and methods |
| `src/ui/state/default/data_loading.rs` | Add `AnalyzingDependencies` stage |
| `src/core/operations/dependency_analysis.rs` | Add `analyze_parallel()` method |
| `src/ui/state/default/pr_selection.rs` | Add Deps column, dialog, highlighting |
| `src/ui/state/default/mod.rs` | Export new dialog state if separate file |

## Testing Strategy

1. **Unit Tests**
   - Parallel analysis produces same results as sequential
   - Dependency column formatting
   - Unselected dependency computation

2. **Snapshot Tests**
   - PR list with dependency column
   - Dependency dialog rendering
   - Highlighted unselected dependencies
   - Warning border style

3. **Integration Tests**
   - Full flow: load → analyze → display → select → highlight

## Design Decisions (Finalized)

All design decisions have been made. See `docs/design/pr-dependency-ui-questions.md` for the complete record.

**Key Decisions:**
- Column format: `P/D` (partial/full counts)
- Transitive deps: Full tree with color differentiation (direct=cyan, transitive=gray)
- Highlight color: Orange/Amber `Rgb(80, 40, 0)`
- Border warning: Yellow border + title indicator
- Analysis timing: Blocking with progress
- Dialog: View-only (navigation deferred)
- Rayon: Always included (not feature-flagged)

**Deferred Features:**
- Auto-select missing dependencies
- Dialog navigation to jump to dependency PR
