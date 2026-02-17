# Mergers

[![CI](https://github.com/egeapak/mergers/actions/workflows/ci.yml/badge.svg)](https://github.com/egeapak/mergers/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/egeapak/mergers/branch/master/graph/badge.svg?token=18UOQC3763)](https://codecov.io/gh/egeapak/mergers)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

A Rust CLI/TUI tool for streamlining Azure DevOps pull request merging and migration workflows via cherry-picking.

<!-- TODO: Add demo GIF
## Demo

![mergers TUI demo](./assets/demo.gif)
-->

## Table of Contents

- [Features](#features)
- [Installation](#installation)
- [Commands](#commands)
- [Quick Start](#quick-start)
- [Usage](#usage)
- [Configuration](#configuration)
- [TUI Controls](#tui-controls)
- [Non-Interactive Mode](#non-interactive-mode)
- [Development](#development)
- [Troubleshooting](#troubleshooting)
- [Contributing](#contributing)
- [License](#license)

## Features

- **Azure DevOps Integration**
  - Fetch pull requests from any organization, project, and repository
  - Filter PRs by tags and merge status
  - Retrieve and display associated work items
  - Update work item states after successful merges

- **Interactive TUI**
  - Select PRs visually with keyboard navigation
  - View PR details and associated work items
  - Open PRs and work items in browser directly from the TUI

- **Flexible Git Workflow**
  - Shallow clone or git worktree support
  - Automated cherry-picking with conflict handling
  - Interactive conflict resolution prompts

- **Non-Interactive Mode**
  - CI/CD friendly commands for automated pipelines
  - JSON/NDJSON output formats for scripting
  - State persistence for resumable operations

## Installation

### Prerequisites

- **Rust 1.85+** (edition 2024)
- **Git** installed and accessible in PATH
- **Azure DevOps PAT** with Code Read and Work Items Read permissions
- **Linux only**: System libraries required by the `arboard` clipboard crate:
  ```bash
  sudo apt-get install libx11-dev libxcb-shape0-dev libxcb-xfixes0-dev
  ```
  macOS and Windows have no extra system dependencies.

### From Source

```bash
git clone https://github.com/egeapak/mergers.git
cd mergers
cargo build --release
```

The executable will be at `target/release/mergers`.

### Install via Cargo

```bash
cargo install mergers
```

### Pre-built Binaries

Download from the [Releases](https://github.com/egeapak/mergers/releases) page.

## Commands

| Subcommand | Alias | Description |
|------------|-------|-------------|
| `merge` | `m` | Cherry-pick merged PRs from dev branch to target branch (interactive TUI or non-interactive) |
| `migrate` | `mi` | Analyze PRs to determine migration eligibility based on work item states |
| `cleanup` | `c` | Delete local patch branches that have been merged to the target branch |
| `release-notes` | `rn` | Generate formatted release notes from git tags and associated work items |

Run `mergers <subcommand> --help` for detailed options.

## Quick Start

```bash
# Set your PAT as an environment variable (recommended)
export MERGERS_PAT="your-azure-devops-pat"

# Run with minimal arguments
mergers -o "MyOrg" -p "MyProject" -r "MyRepo"
```

## Usage

### Basic Usage

```bash
mergers \
    -o "MyAzureOrg" \
    -p "MyProject" \
    -r "MyRepo" \
    -t "YOUR_AZURE_DEVOPS_PAT" \
    --dev-branch "develop" \
    --target-branch "release/1.2.0"
```

### Using Local Repository (Worktree Mode)

```bash
mergers \
    -o "MyAzureOrg" \
    -p "MyProject" \
    -r "MyRepo" \
    --dev-branch "main" \
    --target-branch "hotfix/1.2.1" \
    --local-repo "/path/to/your/local/clone"
```

### Workflow

1. Fetch pull requests from the specified `--dev-branch`
2. Display TUI for PR selection
3. Enter a version number for the new branch name
4. Clone repository or create worktree
5. Cherry-pick selected PRs into the new branch
6. Resolve conflicts interactively if they occur

### Command-Line Arguments

| Argument | Short | Description | Default |
|----------|-------|-------------|---------|
| `--organization` | `-o` | Azure DevOps organization | Required |
| `--project` | `-p` | Azure DevOps project | Required |
| `--repository` | `-r` | Repository name | Required |
| `--pat` | `-t` | Personal Access Token | `$MERGERS_PAT` |
| `--dev-branch` | | Source branch for PRs | `dev` |
| `--target-branch` | | Target branch for merge | `next` |
| `--local-repo` | | Local repo path (worktree mode) | None |

## Configuration

### Configuration File

Create `~/.config/mergers/config.toml`:

```toml
organization = "MyOrg"
project = "MyProject"
repository = "MyRepo"
dev_branch = "develop"
target_branch = "main"
```

### Environment Variables

All configuration options can be set via environment variables with the `MERGERS_` prefix:

| Variable | Description |
|----------|-------------|
| `MERGERS_PAT` | Azure DevOps Personal Access Token |
| `MERGERS_ORGANIZATION` | Azure DevOps organization |
| `MERGERS_PROJECT` | Azure DevOps project |
| `MERGERS_REPOSITORY` | Repository name |
| `MERGERS_DEV_BRANCH` | Source branch for PRs |
| `MERGERS_TARGET_BRANCH` | Target branch for merge |
| `MERGERS_STATE_DIR` | Custom state directory path |

### Configuration Precedence

1. Command-line arguments (highest)
2. Environment variables
3. Configuration file
4. Default values (lowest)

## TUI Controls

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate PR list |
| `Space` | Toggle PR selection |
| `Enter` | Confirm selections |
| `p` | Open PR in browser |
| `w` | Open work items in browser |
| `q` | Quit |

## Non-Interactive Mode

For CI/CD pipelines and automation:

```bash
# Start a merge
mergers merge run -n --version v1.0.0 --select-by-state "Ready for Next"

# Check status
mergers merge status --output json

# Continue after resolving conflicts
mergers merge continue

# Abort a merge
mergers merge abort

# Complete and update work items
mergers merge complete --next-state "Done"
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Conflict - manual resolution needed |
| 3 | Partial success |
| 4 | No state file found |
| 5 | Invalid phase |
| 6 | No PRs matched |
| 7 | Locked (merge in progress) |

## Development

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release
```

### Testing

This project uses [cargo-nextest](https://nexte.st/) for test execution and [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) for coverage.

```bash
# Standard tests
cargo test

# With nextest (faster)
cargo nextest run

# With coverage
cargo llvm-cov nextest

# HTML coverage report
cargo llvm-cov nextest --html

# CI coverage report
cargo llvm-cov nextest --lcov --output-path lcov.info
```

### Test Profiles

Configured in `.config/nextest.toml`:
- `default`: Standard development settings
- `ci`: Optimized for CI with longer timeouts
- `dev`: Verbose output for local development

### Code Quality

```bash
# Format code
cargo fmt

# Lint
cargo clippy --all-targets --all-features -- -D warnings
```

## Troubleshooting

### Common Issues

**Authentication Failed**
- Verify your PAT has not expired
- Ensure PAT has `Code (Read)` and `Work Items (Read)` scopes
- Check organization/project names are correct

**Git Clone/Worktree Fails**
- Ensure git is installed and in PATH
- Check network connectivity to Azure DevOps
- Verify repository URL is accessible with your PAT

**Cherry-pick Conflicts**
- The tool will pause and prompt for manual resolution
- Resolve conflicts in the worktree directory
- Use `mergers merge continue` to resume

**PAT Security**
- Never hardcode PATs in scripts
- Use environment variables or secure credential storage
- Set PAT expiration dates and use minimal scopes

## Contributing

Please see [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, testing, code style, and pull request guidelines.

## License

This project is licensed under the [MIT License](LICENSE).
