# Mergers - Claude Assistant Guide

## Project Overview

**Mergers** is a Rust-based CLI/TUI application for managing Azure DevOps pull request merging and migration workflows. The tool provides:

- **Azure DevOps Integration**: Fetches PRs, work items, and manages API interactions
- **Interactive TUI**: Terminal-based user interface built with `ratatui` for PR selection
- **Git Operations**: Cherry-picking, worktree management, and repository analysis
- **Migration Analysis**: Categorizes PRs for migration between branches
- **Flexible Workflows**: Supports both new clones and existing repository worktrees

## Architecture

- **Library Structure**: `src/lib.rs` (library) + `src/bin/mergers.rs` (binary)
- **Modular Design**: Clear separation between API, Git, UI, and configuration modules
- **State Machine**: UI follows state machine pattern for different workflow modes
- **Async Runtime**: Built on `tokio` with multi-threaded async execution

## Testing Infrastructure

### Test Framework
- **cargo-nextest**: Used for parallel test execution and improved performance
- **cargo-llvm-cov**: Provides comprehensive code coverage reporting
- **Configuration**: Test profiles defined in `.config/nextest.toml`:
  - `default`: Standard development settings
  - `ci`: Optimized for CI with longer timeouts and retries
  - `dev`: Verbose output for local development

### Test Documentation Standard
All tests are documented with markdown headers following this format:

```rust
/// # Test Name
///
/// Brief description of what the test validates.
///
/// ## Test Scenario
/// - Bullet points describing the test setup
/// - What conditions are created
/// - What operations are performed
///
/// ## Expected Outcome
/// - What should happen when the test passes
/// - Expected behavior and results
#[test]
fn test_function_name() {
    // test implementation
}
```

### Running Tests

**Standard test execution:**
```bash
cargo test
```

**With nextest (recommended):**
```bash
cargo nextest run
```

**With coverage:**
```bash
cargo llvm-cov nextest
```

**Generate HTML coverage report:**
```bash
cargo llvm-cov nextest --html
```

**CI profile with coverage:**
```bash
cargo llvm-cov nextest --profile ci --lcov --output-path lcov.info
```

### Snapshot Testing

The project includes comprehensive snapshot testing for UI components using `cargo-insta` and `ratatui`'s TestBackend.

#### Architecture
- **Centralized Structure**: All UI snapshots are organized in `src/ui/snapshots/` following the source code structure
- **Automatic Path Detection**: Uses `std::module_path!()` for clean, descriptive filenames without verbose prefixes
- **Fixed Terminal Size**: Consistent 80x30 dimensions for reliable snapshots

#### Usage Pattern
```rust
#[test]
fn test_ui_component() {
    with_settings_and_module_path(module_path!(), || {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        let state = Box::new(MyState::new(/* args */));

        harness.render_state(state);
        assert_snapshot!("test_name", harness.backend());
    });
}
```

#### Test Configuration Builders
- `create_test_config_default()`: Mixed sources (CLI, env, file, git, default)
- `create_test_config_migration()`: Migration mode configuration
- `create_test_config_all_defaults()`: All default values
- `create_test_config_cli_values()`: CLI-provided values
- `create_test_config_env_values()`: Environment variables
- `create_test_config_file_values()`: Configuration file values

## Key Dependencies

### Core Dependencies
- **ratatui** (0.29): Terminal UI framework
- **crossterm** (0.28): Cross-platform terminal manipulation
- **tokio** (1.x): Async runtime with multi-threading
- **reqwest** (0.12): HTTP client for Azure DevOps API
- **clap** (4.x): Command-line argument parsing
- **serde** + **serde_json**: Serialization for API responses
- **chrono**: Date/time handling
- **toml**: Configuration file parsing

### Development Dependencies
- **mockito**: HTTP mocking for API tests
- **tokio-test**: Testing utilities for async code
- **tempfile**: Temporary file/directory creation for tests

