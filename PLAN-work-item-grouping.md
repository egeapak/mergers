# Plan: PR Work Item Grouping Feature

## Overview

Add a feature that groups PRs sharing the same work item(s) and warns users when selecting a PR without including other PRs connected to the same work item. Allow users to select all related PRs with a single action.

## Current State Analysis

### Existing Structures
- `PullRequestWithWorkItems` contains a PR and its linked work items
- Work items have `id`, `fields.title`, `fields.state`, etc.
- PR selection is tracked via `selected: bool` field
- UI state in `PullRequestSelectionState` manages selection, dialogs, and navigation

### Key Files
- `src/models.rs` - Data structures
- `src/core/operations/pr_selection.rs` - Selection logic
- `src/ui/state/default/pr_selection.rs` - UI rendering and interaction
- `src/core/operations/mod.rs` - Module exports

## Implementation Plan

### Phase 1: Core Logic - Work Item Grouping

#### 1.1 Create Work Item Grouping Module
**File**: `src/core/operations/work_item_grouping.rs`

```rust
/// Index mapping work item IDs to PR indices
pub struct WorkItemPrIndex {
    /// Map from work item ID to list of PR indices that share it
    work_item_to_prs: HashMap<i32, Vec<usize>>,
    /// Map from PR index to list of work item IDs
    pr_to_work_items: HashMap<usize, Vec<i32>>,
}

impl WorkItemPrIndex {
    /// Build index from list of PRs
    pub fn build(prs: &[PullRequestWithWorkItems]) -> Self;

    /// Get all PR indices that share any work item with the given PR
    pub fn get_related_pr_indices(&self, pr_index: usize) -> Vec<usize>;

    /// Get all PR indices for a specific work item
    pub fn get_prs_for_work_item(&self, work_item_id: i32) -> &[usize];

    /// Get shared work items between two PRs
    pub fn get_shared_work_items(&self, pr_index_a: usize, pr_index_b: usize) -> Vec<i32>;
}
```

#### 1.2 Add Warning Detection Functions
```rust
/// Check if selecting a PR would leave related PRs unselected
pub fn check_selection_warning(
    prs: &[PullRequestWithWorkItems],
    index: &WorkItemPrIndex,
    pr_index: usize,
) -> Option<SelectionWarning>;

pub struct SelectionWarning {
    /// The PR being selected
    pub selected_pr_index: usize,
    /// Related PRs that are not selected
    pub unselected_related_prs: Vec<usize>,
    /// Work items shared between selected and unselected PRs
    pub shared_work_items: Vec<i32>,
}
```

### Phase 2: UI State Extensions

#### 2.1 Add State Fields to `PullRequestSelectionState`
**File**: `src/ui/state/default/pr_selection.rs`

```rust
// New fields in PullRequestSelectionState
work_item_pr_index: Option<WorkItemPrIndex>,  // Cached index
show_work_item_warning_dialog: bool,          // Dialog visibility
work_item_warning: Option<SelectionWarning>,  // Current warning
warning_dialog_selection: usize,              // 0=Select This Only, 1=Select All Related
```

#### 2.2 Add New Keyboard Handlers
- When pressing `Space` or `Enter` to select a PR:
  1. Check if PR has unselected related PRs
  2. If yes, show warning dialog instead of immediate selection
  3. Dialog options:
     - **Select This Only** - Select just this PR
     - **Select All Related** - Select this PR and all related PRs
     - **Cancel** - Don't select anything

### Phase 3: UI Rendering

#### 3.1 Warning Dialog Component
```rust
fn render_work_item_warning_dialog(
    &self,
    frame: &mut Frame,
    area: Rect,
    prs: &[PullRequestWithWorkItems],
);
```

Dialog layout:
```
┌─ Work Item Warning ─────────────────────────────────────┐
│                                                         │
│  This PR shares work items with other PRs that are     │
│  not currently selected:                                │
│                                                         │
│  Work Item #12345: "Fix login bug"                     │
│    └─ Also linked to:                                   │
│       • PR #101: "Backend fix" (not selected)          │
│       • PR #102: "Frontend fix" (not selected)         │
│                                                         │
│  Work Item #12346: "Update auth flow"                  │
│    └─ Also linked to:                                   │
│       • PR #103: "Auth changes" (not selected)         │
│                                                         │
│  ┌─────────────────┐  ┌─────────────────────────┐      │
│  │ Select This Only│  │ Select All Related (4)  │      │
│  └─────────────────┘  └─────────────────────────┘      │
│                                                         │
│  Press ← → to choose, Enter to confirm, Esc to cancel  │
└─────────────────────────────────────────────────────────┘
```

#### 3.2 Visual Indicator in PR List
Add indicator column or badge when PR has related PRs:
- `[G]` or similar indicator for PRs with shared work items
- Highlight related PRs when hovering/selecting a PR with shared work items

### Phase 4: Integration

#### 4.1 Build Index on PR Load
In `PullRequestSelectionState::new()`:
```rust
let work_item_pr_index = WorkItemPrIndex::build(&prs);
```

#### 4.2 Modify Selection Flow
1. User presses Space/Enter on a PR
2. Call `check_selection_warning()`
3. If warning exists → show dialog
4. If no warning → proceed with normal selection

### Phase 5: Testing

#### 5.1 Unit Tests (`src/core/operations/work_item_grouping.rs`)
- `test_build_index_empty_prs`
- `test_build_index_no_shared_work_items`
- `test_build_index_single_shared_work_item`
- `test_build_index_multiple_shared_work_items`
- `test_get_related_pr_indices_no_relations`
- `test_get_related_pr_indices_with_relations`
- `test_check_selection_warning_no_warning`
- `test_check_selection_warning_with_unselected_related`
- `test_check_selection_warning_all_related_selected`

#### 5.2 Integration Tests
- Test selection flow with warning dialog
- Test "Select All Related" functionality
- Test "Select This Only" functionality
- Test cancellation behavior

#### 5.3 Snapshot Tests (`src/ui/snapshots/`)
- `work_item_warning_dialog_single_work_item`
- `work_item_warning_dialog_multiple_work_items`
- `pr_list_with_grouping_indicator`
- `work_item_warning_dialog_button_focus_left`
- `work_item_warning_dialog_button_focus_right`

## File Changes Summary

### New Files
1. `src/core/operations/work_item_grouping.rs` - Core grouping logic

### Modified Files
1. `src/core/operations/mod.rs` - Export new module
2. `src/ui/state/default/pr_selection.rs` - UI state and rendering
3. `src/models.rs` - (if needed for new structs)

## Verification Plan

### Manual Testing
1. Load PRs with shared work items
2. Select a PR that shares work items with unselected PRs
3. Verify warning dialog appears
4. Test "Select This Only" - only original PR selected
5. Test "Select All Related" - all related PRs selected
6. Test Cancel - no selection changes
7. Verify grouping indicator appears in PR list

### Automated Testing
1. Run unit tests: `cargo nextest run work_item_grouping`
2. Run snapshot tests: `cargo nextest run snapshot`
3. Run full test suite: `cargo nextest run`
4. Verify coverage: `cargo llvm-cov nextest`

### Edge Cases
1. PR with no work items (should never show warning)
2. PR with work items but no other PRs share them
3. All related PRs already selected (no warning)
4. Circular relationships (A shares with B, B shares with C)
5. Large number of related PRs (scrolling in dialog)

## Dependencies

- No new external dependencies required
- Uses existing `ratatui` patterns for dialog rendering
- Follows existing snapshot testing patterns
