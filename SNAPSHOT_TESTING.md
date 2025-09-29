# Snapshot Testing for UI Components

This document describes the snapshot testing infrastructure for TUI components using cargo-insta and ratatui's TestBackend.

## Architecture

### Centralized Structure
All UI snapshots are organized in `src/ui/snapshots/` following the source code structure:

```
src/ui/snapshots/
├── state/
│   ├── default/
│   │   ├── cherry_pick/
│   │   ├── completion/
│   │   ├── conflict_resolution/
│   │   ├── data_loading/
│   │   ├── post_completion/
│   │   ├── pr_selection/
│   │   ├── setup_repo/
│   │   └── version_input/
│   ├── migration/
│   │   ├── data_loading/
│   │   ├── results/
│   │   ├── tagging/
│   │   └── version_input/
│   └── shared/
│       ├── settings_confirmation/
│       └── error/
```

### Automatic Path Detection & Clean Naming
The snapshot testing infrastructure automatically:
- Determines snapshot locations using `std::module_path!()`
- Uses clean, descriptive filenames without module path prefixes
- Since directory structure mirrors source code, verbose prefixes are unnecessary

## Usage

### Basic Pattern
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::testing::*;
    use crate::ui::snapshot_testing::with_settings_and_module_path;
    use insta::assert_snapshot;

    #[test]
    fn test_my_ui_component() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);
            let state = Box::new(MyState::new(/* args */));

            harness.render_state(state);
            assert_snapshot!("test_name", harness.backend());
        });
    }
}
```

### Key Components

#### TuiTestHarness
- Fixed terminal size: 80x30 for consistent snapshots
- Provides mock App and AzureDevOpsClient for testing
- Various configuration builders for different test scenarios

#### Snapshot Path Configuration
- `with_settings_and_module_path()`: Uses automatic path detection
- `with_settings_explicit()`: Allows manual path override
- `with_snapshot_settings!` macro: Convenience wrapper

## Test Configuration Builders

The testing infrastructure provides several configuration builders:

- `create_test_config_default()`: Mixed sources (CLI, env, file, git, default)
- `create_test_config_migration()`: Migration mode configuration
- `create_test_config_all_defaults()`: All default values
- `create_test_config_cli_values()`: CLI-provided values
- `create_test_config_env_values()`: Environment variables
- `create_test_config_file_values()`: Configuration file values

## Best Practices

### Test Structure
1. **Use descriptive snapshot names**: `assert_snapshot!("meaningful_name", ...)`
2. **Document test scenarios**: Include comprehensive documentation
3. **Test multiple configurations**: Cover different input sources
4. **Fixed terminal size**: Always use the standard 80x30 dimensions

### Organization
1. **Mirror source structure**: Snapshots follow the same hierarchy as source files
2. **One directory per module**: Each UI module gets its own snapshot directory
3. **Centralized location**: All UI snapshots under `src/ui/snapshots/`

### Development Workflow
1. Write test using `with_settings_and_module_path(module_path!(), || { ... })`
2. Run test to generate initial snapshot
3. Use `cargo insta review` to examine and accept snapshots
4. Future runs will compare against accepted snapshots

## Example: Settings Confirmation Tests

The `settings_confirmation.rs` module demonstrates the complete pattern:

```rust
#[test]
fn test_settings_confirmation_default_mode() {
    use crate::ui::snapshot_testing::with_settings_and_module_path;

    with_settings_and_module_path(module_path!(), || {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        let state = Box::new(SettingsConfirmationState::new(
            harness.app.config.as_ref().clone(),
        ));

        harness.render_state(state);
        assert_snapshot!("default_mode", harness.backend());
    });
}
```

This creates snapshots in: `src/ui/snapshots/state/shared/settings_confirmation/`

## Configuration

### Insta Configuration File

The project includes `.config/insta.yaml` with optimized settings:

```yaml
# Behavior configuration
behavior:
  update: auto          # Updates snapshots during test, requires review
  output: diff          # Show detailed diffs when snapshots don't match
  force_pass: false     # Don't force tests to pass when snapshots don't match
  glob_fail_fast: true  # Fail fast when using glob patterns

# Test runner configuration
test:
  runner: nextest       # Use nextest (integrates with existing .config/nextest.toml)
  auto_review: false    # Don't automatically review - require explicit step
  auto_accept_unseen: false # Don't automatically accept new snapshots

# Review configuration
review:
  include_ignored: false    # Include ignored files in review
  include_hidden: true      # Include hidden directories (important for centralized snapshots)
  warn_undiscovered: true   # Warn about snapshots that can't be discovered
```

### Environment Variable Overrides

Users can override settings via environment variables:
- `INSTA_UPDATE=always` - Always update snapshots
- `INSTA_FORCE_PASS=1` - Force tests to pass
- `INSTA_OUTPUT=minimal` - Reduce output verbosity

## Benefits

1. **Automatic organization**: No hardcoded paths, adapts to code moves
2. **Regression detection**: UI changes are caught immediately
3. **Visual documentation**: Snapshots serve as examples of expected output
4. **Maintainable**: Easy to update with `cargo insta review`
5. **CI-friendly**: Prevents unwanted changes in continuous integration
6. **Nextest integration**: Works seamlessly with existing test infrastructure

## Adding Tests to New Modules

When adding snapshot tests to a new UI module:

1. Add test module with the standard pattern
2. The snapshot directory will be created automatically
3. Use meaningful snapshot names for clarity
4. Follow the existing documentation standards

No configuration changes needed - the infrastructure handles path detection automatically using `module_path!()`.