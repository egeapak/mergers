# PR Dependency UI - Design Questions

## Questions Requiring Clarification

These design decisions need your input before implementation can proceed.

---

### Q1: Dependency Column Format

**Context**: The new "Deps" column shows dependency counts.

**Options**:
1. `P/D` format (e.g., `2/1` = 2 partial, 1 full) - compact
2. `P+D` format (e.g., `2+1`) - additive feel
3. Separate mini-columns `P | D` - clearer but wider
4. Icon-based: `◐2 ●1` (half-circle for partial, full circle for full)
5. Total only with color: `3` in red/yellow based on severity

**Recommendation**: Option 1 (`P/D`) is compact and fits the 7-char column width.

**Your preference?**

---

### Q2: Dependency Dialog - Transitive Dependencies

**Context**: PR A depends on B, B depends on C. Should the dialog for A show C?

**Options**:
1. **Direct only**: Only show immediate dependencies (simpler, less noise)
2. **Full tree**: Show transitive dependencies nested (complete picture)
3. **Configurable depth**: Default 1, allow expanding (balanced)
4. **Separate sections**: "Direct" and "Transitive" sections

**Recommendation**: Option 3 with default depth 1, expand on demand.

**Your preference?**

---

### Q3: Unselected Dependency Highlight Color

**Context**: PRs that selected PRs depend on but aren't selected need highlighting.

**Options**:
1. **Orange/Amber** `Rgb(80, 40, 0)` - warning without alarm
2. **Yellow background** - high visibility
3. **Magenta/Purple** - distinct from green/blue/red
4. **Red border on row** (instead of background)
5. **Blinking/bold** - very attention-grabbing

**Recommendation**: Option 1 (Orange/Amber) - clear warning without being as alarming as red.

**Your preference?**

---

### Q4: Border Warning Behavior

**Context**: Border turns yellow when there are missing dependencies.

**Options**:
1. **Yellow border + title indicator**: `"Pull Requests (⚠ 2 missing deps)"`
2. **Yellow border only**: No title change
3. **Title indicator only**: Border stays white
4. **Animated/blinking border**: Very attention-grabbing
5. **Red border for critical (full deps), yellow for partial**

**Recommendation**: Option 1 - both visual cues for clarity.

**Your preference?**

---

### Q5: Analysis Timing - Blocking vs Background

**Context**: Dependency analysis can take several seconds for many PRs.

**Options**:
1. **Blocking with progress**: Show "Analyzing dependencies (15/100)..."
2. **Background async**: Transition to PR list immediately, show deps when ready
3. **On-demand**: Don't analyze until user presses a key (e.g., 'A')
4. **Hybrid**: Show list immediately, analyze in background, highlight when done

**Recommendation**: Option 1 for consistency with existing loading stages. Analysis time is acceptable.

**Your preference?**

---

### Q6: Dialog Navigation - Jump to Dependency

**Context**: When viewing dependencies in the dialog, should pressing Enter navigate to that PR in the list?

**Options**:
1. **Yes, jump and close**: Close dialog, move cursor to selected dependency
2. **Yes, jump and keep open**: Move cursor but keep dialog visible
3. **No navigation**: Dialog is view-only
4. **Optional select**: 's' to select dependency, 'Enter' to jump

**Recommendation**: Option 1 - natural workflow to review and then select dependencies.

**Your preference?**

---

### Q7: Should `rayon` Be Optional?

**Context**: Adding rayon increases compile time and binary size.

**Options**:
1. **Always include**: Parallel analysis is always available
2. **Feature flag**: `--features parallel-analysis`
3. **Runtime detection**: Use rayon if available, fallback to sequential

**Recommendation**: Option 1 - the performance benefit outweighs minimal size increase. Modern CPUs expect parallelism.

**Your preference?**

---

### Q8: Missing Dependency Count Scope

**Context**: "Missing deps" in title/status bar - what should it count?

**Options**:
1. **Unique PRs**: Count each unselected dependency PR once
2. **Total relationships**: Sum of all dependency relationships
3. **Critical only**: Only count full dependencies, not partials
4. **By severity**: "2 critical, 5 partial missing"

**Recommendation**: Option 1 - users care about how many PRs they need to add, not relationship count.

**Your preference?**

---

### Q9: Dialog Position

**Context**: Where should the dependency dialog appear?

**Options**:
1. **Centered overlay**: Traditional modal (like search overlay)
2. **Side panel (right)**: Leave PR list partially visible
3. **Bottom panel**: Split screen horizontally
4. **Full screen**: Replace PR list entirely

**Recommendation**: Option 1 - consistent with existing dialogs (search, state filter).

**Your preference?**

---

### Q10: Keyboard Shortcut for Dialog

**Context**: Which key opens the dependency dialog?

**Options**:
1. `d` - for "dependencies"
2. `D` (shift+d) - less accidental presses
3. `Ctrl+d` - modifier key
4. `F2` or similar function key
5. Something else?

**Recommendation**: Option 1 (`d`) - intuitive and consistent with lowercase shortcuts (`s` for states, `/` for search).

**Your preference?**

---

## Implementation Priority Questions

### Q11: Phase Order

Should I implement in the proposed order (1→2→3→4→5) or would you prefer a different sequence?

For example:
- Could start with Phase 4 (highlighting) for immediate value
- Could defer Phase 3 (dialog) if time-constrained

---

### Q12: Incremental vs Big Bang

**Options**:
1. **Incremental commits**: One phase per PR, merge as complete
2. **Single PR**: All phases in one larger PR
3. **Feature branch**: Develop all, merge when complete

**Recommendation**: Option 1 - easier to review and test incrementally.

**Your preference?**

---

## Additional Features (Future Consideration)

These are not in the current plan but could be added:

1. **Auto-select dependencies**: Button to select all unselected deps at once
2. **Dependency export**: Export graph as DOT/Mermaid for documentation
3. **Conflict prediction**: Use dependency analysis to predict merge conflicts
4. **Ordering suggestion**: Suggest optimal cherry-pick order based on graph

Should any of these be included in the initial implementation?

---

## Summary

Please review and respond to each question. Your answers will be incorporated into the implementation plan before coding begins.

To respond, you can either:
1. Edit this file directly with your choices
2. Reply verbally and I'll update the plan accordingly
