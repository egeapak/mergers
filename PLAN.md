# Plan: Add Associated Types to App Trait for Config Type Safety

## Problem Statement

The current architecture has apps (`MergeApp`, `MigrationApp`, `CleanupApp`) that each require a specific config variant (`AppConfig::Default`, `AppConfig::Migration`, `AppConfig::Cleanup`), but this relationship is not enforced at compile time.

### Current Issues

1. **No compile-time config-app association**: Any app can receive any `AppConfig` variant
2. **Runtime pattern matching required**: Each app must match on all config variants to access mode-specific fields
3. **Repetitive boilerplate**: Each app has identical `Deref`/`DerefMut`/`AppMode` implementations (~25 lines × 3 apps)
4. **Type confusion possible**: Nothing prevents passing `AppConfig::Migration` to `MergeApp`

### Current Code Examples Showing the Problem

```rust
// In MergeApp - must match all variants even though only Default is valid
pub fn work_item_state(&self) -> &str {
    match &*self.config {
        AppConfig::Default { default, .. } => default.work_item_state.value(),
        AppConfig::Migration { .. } => "Next Merged", // fallback - shouldn't happen
        AppConfig::Cleanup { .. } => "Next Merged",   // fallback - shouldn't happen
    }
}

// In MigrationApp - same pattern
pub fn terminal_states(&self) -> &[String] {
    match &*self.config {
        AppConfig::Migration { migration, .. } => migration.terminal_states.value(),
        _ => &[], // fallback - shouldn't happen
    }
}
```

## Proposed Solution

Add an associated `Config` type to the `AppMode` trait that enforces which config type each app works with at compile time.

### Design Approach

#### Option A: Separate Config Structs (Recommended)

Extract each config variant into a standalone struct that includes the shared config:

```rust
// New config structs
pub struct MergeConfig {
    pub shared: SharedConfig,
    pub work_item_state: ParsedProperty<String>,
}

pub struct MigrationConfig {
    pub shared: SharedConfig,
    pub terminal_states: ParsedProperty<Vec<String>>,
}

pub struct CleanupConfig {
    pub shared: SharedConfig,
    pub target: ParsedProperty<String>,
}

// Updated trait with associated type
pub trait AppMode: Send + Sync {
    type Config: AppModeConfig + Send + Sync;

    fn base(&self) -> &AppBase<Self::Config>;
    fn base_mut(&mut self) -> &mut AppBase<Self::Config>;
    fn config(&self) -> &Arc<Self::Config>;
}

// Each app specifies its config type
impl AppMode for MergeApp {
    type Config = MergeConfig;
    // ...
}
```

**Pros:**
- Full compile-time type safety
- Direct access to mode-specific config without pattern matching
- Cleaner API for each app

**Cons:**
- Breaking change to config structure
- Need to update `Args::resolve_config()` to return different types
- More significant refactoring

#### Option B: Keep Enum, Add Type Markers

Keep `AppConfig` as an enum but add marker types to verify correct association:

```rust
pub trait ConfigVariant {
    fn from_config(config: &AppConfig) -> Option<&Self>;
}

pub trait AppMode: Send + Sync {
    type ConfigMarker: ConfigVariant;
    // ...
}
```

**Pros:**
- Less disruptive change
- Maintains backward compatibility

**Cons:**
- Less elegant, still has runtime checks
- Partial solution to the problem

### Recommended Approach: Option A with Migration Path

Implement Option A but maintain backward compatibility during transition:

## Implementation Steps

### Phase 1: Create Config Trait and Struct Hierarchy

1. **Create `AppModeConfig` trait** in `src/models.rs`:
   ```rust
   pub trait AppModeConfig: Clone + Send + Sync {
       fn shared(&self) -> &SharedConfig;
   }
   ```

2. **Create mode-specific config structs**:
   - `MergeConfig` (replaces `AppConfig::Default` fields)
   - `MigrationConfig` (replaces `AppConfig::Migration` fields)
   - `CleanupConfig` (replaces `AppConfig::Cleanup` fields)

3. **Implement `AppModeConfig` for each struct**

### Phase 2: Make AppBase Generic

1. **Update `AppBase` to be generic over config type**:
   ```rust
   pub struct AppBase<C: AppModeConfig> {
       pub config: Arc<C>,
       pub pull_requests: Vec<PullRequestWithWorkItems>,
       pub client: AzureDevOpsClient,
       // ...
   }
   ```