## Development Workflow

### Pre-commit Hooks (lefthook)
The project uses `lefthook.yml` for pre-commit validation:
```yaml
pre-commit:
  parallel: true
  commands:
    fmt:
      run: cargo fmt --check
    clippy:
      run: cargo clippy --all-targets --all-features -- -D warnings
```

### CI/CD Pipeline
GitHub Actions workflow (`.github/workflows/ci.yml`) includes:
- **Multi-platform testing**: Ubuntu, Windows, macOS
- **Coverage reporting**: Integrated with Codecov
- **Linting**: `cargo fmt` and `cargo clippy` validation
- **Nextest integration**: Parallel test execution in CI

### Essential Commands

**Formatting:**
```bash
cargo fmt
```

**Linting:**
```bash
cargo clippy --all-targets --all-features -- -D warnings
```

**Build:**
```bash
cargo build --release
```

**Development testing:**
```bash
cargo nextest run --profile dev
```

## Module Organization

- **`api`**: Azure DevOps API client and data fetching
- **`config`**: Configuration loading and management
- **`git`**: Git operations, worktrees, and repository analysis
- **`migration`**: PR categorization and migration analysis
- **`models`**: Data structures and domain models
- **`ui`**: Terminal user interface and state management
- **`utils`**: Utility functions (date parsing, HTML parsing, throttling)

## Important Notes for Development

### Configuration Sources
The tool supports multiple configuration sources with precedence:
1. Command-line arguments (highest)
2. Environment variables (`MERGERS_*`)
3. Configuration file (`~/.config/mergers/config.toml`)
4. Default values (lowest)

### Git Integration
- Supports both shallow cloning and worktree creation
- Handles conflict resolution during cherry-picking
- Maintains clean separation between main repo and working branches

### Azure DevOps Integration
- Requires Personal Access Token (PAT) for authentication
- Supports both modern and legacy Azure DevOps URL formats
- Implements pagination and rate limiting for API calls

## Non-Interactive Merge Mode

The tool supports a non-interactive mode designed for CI/CD pipelines and AI agents.

### CLI Commands

**Start a new merge:**
```bash
mergers merge -n --version v1.0.0 --select-by-state "Ready for Next"
```

**Check status:**
```bash
mergers merge status [--repo /path/to/repo] [--output json|text|ndjson]
```

**Continue after conflict resolution:**
```bash
mergers merge continue [--repo /path/to/repo]
```

**Abort a merge:**
```bash
mergers merge abort [--repo /path/to/repo]
```

**Complete merge (tag PRs, update work items):**
```bash
mergers merge complete --next-state "Done" [--repo /path/to/repo]
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Conflict - manual resolution needed |
| 3 | Partial success (some operations failed) |
| 4 | No state file found |
| 5 | Invalid phase for operation |
| 6 | No PRs matched criteria |
| 7 | Locked (another merge in progress) |

### Output Formats

- `text` (default): Human-readable progress with Unicode symbols
- `json`: Single JSON object with summary at end
- `ndjson`: Streaming newline-delimited JSON (one event per line)

### State Files

State files are stored in `~/.local/state/mergers/` (Linux) or equivalent XDG directory.
Override with `MERGERS_STATE_DIR` environment variable.

State files enable:
- Resume after conflicts
- Cross-mode handoff (TUI → CLI)
- Progress tracking

## Post-Task Completion Checklist

**IMPORTANT**: After completing any code modifications, always run these commands before considering the task complete:

1. **Format code:**
   ```bash
   cargo fmt
   ```

2. **Fix linting issues:**
   ```bash
   cargo clippy --all-targets --all-features -- -D warnings
   ```

3. **Verify tests pass:**
   ```bash
   cargo nextest run
   ```

These steps ensure code meets project standards and maintains consistency with the existing codebase. The pre-commit hooks will enforce these same standards, so running them manually prevents commit failures.