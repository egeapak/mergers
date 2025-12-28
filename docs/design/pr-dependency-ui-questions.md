# PR Dependency UI - Design Decisions

## Resolved Questions

All design decisions have been made. This document records the final choices.

---

### Q1: Dependency Column Format

**Decision**: Option 1 - `P/D` format (e.g., `2/1` = 2 partial, 1 full)

Standard, compact format that fits the column width.

---

### Q2: Dependency Dialog - Transitive Dependencies

**Decision**: Full tree with color differentiation

- Show complete transitive dependency tree
- Direct dependencies in one color
- Transitive dependencies in a different color
- Allows users to see the complete picture at a glance

---

### Q3: Unselected Dependency Highlight Color

**Decision**: Option 1 - Orange/Amber `Rgb(80, 40, 0)`

Clear warning without being as alarming as red.

---

### Q4: Border Warning Behavior

**Decision**: Option 1 - Yellow border + title indicator

`"Pull Requests (⚠ 2 missing deps)"` - both visual cues for clarity.

---

### Q5: Analysis Timing - Blocking vs Background

**Decision**: Option 1 - Blocking with progress

Show "Analyzing dependencies (15/100)..." - consistent with existing loading stages.

---

### Q6: Dialog Navigation - Jump to Dependency

**Decision**: Deferred

Skip for initial implementation. Dialog is view-only for now.

---

### Q7: Should `rayon` Be Optional?

**Decision**: Option 1 - Always include

Performance benefit outweighs minimal size increase.

---

### Q8: Missing Dependency Count Scope

**Decision**: Option 1 - Unique PRs

Count each unselected dependency PR once.

---

### Q9: Dialog Position

**Decision**: Option 1 - Centered overlay

Consistent with existing dialogs (search, state filter).

---

### Q10: Keyboard Shortcut for Dialog

**Decision**: Option 1 - `d` for dependencies

Intuitive and consistent with lowercase shortcuts.

---

### Q11: Phase Order

**Decision**: Implement in proposed order (1→2→3→4→5)

---

### Q12: Incremental vs Big Bang

**Decision**: Incremental commits per phase

---

## Deferred Features

The following features are explicitly deferred for future consideration:

1. **Auto-select dependencies**: Skipped - selection should be deliberate
2. **Dialog navigation (jump to PR)**: Skipped - view-only dialog for now
3. **Dependency export**: Future consideration
4. **Conflict prediction**: Future consideration
5. **Ordering suggestion**: Future consideration

---

## Implementation Summary

| Feature | Decision |
|---------|----------|
| Column format | `P/D` |
| Transitive deps | Full tree, color-coded |
| Highlight color | Orange/Amber |
| Border warning | Yellow + title indicator |
| Analysis timing | Blocking with progress |
| Dialog navigation | View-only (deferred) |
| Rayon | Always included |
| Missing count | Unique PRs |
| Dialog position | Centered overlay |
| Shortcut | `d` |
| Auto-select | Deferred |