2. **Update `AppBase` methods** to work with generic config

### Phase 3: Update AppMode Trait

1. **Add associated `Config` type to `AppMode`**:
   ```rust
   pub trait AppMode: Send + Sync {
       type Config: AppModeConfig + Send + Sync;

       fn base(&self) -> &AppBase<Self::Config>;
       fn base_mut(&mut self) -> &mut AppBase<Self::Config>;

       // Convenience method to access config
       fn config(&self) -> &Arc<Self::Config> {
           &self.base().config
       }
   }
   ```

2. **Consider adding a derive macro** to reduce boilerplate (optional enhancement)

### Phase 4: Update App Implementations

1. **Update `MergeApp`**:
   ```rust
   pub struct MergeApp {
       base: AppBase<MergeConfig>,
       pub cherry_pick_items: Vec<CherryPickItem>,
       pub current_cherry_pick_index: usize,
   }

   impl AppMode for MergeApp {
       type Config = MergeConfig;

       fn base(&self) -> &AppBase<MergeConfig> { &self.base }
       fn base_mut(&mut self) -> &mut AppBase<MergeConfig> { &mut self.base }
   }

   impl MergeApp {
       // Now type-safe access without pattern matching
       pub fn work_item_state(&self) -> &str {
           self.base().config.work_item_state.value()
       }
   }
   ```

2. **Similarly update `MigrationApp` and `CleanupApp`**

### Phase 5: Update Config Resolution

1. **Update `Args::resolve_config()`** to return mode-specific types:
   ```rust
   pub enum ResolvedConfig {
       Merge(Arc<MergeConfig>),
       Migration(Arc<MigrationConfig>),
       Cleanup(Arc<CleanupConfig>),
   }
   ```

2. **Update `App` enum construction** to use the new types

### Phase 6: Update App Enum Container

1. **Update `App` enum** to work with typed apps:
   - Keep the enum for runtime polymorphism
   - Each variant now contains typed app

2. **Update delegation methods** in `App`:
   ```rust
   impl App {
       pub fn shared_config(&self) -> &SharedConfig {
           match self {
               App::Merge(app) => app.config().shared(),
               App::Migration(app) => app.config().shared(),
               App::Cleanup(app) => app.config().shared(),
           }
       }
   }
   ```

### Phase 7: Update State System Integration

1. **Update `AppState` trait** if needed to work with new config types
2. **Verify `ModeState` derivation** still works correctly

### Phase 8: Cleanup and Testing

1. **Remove old `AppConfig` enum** (or deprecate with type alias)
2. **Update all tests** to use new config types
3. **Run full test suite** and fix any issues
4. **Update documentation** and examples

## Benefits After Implementation

1. **Compile-time type safety**: Cannot pass wrong config to wrong app
2. **No runtime pattern matching**: Direct access to mode-specific config
3. **Cleaner APIs**: Each app knows exactly what config it has
4. **Better error messages**: Compiler catches config mismatches
5. **Self-documenting code**: Associated types make relationships explicit

## Estimated Boilerplate Reduction

| Location | Current Lines | After Change | Reduction |
|----------|---------------|--------------|-----------|
| `MergeApp::work_item_state()` | 6 | 1 | ~83% |
| `MigrationApp::terminal_states()` | 4 | 1 | ~75% |
| `CleanupApp::cleanup_target()` | 4 | 1 | ~75% |
| Config type checks throughout | Scattered | 0 | 100% |

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Breaking changes | Maintain deprecated aliases during transition |
| Complex generics | Keep type bounds simple; use type aliases |
| Test updates | Update incrementally per phase |
| State system impact | Verify trait relationships early |

## Files to Modify

1. `src/models.rs` - Config structs and traits
2. `src/ui/app_mode.rs` - AppMode trait with associated type
3. `src/ui/app_base.rs` - Generic AppBase
4. `src/ui/apps/merge_app.rs` - MergeApp implementation
5. `src/ui/apps/migration_app.rs` - MigrationApp implementation
6. `src/ui/apps/cleanup_app.rs` - CleanupApp implementation
7. `src/ui/app.rs` - App enum updates
8. `src/ui/state/typed.rs` - Verify state system compatibility
9. Test files - Update test configurations

## Success Criteria

- [ ] All apps have associated `Config` type
- [ ] Mode-specific config access is type-safe without pattern matching
- [ ] `cargo clippy` passes with no warnings
- [ ] All existing tests pass
- [ ] New tests verify compile-time type enforcement
