# Contributing

## Prerequisites

- **Rust 1.85+**
- **Git**
- **Linux only**: The `arboard` clipboard crate requires these system libraries:
  ```bash
  sudo apt-get install libx11-dev libxcb-shape0-dev libxcb-xfixes0-dev
  ```
  macOS and Windows have no extra system dependencies.

## Getting Started

```bash
git clone https://github.com/egeapak/mergers.git
cd mergers
lefthook install
cargo build
```

## Testing

```bash
# Run tests
cargo nextest run

# Or with the standard runner
cargo test

# Review snapshot changes
cargo insta review

# Generate HTML coverage report
cargo llvm-cov nextest --html
```

## Code Style

```bash
# Format
cargo fmt

# Lint
cargo clippy --all-targets --all-features -- -D warnings
```

Pre-commit hooks via `lefthook` automatically run `cargo fmt --check` and `cargo clippy` on every commit. Install them with `lefthook install` as shown above.

## Pull Request Guidelines

This project uses [git-cliff](https://git-cliff.org/) for changelog generation, so commit messages must follow [Conventional Commits](https://www.conventionalcommits.org/):

| Prefix | Use for |
|--------|---------|
| `feat:` | New features |
| `fix:` | Bug fixes |
| `docs:` | Documentation changes |
| `refactor:` | Code restructuring without behavior change |
| `test:` | Adding or updating tests |
| `chore:` | Maintenance tasks, dependency bumps |

Before submitting:

- All tests pass: `cargo nextest run`
- Clippy is clean: `cargo clippy --all-targets --all-features -- -D warnings`
- Code is formatted: `cargo fmt`
- Reference any related issues in the PR description
