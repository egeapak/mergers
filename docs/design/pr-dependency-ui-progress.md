# PR Dependency UI Integration - Progress Tracker

## Status: Ready for Implementation

**Last Updated**: 2024-12-28
**Current Phase**: Implementation Phase 1
**Blocked By**: None - all design decisions finalized

---

## Phases Overview

| Phase | Description | Status | Completion |
|-------|-------------|--------|------------|
| 1 | Move Analysis to Data Loading | ⏳ Pending | 0% |
| 2 | Add Dependency Column to PR List | ⏳ Pending | 0% |
| 3 | Dependency Graph Dialog | ⏳ Pending | 0% |
| 4 | Unselected Dependency Highlighting | ⏳ Pending | 0% |
| 5 | Status Bar Summary | ⏳ Pending | 0% |

---

## Phase 1: Move Analysis to Data Loading

### Tasks

- [ ] Add `rayon` to Cargo.toml
- [ ] Add `dependency_graph` field to `MergeApp`
- [ ] Add getter/setter methods for dependency graph
- [ ] Add `AnalyzingDependencies` loading stage enum variant
- [ ] Implement `analyze_parallel()` in `DependencyAnalyzer`
- [ ] Integrate analysis into data loading flow (blocking with progress)
- [ ] Update loading message: "Analyzing dependencies (N/M)..."
- [ ] Add tests for parallel analysis correctness
- [ ] Add snapshot test for loading stage display

### Design Decisions (Finalized)
- Analysis runs blocking with progress indicator
- Rayon is always included (not feature-flagged)

---

## Phase 2: Add Dependency Column

### Tasks

- [ ] Update column constraints (adjust widths for 7 columns)
- [ ] Add "Deps" column header
- [ ] Implement dependency count cell rendering with `P/D` format
- [ ] Add color coding: Green (0/0), Yellow (P>0), Red (D>0)
- [ ] Update header row alignment
- [ ] Add tests for column formatting
- [ ] Add snapshot tests for new column

### Design Decisions (Finalized)
- Column format: `P/D` (partial/full counts)

---

## Phase 3: Dependency Dialog

### Tasks

- [ ] Create `DependencyDialogState` struct
- [ ] Add dialog rendering function (centered overlay)
- [ ] Implement full transitive dependency tree computation (BFS)
- [ ] Render tree-view for dependencies/dependents
- [ ] Color direct dependencies in Cyan
- [ ] Color transitive dependencies in Gray
- [ ] Add file overlap details display
- [ ] Handle keyboard navigation (scroll with ↑/↓, close with Esc)
- [ ] Add 'd' keybinding
- [ ] Update help text with new shortcut
- [ ] Add snapshot tests for dialog

### Design Decisions (Finalized)
- Show full transitive tree with color differentiation
- Direct deps: Cyan, Transitive deps: Gray
- Dialog is view-only (navigation to PRs deferred)

---

## Phase 4: Unselected Dependency Highlighting

### Tasks

- [ ] Implement `compute_unselected_dependencies()`
- [ ] Add Orange/Amber highlight `Rgb(80, 40, 0)` for unselected deps
- [ ] Update row style selection logic
- [ ] Change border color to Yellow when missing deps
- [ ] Add warning indicator to title: `"Pull Requests (⚠ N missing deps)"`
- [ ] Cache computation (recompute on selection change)
- [ ] Add tests for highlight logic
- [ ] Add snapshot tests for highlighting

### Design Decisions (Finalized)
- Highlight color: Orange/Amber `Rgb(80, 40, 0)`
- Border: Yellow when missing dependencies
- Title shows warning indicator with count

---

## Phase 5: Status Bar Summary

### Tasks

- [ ] Add dependency summary to status bar
- [ ] Show selected count with dependency breakdown
- [ ] Update on selection changes
- [ ] Add tests for summary generation

### Dependencies
- Phase 4 complete

---

## Verification Checklist

See `docs/design/pr-dependency-ui-verification.md` for detailed verification steps.

---

## Blockers & Risks

| Issue | Impact | Mitigation |
|-------|--------|------------|
| Large PR count performance | High | Use rayon, add progress indicator |
| Transitive dependency complexity | Medium | Limit depth or make configurable |
| UI space constraints | Medium | Consider collapsible column |

---

## Notes

- Keep dependency analysis cache in MergeApp for re-use
- Consider lazy evaluation for transitive deps
- Dialog should be usable via keyboard only
