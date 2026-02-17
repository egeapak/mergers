use crate::{config::Config, parsed_property::ParsedProperty, utils::parse_since_date};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::{
    Args as ClapArgs, Parser, Subcommand,
    builder::{Styles, styling::AnsiColor},
};
use serde::Deserialize;

/// Build a version string that includes the git commit hash
fn build_version() -> &'static str {
    // Use concat! with env! to create a compile-time constant string
    concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")")
}

/// Define custom styles for colorized help output
fn help_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default().bold())
        .usage(AnsiColor::Yellow.on_default().bold())
        .literal(AnsiColor::Green.on_default().bold())
        .placeholder(AnsiColor::Cyan.on_default())
        .valid(AnsiColor::Green.on_default())
        .invalid(AnsiColor::Red.on_default())
        .error(AnsiColor::Red.on_default().bold())
}

/// Apply syntax highlighting to shell examples
fn highlight_shell(content: &str) -> String {
    use clap::builder::styling::AnsiColor;

    let comment_style = AnsiColor::BrightBlack.on_default();
    let command_style = AnsiColor::Green.on_default().bold();
    let flag_style = AnsiColor::Cyan.on_default();
    let string_style = AnsiColor::Yellow.on_default();
    let reset = AnsiColor::White.on_default();

    let mut result = String::new();
    let mut in_command_block = false;

    for line in content.lines() {
        let trimmed = line.trim_start();

        // Handle comment lines
        if trimmed.starts_with('#') {
            result.push_str(&format!("{comment_style}{line}{reset:#}\n"));
            in_command_block = false;
            continue;
        }

        // Handle empty lines
        if trimmed.is_empty() {
            result.push('\n');
            in_command_block = false;
            continue;
        }

        // Check if this is a shell command line (starts with known command or continuation)
        let is_command_line = trimmed.starts_with("mergers")
            || (in_command_block
                && trimmed
                    .chars()
                    .next()
                    .is_some_and(|c| c == '-' || c.is_whitespace()));

        // Non-command lines (like "For more information...") - just output as-is
        if !is_command_line && !in_command_block {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // Track if line ends with continuation
        let has_continuation = line.trim_end().ends_with('\\');
        in_command_block = has_continuation;

        // Preserve leading whitespace
        let leading_spaces = line.len() - trimmed.len();
        result.push_str(&" ".repeat(leading_spaces));

        // Simple tokenization for shell highlighting
        let mut chars = trimmed.chars().peekable();
        let mut current_token = String::new();
        let mut in_string = false;
        let mut string_char = ' ';
        let mut is_first_token = trimmed.starts_with("mergers");

        while let Some(ch) = chars.next() {
            match ch {
                '"' | '\'' if !in_string => {
                    // Flush current token
                    if !current_token.is_empty() {
                        if is_first_token {
                            result.push_str(&format!("{command_style}{current_token}{reset:#}"));
                            is_first_token = false;
                        } else if current_token.starts_with('-') {
                            result.push_str(&format!("{flag_style}{current_token}{reset:#}"));
                        } else {
                            result.push_str(&current_token);
                        }
                        current_token.clear();
                    }
                    // Start string
                    in_string = true;
                    string_char = ch;
                    current_token.push(ch);
                }
                c if c == string_char && in_string => {
                    // End string
                    current_token.push(ch);
                    result.push_str(&format!("{string_style}{current_token}{reset:#}"));
                    current_token.clear();
                    in_string = false;
                }
                ' ' | '\t' if !in_string => {
                    // Token boundary
                    if !current_token.is_empty() {
                        if is_first_token {
                            result.push_str(&format!("{command_style}{current_token}{reset:#}"));
                            is_first_token = false;
                        } else if current_token.starts_with('-') {
                            result.push_str(&format!("{flag_style}{current_token}{reset:#}"));
                        } else if current_token.starts_with('<') && current_token.ends_with('>') {
                            result.push_str(&format!("{string_style}{current_token}{reset:#}"));
                        } else {
                            result.push_str(&current_token);
                        }
                        current_token.clear();
                    }
                    result.push(ch);
                }
                '\\' if chars.peek() == Some(&'\n') => {
                    // Line continuation
                    if !current_token.is_empty() {
                        if is_first_token {
                            result.push_str(&format!("{command_style}{current_token}{reset:#}"));
                            is_first_token = false;
                        } else if current_token.starts_with('-') {
                            result.push_str(&format!("{flag_style}{current_token}{reset:#}"));
                        } else {
                            result.push_str(&current_token);
                        }
                        current_token.clear();
                    }
                    result.push_str("\\\n");
                    chars.next(); // consume the newline
                }
                _ => {
                    current_token.push(ch);
                }
            }
        }

        // Flush remaining token
        if !current_token.is_empty() {
            if is_first_token {
                result.push_str(&format!("{command_style}{current_token}{reset:#}"));
            } else if current_token.starts_with('-') {
                result.push_str(&format!("{flag_style}{current_token}{reset:#}"));
            } else if current_token.starts_with('<') && current_token.ends_with('>') {
                result.push_str(&format!("{string_style}{current_token}{reset:#}"));
            } else {
                result.push_str(&current_token);
            }
        }

        result.push('\n');
    }

    result
}

/// Build styled after_help text with colorized EXAMPLES header and syntax highlighting
fn styled_examples(content: &str) -> String {
    let header_style = AnsiColor::Yellow.on_default().bold();
    let highlighted = highlight_shell(content);
    format!("{header_style}EXAMPLES:{header_style:#}\n{highlighted}")
}

/// Main command examples
fn main_examples() -> &'static str {
    use std::sync::OnceLock;
    static EXAMPLES: OnceLock<String> = OnceLock::new();
    EXAMPLES.get_or_init(|| styled_examples(include_str!("../docs/examples/main.txt")))
}

/// Merge command examples
fn merge_examples() -> &'static str {
    use std::sync::OnceLock;
    static EXAMPLES: OnceLock<String> = OnceLock::new();
    EXAMPLES.get_or_init(|| styled_examples(include_str!("../docs/examples/merge.txt")))
}

/// Migrate command examples
fn migrate_examples() -> &'static str {
    use std::sync::OnceLock;
    static EXAMPLES: OnceLock<String> = OnceLock::new();
    EXAMPLES.get_or_init(|| styled_examples(include_str!("../docs/examples/migrate.txt")))
}

/// Cleanup command examples
fn cleanup_examples() -> &'static str {
    use std::sync::OnceLock;
    static EXAMPLES: OnceLock<String> = OnceLock::new();
    EXAMPLES.get_or_init(|| styled_examples(include_str!("../docs/examples/cleanup.txt")))
}

/// Release-notes command examples
fn release_notes_examples() -> &'static str {
    use std::sync::OnceLock;
    static EXAMPLES: OnceLock<String> = OnceLock::new();
    EXAMPLES.get_or_init(|| styled_examples(include_str!("../docs/examples/release-notes.txt")))
}

/// Shared arguments used by all commands
#[derive(ClapArgs, Clone, Default, Debug)]
pub struct SharedArgs {
    /// Local repository path or alias (positional argument, takes precedence over --local-repo)
    pub path: Option<String>,

    // Azure DevOps Connection
    /// Azure DevOps organization name
    #[arg(short, long, help_heading = "Azure DevOps Connection")]
    pub organization: Option<String>,

    /// Azure DevOps project name
    #[arg(short, long, help_heading = "Azure DevOps Connection")]
    pub project: Option<String>,

    /// Azure DevOps repository name
    #[arg(short, long, help_heading = "Azure DevOps Connection")]
    pub repository: Option<String>,

    /// Personal Access Token for Azure DevOps API authentication
    #[arg(short = 't', long, help_heading = "Azure DevOps Connection")]
    pub pat: Option<String>,

    // Branch Configuration
    /// Source branch to fetch PRs from [default: dev]
    #[arg(long, help_heading = "Branch Configuration")]
    pub dev_branch: Option<String>,

    /// Target branch for cherry-picks [default: next]
    #[arg(long, help_heading = "Branch Configuration")]
    pub target_branch: Option<String>,

    // Repository Options
    /// Local repository path (alternative to positional argument)
    #[arg(long, help_heading = "Repository Options")]
    pub local_repo: Option<String>,

    /// Prefix for tagging processed PRs
    #[arg(long, default_value = "merged-", help_heading = "Repository Options")]
    pub tag_prefix: Option<String>,

    // Performance Tuning
    /// Maximum parallel API requests [default: 300]
    #[arg(long, help_heading = "Performance Tuning")]
    pub parallel_limit: Option<usize>,

    /// Maximum concurrent network operations [default: 100]
    #[arg(long, help_heading = "Performance Tuning")]
    pub max_concurrent_network: Option<usize>,

    /// Maximum concurrent processing operations [default: 10]
    #[arg(long, help_heading = "Performance Tuning")]
    pub max_concurrent_processing: Option<usize>,

    // Filtering
    /// Only fetch items created after this date (e.g., "1mo", "2w", "2025-01-15")
    #[arg(long, help_heading = "Filtering")]
    pub since: Option<String>,

    // Behavior
    /// Skip the settings confirmation screen and proceed directly
    #[arg(long, help_heading = "Behavior")]
    pub skip_confirmation: bool,

    // Logging
    /// Log level (trace, debug, info, warn, error)
    #[arg(long, help_heading = "Logging")]
    pub log_level: Option<String>,

    /// Log file path (logs to file instead of stderr)
    #[arg(long, help_heading = "Logging")]
    pub log_file: Option<String>,

    /// Log format (text, json) [default: text]
    #[arg(long, help_heading = "Logging", value_parser = ["text", "json"])]
    pub log_format: Option<String>,
}

/// Arguments specific to non-interactive mode.
/// Flattened into MergeArgs so these flags are available on `mergers merge` directly.
#[derive(ClapArgs, Clone, Default, Debug)]
pub struct NonInteractiveArgs {
    /// Run in non-interactive mode (for CI/AI agents)
    #[arg(short = 'n', long, help_heading = "Non-Interactive Mode")]
    pub non_interactive: bool,

    /// Merge branch version (required with --non-interactive)
    #[arg(long, help_heading = "Non-Interactive Mode")]
    pub version: Option<String>,

    /// Comma-separated work item states for PR filtering
    #[arg(long, help_heading = "Non-Interactive Mode")]
    pub select_by_state: Option<String>,

    /// Output format: text, json, ndjson
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, help_heading = "Output Options")]
    pub output: OutputFormat,

    /// Suppress progress output
    #[arg(short, long, help_heading = "Output Options")]
    pub quiet: bool,
}

/// Arguments specific to merge mode
#[derive(ClapArgs, Clone)]
pub struct MergeArgs {
    #[command(flatten)]
    pub shared: SharedArgs,

    #[command(flatten)]
    pub ni: NonInteractiveArgs,

    /// State to set work items to after successful merge [default: Next Merged]
    #[arg(long, help_heading = "Merge Options")]
    pub work_item_state: Option<String>,

    /// Run git hooks during cherry-pick operations (hooks are skipped by default)
    #[arg(long, help_heading = "Merge Options")]
    pub run_hooks: bool,

    /// Subcommand for non-interactive operations
    #[command(subcommand)]
    pub subcommand: Option<MergeSubcommand>,
}

/// Arguments specific to migration mode
#[derive(ClapArgs, Clone)]
pub struct MigrateArgs {
    #[command(flatten)]
    pub shared: SharedArgs,

    /// Comma-separated list of work item states considered terminal
    #[arg(
        long,
        default_value = "Closed,Next Closed,Next Merged",
        help_heading = "Migration Options"
    )]
    pub terminal_states: String,
}

/// Arguments specific to cleanup mode
#[derive(ClapArgs, Clone)]
pub struct CleanupArgs {
    #[command(flatten)]
    pub shared: SharedArgs,

    /// Target branch to check for merged patches (defaults to --target-branch)
    #[arg(long, help_heading = "Cleanup Options")]
    pub target: Option<String>,
}

// ============================================================================
// Non-Interactive Merge Mode CLI Arguments
// ============================================================================

/// Output format for non-interactive mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    /// Human-readable text output.
    #[default]
    Text,
    /// JSON summary at the end.
    Json,
    /// Newline-delimited JSON (one event per line).
    Ndjson,
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Text => write!(f, "text"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Ndjson => write!(f, "ndjson"),
        }
    }
}

// ============================================================================
// Release Notes CLI Arguments
// ============================================================================

/// Output format for release-notes command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum, serde::Serialize)]
pub enum ReleaseNotesOutputFormat {
    /// Markdown table format.
    #[default]
    Markdown,
    /// JSON array of task objects.
    Json,
    /// Plain text list.
    Plain,
}

impl std::fmt::Display for ReleaseNotesOutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReleaseNotesOutputFormat::Markdown => write!(f, "markdown"),
            ReleaseNotesOutputFormat::Json => write!(f, "json"),
            ReleaseNotesOutputFormat::Plain => write!(f, "plain"),
        }
    }
}

/// Task grouping category based on commit message prefix.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum TaskGroup {
    /// Feature additions (feat:, feature:)
    Feature,
    /// Bug fixes (fix:, bugfix:)
    Fix,
    /// Code refactoring (refactor:)
    Refactor,
    /// Other changes
    #[default]
    Other,
}

impl std::fmt::Display for TaskGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskGroup::Feature => write!(f, "Features"),
            TaskGroup::Fix => write!(f, "Fixes"),
            TaskGroup::Refactor => write!(f, "Refactors"),
            TaskGroup::Other => write!(f, "Other"),
        }
    }
}

/// Arguments for the release-notes command.
#[derive(ClapArgs, Clone, Debug)]
pub struct ReleaseNotesArgs {
    #[command(flatten)]
    pub shared: SharedArgs,

    /// Output format: markdown, json, plain
    #[arg(long, value_enum, default_value_t = ReleaseNotesOutputFormat::Markdown, help_heading = "Output Options")]
    pub output: ReleaseNotesOutputFormat,

    /// Copy output to clipboard
    #[arg(long, help_heading = "Output Options")]
    pub copy: bool,

    /// Group tasks by commit type (feat, fix, refactor)
    #[arg(long, help_heading = "Output Options")]
    pub group: bool,

    /// Include PR links in output
    #[arg(long, help_heading = "Output Options")]
    pub include_prs: bool,

    /// Starting version/tag for range (inclusive)
    #[arg(long, help_heading = "Version Range")]
    pub from: Option<String>,

    /// Ending version/tag for range (inclusive, defaults to HEAD)
    #[arg(long, help_heading = "Version Range")]
    pub to: Option<String>,

    /// Skip cache and fetch fresh data from API
    #[arg(long, help_heading = "Cache Options")]
    pub no_cache: bool,
}

/// Arguments for the `merge continue` subcommand.
#[derive(ClapArgs, Clone, Debug)]
pub struct MergeContinueArgs {
    /// Repository path (auto-detected if in repo)
    #[arg(long, help_heading = "Repository")]
    pub repo: Option<String>,

    /// Output format: text, json, ndjson
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, help_heading = "Output Options")]
    pub output: OutputFormat,

    /// Suppress progress output
    #[arg(short, long, help_heading = "Output Options")]
    pub quiet: bool,
}

/// Arguments for the `merge abort` subcommand.
#[derive(ClapArgs, Clone, Debug)]
pub struct MergeAbortArgs {
    /// Repository path (auto-detected if in repo)
    #[arg(long, help_heading = "Repository")]
    pub repo: Option<String>,

    /// Output format: text, json, ndjson
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, help_heading = "Output Options")]
    pub output: OutputFormat,
}

/// Arguments for the `merge status` subcommand.
#[derive(ClapArgs, Clone, Debug)]
pub struct MergeStatusArgs {
    /// Repository path (auto-detected if in repo)
    #[arg(long, help_heading = "Repository")]
    pub repo: Option<String>,

    /// Output format: text, json, ndjson
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, help_heading = "Output Options")]
    pub output: OutputFormat,
}

/// Arguments for the `merge complete` subcommand.
#[derive(ClapArgs, Clone, Debug)]
pub struct MergeCompleteArgs {
    /// Repository path (auto-detected if in repo)
    #[arg(long, help_heading = "Repository")]
    pub repo: Option<String>,

    /// State to set work items to (required)
    #[arg(long, help_heading = "Completion Options")]
    pub next_state: String,

    /// Output format: text, json, ndjson
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, help_heading = "Output Options")]
    pub output: OutputFormat,

    /// Suppress progress output
    #[arg(short, long, help_heading = "Output Options")]
    pub quiet: bool,
}

/// Subcommands for the merge mode.
#[derive(Subcommand, Clone, Debug)]
pub enum MergeSubcommand {
    /// Continue merge after resolving conflicts
    #[command(
        about = "Continue merge after resolving conflicts",
        long_about = "Continue a merge operation that was paused due to conflicts.\n\n\
            This command reads the state file, verifies conflicts are resolved,\n\
            and continues cherry-picking remaining commits."
    )]
    Continue(MergeContinueArgs),

    /// Abort and clean up an in-progress merge
    #[command(
        about = "Abort and clean up an in-progress merge",
        long_about = "Abort an in-progress merge operation and clean up.\n\n\
            This removes the worktree, deletes the working branch, and aborts\n\
            any in-progress cherry-pick."
    )]
    Abort(MergeAbortArgs),

    /// Show status of current merge operation
    #[command(
        about = "Show status of current merge operation",
        long_about = "Show the current status of an in-progress merge operation.\n\n\
            Displays the current phase, progress, and any conflicts."
    )]
    Status(MergeStatusArgs),

    /// Complete merge by tagging PRs and updating work items
    #[command(
        about = "Complete merge by tagging PRs and updating work items",
        long_about = "Complete a merge operation after all cherry-picks are done.\n\n\
            This tags successful PRs in Azure DevOps and updates work items\n\
            to the specified next state."
    )]
    Complete(MergeCompleteArgs),
}

/// Trait to extract shared arguments from command-specific argument structs
pub trait HasSharedArgs {
    fn shared_args(&self) -> &SharedArgs;
    fn shared_args_mut(&mut self) -> &mut SharedArgs;
}

impl HasSharedArgs for MergeArgs {
    fn shared_args(&self) -> &SharedArgs {
        &self.shared
    }

    fn shared_args_mut(&mut self) -> &mut SharedArgs {
        &mut self.shared
    }
}

impl HasSharedArgs for MigrateArgs {
    fn shared_args(&self) -> &SharedArgs {
        &self.shared
    }

    fn shared_args_mut(&mut self) -> &mut SharedArgs {
        &mut self.shared
    }
}

impl HasSharedArgs for CleanupArgs {
    fn shared_args(&self) -> &SharedArgs {
        &self.shared
    }

    fn shared_args_mut(&mut self) -> &mut SharedArgs {
        &mut self.shared
    }
}

impl HasSharedArgs for ReleaseNotesArgs {
    fn shared_args(&self) -> &SharedArgs {
        &self.shared
    }

    fn shared_args_mut(&mut self) -> &mut SharedArgs {
        &mut self.shared
    }
}

/// Available commands
#[derive(Subcommand, Clone)]
pub enum Commands {
    /// Cherry-pick merged PRs from dev branch to target branch
    #[command(
        visible_alias = "m",
        long_about = "Cherry-pick merged PRs from the dev branch to a target branch.\n\n\
            This mode fetches completed PRs from Azure DevOps, displays them in an interactive\n\
            TUI for selection, and cherry-picks the selected commits to your target branch.\n\
            Work items can be automatically transitioned to a specified state after merge.",
        after_help = merge_examples()
    )]
    Merge(MergeArgs),

    /// Analyze PRs to determine migration eligibility
    #[command(
        visible_alias = "mi",
        long_about = "Analyze pull requests to determine which ones are eligible for migration.\n\n\
            This mode examines PRs and their associated work items to categorize them as:\n  \
            • Eligible: All work items in terminal states, commit found in target\n  \
            • Unsure: Mixed signals requiring manual review\n  \
            • Not merged: PR commits not present in target branch\n\n\
            Results are displayed in an interactive TUI for review and manual override.",
        after_help = migrate_examples()
    )]
    Migrate(MigrateArgs),

    /// Clean up merged patch branches from the repository
    #[command(
        visible_alias = "c",
        long_about = "Clean up patch branches that have been merged to the target branch.\n\n\
            This mode identifies local branches matching the tag prefix pattern (default: merged-*)\n\
            that have been fully merged into the target branch, and offers to delete them.\n\
            Useful for maintaining a clean repository after completing merge operations.",
        after_help = cleanup_examples()
    )]
    Cleanup(CleanupArgs),

    /// Generate release notes from version commits
    #[command(
        visible_alias = "rn",
        long_about = "Generate release notes from git tags and pull requests.\n\n\
            Discovers PRs tagged with a configurable prefix (tag_prefix) and fetches\n\
            associated work items from Azure DevOps to build formatted release notes.\n\n\
            Features:\n  \
            • Supports version ranges (--from / --to)\n  \
            • Groups entries by type (feat, fix, refactor)\n  \
            • Caches work item titles locally\n  \
            • Multiple output formats (markdown, json, plain)\n  \
            • Clipboard support (--copy)",
        after_help = release_notes_examples()
    )]
    ReleaseNotes(ReleaseNotesArgs),
}

impl Commands {
    /// Extract shared arguments from any command variant.
    pub fn shared_args(&self) -> &SharedArgs {
        match self {
            Commands::Merge(args) => args.shared_args(),
            Commands::Migrate(args) => args.shared_args(),
            Commands::Cleanup(args) => args.shared_args(),
            Commands::ReleaseNotes(args) => args.shared_args(),
        }
    }

    /// Extract mutable shared arguments from any command variant.
    pub fn shared_args_mut(&mut self) -> &mut SharedArgs {
        match self {
            Commands::Merge(args) => args.shared_args_mut(),
            Commands::Migrate(args) => args.shared_args_mut(),
            Commands::Cleanup(args) => args.shared_args_mut(),
            Commands::ReleaseNotes(args) => args.shared_args_mut(),
        }
    }

    /// Check if this command is the ReleaseNotes command.
    #[must_use]
    pub fn is_release_notes(&self) -> bool {
        matches!(self, Commands::ReleaseNotes(_))
    }
}

#[derive(Parser, Clone)]
#[command(
    name = "mergers",
    author,
    version,
    long_version = build_version(),
    about = "Manage Azure DevOps pull request merging and migration workflows",
    long_about = "A CLI/TUI tool for managing Azure DevOps pull request merging and migration workflows.\n\n\
        Mergers helps you:\n  \
        • Cherry-pick merged PRs from dev to target branches\n  \
        • Analyze PRs for migration eligibility\n  \
        • Clean up merged patch branches\n\n\
        Configuration can be provided via CLI arguments, environment variables (MERGERS_*),\n\
        config file (~/.config/mergers/config.toml), or auto-detected from git remotes.",
    before_help = concat!("mergers ", env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")"),
    after_help = main_examples(),
    styles = help_styles()
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Create a sample configuration file at ~/.config/mergers/config.toml
    #[arg(long)]
    pub create_config: bool,
}

/// Temporary wrapper to parse MergeArgs as if they were top-level
#[derive(Parser, Clone)]
#[command(
    name = "mergers",
    about = None,
    long_about = None,
    before_help = concat!("mergers ", env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")"),
    styles = help_styles()
)]
pub struct MergeArgsParser {
    #[command(flatten)]
    pub merge_args: MergeArgs,
}

impl Args {
    /// Parse arguments with default mode fallback.
    /// If no subcommand is provided, attempts to parse args as MergeArgs.
    pub fn parse_with_default_mode() -> Self {
        use clap::error::ErrorKind;

        // First try normal parsing
        match Args::try_parse() {
            Ok(args) => {
                // Successfully parsed as Args, check if command is present
                if args.command.is_some() || args.create_config {
                    return args;
                }
                // No command and no create_config, fall through to try merge mode
            }
            Err(e) => {
                // If it's a help or version display, show it and exit
                match e.kind() {
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                        e.exit();
                    }
                    _ => {
                        // Other errors: fall through to try merge mode
                    }
                }
            }
        }

        // Try to parse as MergeArgs using the wrapper
        match MergeArgsParser::try_parse() {
            Ok(parser) => Args {
                command: Some(Commands::Merge(parser.merge_args)),
                create_config: false,
            },
            Err(e) => {
                // If MergeArgs parsing also fails, show the error and exit
                e.exit();
            }
        }
    }
}

/// Shared configuration used by both modes
#[derive(Debug, Clone)]
pub struct SharedConfig {
    pub organization: ParsedProperty<String>,
    pub project: ParsedProperty<String>,
    pub repository: ParsedProperty<String>,
    pub pat: ParsedProperty<String>,
    pub dev_branch: ParsedProperty<String>,
    pub target_branch: ParsedProperty<String>,
    pub local_repo: Option<ParsedProperty<String>>,
    pub parallel_limit: ParsedProperty<usize>,
    pub max_concurrent_network: ParsedProperty<usize>,
    pub max_concurrent_processing: ParsedProperty<usize>,
    pub tag_prefix: ParsedProperty<String>,
    pub since: Option<ParsedProperty<DateTime<Utc>>>,
    pub skip_confirmation: bool,
}

/// Configuration specific to default mode
#[derive(Debug, Clone)]
pub struct DefaultModeConfig {
    pub work_item_state: ParsedProperty<String>,
    /// Whether to run git hooks during cherry-pick operations (default: false).
    /// When false, hooks are disabled at repo initialization by setting core.hooksPath=/dev/null.
    pub run_hooks: ParsedProperty<bool>,
}

/// Configuration specific to migration mode
#[derive(Debug, Clone)]
pub struct MigrationModeConfig {
    pub terminal_states: ParsedProperty<Vec<String>>,
}

/// Configuration specific to cleanup mode
#[derive(Debug, Clone)]
pub struct CleanupModeConfig {
    pub target: ParsedProperty<String>,
}

/// Configuration specific to release notes mode
#[derive(Debug, Clone)]
pub struct ReleaseNotesModeConfig {
    pub from_version: Option<String>,
    pub to_version: Option<String>,
    pub output_format: ReleaseNotesOutputFormat,
    pub grouped: bool,
    pub include_prs: bool,
    pub copy_to_clipboard: bool,
    pub no_cache: bool,
}

// ============================================================================
// Type-Safe App Configuration System
// ============================================================================

/// Trait for mode-specific configurations with compile-time type safety.
///
/// This trait is implemented by each mode-specific config struct, providing
/// a common interface for accessing shared configuration while ensuring
/// type-safe access to mode-specific fields.
///
/// # Example
///
/// ```ignore
/// fn process_config<C: AppModeConfig>(config: &C) {
///     let org = config.shared().organization.value();
///     // Access shared config...
/// }
/// ```
pub trait AppModeConfig: Clone + Send + Sync + std::fmt::Debug {
    /// Returns a reference to the shared configuration.
    fn shared(&self) -> &SharedConfig;
}

/// Complete configuration for merge mode with compile-time type safety.
///
/// This struct combines `SharedConfig` with merge-specific settings,
/// providing type-safe access without runtime pattern matching.
#[derive(Debug, Clone)]
pub struct MergeConfig {
    /// Shared configuration common to all modes.
    pub shared: SharedConfig,
    /// State to set work items to after successful merge.
    pub work_item_state: ParsedProperty<String>,
    /// Whether to run git hooks during cherry-pick operations (default: false).
    pub run_hooks: ParsedProperty<bool>,
}

impl AppModeConfig for MergeConfig {
    fn shared(&self) -> &SharedConfig {
        &self.shared
    }
}

impl MergeConfig {
    /// Converts to AppConfig for backward compatibility with state types.
    pub fn to_app_config(&self) -> AppConfig {
        AppConfig::Default {
            shared: self.shared.clone(),
            default: DefaultModeConfig {
                work_item_state: self.work_item_state.clone(),
                run_hooks: self.run_hooks.clone(),
            },
        }
    }
}

/// Complete configuration for migration mode with compile-time type safety.
///
/// This struct combines `SharedConfig` with migration-specific settings,
/// providing type-safe access without runtime pattern matching.
#[derive(Debug, Clone)]
pub struct MigrationConfig {
    /// Shared configuration common to all modes.
    pub shared: SharedConfig,
    /// Work item states that indicate completion.
    pub terminal_states: ParsedProperty<Vec<String>>,
}

impl AppModeConfig for MigrationConfig {
    fn shared(&self) -> &SharedConfig {
        &self.shared
    }
}

impl MigrationConfig {
    /// Converts to AppConfig for backward compatibility with state types.
    pub fn to_app_config(&self) -> AppConfig {
        AppConfig::Migration {
            shared: self.shared.clone(),
            migration: MigrationModeConfig {
                terminal_states: self.terminal_states.clone(),
            },
        }
    }
}

/// Complete configuration for cleanup mode with compile-time type safety.
///
/// This struct combines `SharedConfig` with cleanup-specific settings,
/// providing type-safe access without runtime pattern matching.
#[derive(Debug, Clone)]
pub struct CleanupConfig {
    /// Shared configuration common to all modes.
    pub shared: SharedConfig,
    /// Target branch to check for merged patches.
    pub target: ParsedProperty<String>,
}

impl AppModeConfig for CleanupConfig {
    fn shared(&self) -> &SharedConfig {
        &self.shared
    }
}

impl CleanupConfig {
    /// Converts to AppConfig for backward compatibility with state types.
    pub fn to_app_config(&self) -> AppConfig {
        AppConfig::Cleanup {
            shared: self.shared.clone(),
            cleanup: CleanupModeConfig {
                target: self.target.clone(),
            },
        }
    }
}

// ============================================================================
// Legacy AppConfig Enum (for backward compatibility)
// ============================================================================

/// Resolved configuration with mode-specific settings.
///
/// This enum is maintained for backward compatibility. New code should prefer
/// the type-safe config structs ([`MergeConfig`], [`MigrationConfig`], [`CleanupConfig`]).
#[derive(Debug, Clone)]
pub enum AppConfig {
    Default {
        shared: SharedConfig,
        default: DefaultModeConfig,
    },
    Migration {
        shared: SharedConfig,
        migration: MigrationModeConfig,
    },
    Cleanup {
        shared: SharedConfig,
        cleanup: CleanupModeConfig,
    },
    ReleaseNotes {
        shared: SharedConfig,
        release_notes: ReleaseNotesModeConfig,
    },
}

impl AppConfig {
    pub fn shared(&self) -> &SharedConfig {
        match self {
            AppConfig::Default { shared, .. }
            | AppConfig::Migration { shared, .. }
            | AppConfig::Cleanup { shared, .. }
            | AppConfig::ReleaseNotes { shared, .. } => shared,
        }
    }

    pub fn is_migration_mode(&self) -> bool {
        matches!(self, AppConfig::Migration { .. })
    }

    pub fn is_cleanup_mode(&self) -> bool {
        matches!(self, AppConfig::Cleanup { .. })
    }

    /// Converts to MergeConfig if this is a Default variant.
    ///
    /// # Panics
    ///
    /// Panics if called on a non-Default variant.
    pub fn into_merge_config(self) -> MergeConfig {
        match self {
            AppConfig::Default { shared, default } => MergeConfig {
                shared,
                work_item_state: default.work_item_state,
                run_hooks: default.run_hooks,
            },
            _ => panic!("into_merge_config called on non-Default variant"),
        }
    }

    /// Converts to MigrationConfig if this is a Migration variant.
    ///
    /// # Panics
    ///
    /// Panics if called on a non-Migration variant.
    pub fn into_migration_config(self) -> MigrationConfig {
        match self {
            AppConfig::Migration { shared, migration } => MigrationConfig {
                shared,
                terminal_states: migration.terminal_states,
            },
            _ => panic!("into_migration_config called on non-Migration variant"),
        }
    }

    /// Converts to CleanupConfig if this is a Cleanup variant.
    ///
    /// # Panics
    ///
    /// Panics if called on a non-Cleanup variant.
    pub fn into_cleanup_config(self) -> CleanupConfig {
        match self {
            AppConfig::Cleanup { shared, cleanup } => CleanupConfig {
                shared,
                target: cleanup.target,
            },
            _ => panic!("into_cleanup_config called on non-Cleanup variant"),
        }
    }

    /// Converts to ReleaseNotesRunnerConfig if this is a ReleaseNotes variant.
    ///
    /// # Panics
    ///
    /// Panics if called on a non-ReleaseNotes variant.
    /// Converts to ReleaseNotesRunnerConfig if this is a ReleaseNotes variant.
    ///
    /// # Panics
    ///
    /// Panics if called on a non-ReleaseNotes variant.
    pub fn into_release_notes_runner_config(
        self,
    ) -> crate::core::runner::release_notes::ReleaseNotesRunnerConfig {
        match self {
            AppConfig::ReleaseNotes {
                shared,
                release_notes,
            } => crate::core::runner::release_notes::ReleaseNotesRunnerConfig {
                organization: shared.organization.value().clone(),
                project: shared.project.value().clone(),
                repository: shared.repository.value().clone(),
                pat: shared.pat.value().clone(),
                dev_branch: shared.dev_branch.value().clone(),
                tag_prefix: shared.tag_prefix.value().clone(),
                from_version: release_notes.from_version,
                to_version: release_notes.to_version,
                output_format: release_notes.output_format,
                grouped: release_notes.grouped,
                include_prs: release_notes.include_prs,
                copy_to_clipboard: release_notes.copy_to_clipboard,
                no_cache: release_notes.no_cache,
                max_concurrent_network: *shared.max_concurrent_network.value(),
                max_concurrent_processing: *shared.max_concurrent_processing.value(),
            },
            _ => panic!("into_release_notes_runner_config called on non-ReleaseNotes variant"),
        }
    }

    /// Tries to convert to MergeConfig, returning None if not a Default variant.
    pub fn try_into_merge_config(self) -> Option<MergeConfig> {
        match self {
            AppConfig::Default { shared, default } => Some(MergeConfig {
                shared,
                work_item_state: default.work_item_state,
                run_hooks: default.run_hooks,
            }),
            _ => None,
        }
    }

    /// Tries to convert to MigrationConfig, returning None if not a Migration variant.
    pub fn try_into_migration_config(self) -> Option<MigrationConfig> {
        match self {
            AppConfig::Migration { shared, migration } => Some(MigrationConfig {
                shared,
                terminal_states: migration.terminal_states,
            }),
            _ => None,
        }
    }

    /// Tries to convert to CleanupConfig, returning None if not a Cleanup variant.
    pub fn try_into_cleanup_config(self) -> Option<CleanupConfig> {
        match self {
            AppConfig::Cleanup { shared, cleanup } => Some(CleanupConfig {
                shared,
                target: cleanup.target,
            }),
            _ => None,
        }
    }
}

/// Implement AppModeConfig for AppConfig to allow backward compatibility.
///
/// This allows existing code using `AppBase` with `Arc<AppConfig>` to continue
/// working while new code can use the type-safe config structs.
impl AppModeConfig for AppConfig {
    fn shared(&self) -> &SharedConfig {
        match self {
            AppConfig::Default { shared, .. }
            | AppConfig::Migration { shared, .. }
            | AppConfig::Cleanup { shared, .. }
            | AppConfig::ReleaseNotes { shared, .. } => shared,
        }
    }
}

impl Args {
    /// Resolve configuration from CLI args, environment variables, config file, and git remote
    /// Priority: CLI args > environment variables > git remote > config file > defaults
    pub fn resolve_config(self) -> Result<AppConfig> {
        // Destructure self to extract command
        let Args {
            command,
            create_config: _,
        } = self;

        // Use command or default to merge mode
        let mode_command = command.unwrap_or_else(|| {
            Commands::Merge(MergeArgs {
                shared: SharedArgs::default(),
                ni: NonInteractiveArgs::default(),
                work_item_state: None,
                run_hooks: false,
                subcommand: None,
            })
        });

        // Access shared args through the command using the trait
        let shared = mode_command.shared_args();

        // Determine local_repo path from CLI (positional arg takes precedence over --local-repo flag)
        let cli_local_repo = shared.path.as_ref().or(shared.local_repo.as_ref());

        // Load from config file (lowest priority)
        let file_config = Config::load_from_file()?;

        // Load from environment variables
        let env_config = Config::load_from_env();

        // Resolve repo aliases for all commands (supports path or alias via SharedArgs.path)
        let repo_aliases = file_config.repo_aliases.as_ref().map(|p| p.value().clone());
        let resolved_local_repo = cli_local_repo.and_then(|path| {
            crate::config::resolve_repo_path(Some(path), &repo_aliases)
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        });

        // Determine effective local_repo path for git detection
        // CLI (resolved via aliases) takes precedence, then env var, then config file
        let effective_local_repo = resolved_local_repo.or_else(|| {
            env_config
                .local_repo
                .as_ref()
                .map(|p| p.value().clone())
                .or_else(|| file_config.local_repo.as_ref().map(|p| p.value().clone()))
        });

        // Try to detect from git remote if we have a local repo path from any source
        let git_config = if let Some(ref repo_path) = effective_local_repo {
            Config::detect_from_git_remote(repo_path)
        } else {
            Config::default()
        };

        let cli_config = Config::from_shared_args(shared);

        // Merge configs: file < git_remote < env < cli
        let merged_config = file_config
            .merge(git_config)
            .merge(env_config)
            .merge(cli_config);

        // Validate required shared fields
        let organization = merged_config.organization
            .ok_or_else(|| anyhow::anyhow!("organization is required (use --organization, MERGERS_ORGANIZATION env var, or config file)"))?;
        let project = merged_config.project.ok_or_else(|| {
            anyhow::anyhow!(
                "project is required (use --project, MERGERS_PROJECT env var, or config file)"
            )
        })?;
        let repository = merged_config.repository
            .ok_or_else(|| anyhow::anyhow!("repository is required (use --repository, MERGERS_REPOSITORY env var, or config file)"))?;
        let pat = merged_config.pat.ok_or_else(|| {
            anyhow::anyhow!("pat is required (use --pat, MERGERS_PAT env var, or config file)")
        })?;

        // Handle since field parsing
        let since = if let Some(since_str) = &shared.since {
            let parsed_date = parse_since_date(since_str)
                .with_context(|| format!("Failed to parse since date: {}", since_str))?;
            Some(ParsedProperty::Cli(parsed_date, since_str.clone()))
        } else {
            None
        };

        let shared_config = SharedConfig {
            organization,
            project,
            repository,
            pat,
            dev_branch: merged_config
                .dev_branch
                .unwrap_or_else(|| "dev".to_string().into()),
            target_branch: merged_config
                .target_branch
                .unwrap_or_else(|| "next".to_string().into()),
            local_repo: merged_config.local_repo,
            parallel_limit: merged_config.parallel_limit.unwrap_or(300.into()),
            max_concurrent_network: merged_config.max_concurrent_network.unwrap_or(100.into()),
            max_concurrent_processing: merged_config.max_concurrent_processing.unwrap_or(10.into()),
            tag_prefix: merged_config
                .tag_prefix
                .unwrap_or_else(|| "merged-".to_string().into()),
            since,
            skip_confirmation: shared.skip_confirmation,
        };

        // Return appropriate configuration based on command
        match mode_command {
            Commands::Migrate(migrate_args) => {
                // Parse terminal states from CLI
                let terminal_states_parsed = crate::api::AzureDevOpsClient::parse_terminal_states(
                    &migrate_args.terminal_states,
                );
                Ok(AppConfig::Migration {
                    shared: shared_config,
                    migration: MigrationModeConfig {
                        terminal_states: ParsedProperty::Cli(
                            terminal_states_parsed,
                            migrate_args.terminal_states,
                        ),
                    },
                })
            }
            Commands::Merge(merge_args) => Ok(AppConfig::Default {
                shared: shared_config,
                default: DefaultModeConfig {
                    work_item_state: match merge_args.work_item_state {
                        Some(state) => ParsedProperty::Cli(state.clone(), state),
                        None => merged_config
                            .work_item_state
                            .unwrap_or_else(|| ParsedProperty::Default("Next Merged".to_string())),
                    },
                    run_hooks: if merge_args.run_hooks {
                        ParsedProperty::Cli(true, "true".to_string())
                    } else {
                        merged_config
                            .run_hooks
                            .unwrap_or(ParsedProperty::Default(false))
                    },
                },
            }),
            Commands::Cleanup(cleanup_args) => {
                let target = cleanup_args
                    .target
                    .map(|t| ParsedProperty::Cli(t.clone(), t))
                    .or_else(|| Some(shared_config.target_branch.clone()))
                    .unwrap();
                Ok(AppConfig::Cleanup {
                    shared: shared_config,
                    cleanup: CleanupModeConfig { target },
                })
            }
            Commands::ReleaseNotes(rn_args) => Ok(AppConfig::ReleaseNotes {
                shared: shared_config,
                release_notes: ReleaseNotesModeConfig {
                    from_version: rn_args.from.clone(),
                    to_version: rn_args.to.clone(),
                    output_format: rn_args.output,
                    grouped: rn_args.group,
                    include_prs: rn_args.include_prs,
                    copy_to_clipboard: rn_args.copy,
                    no_cache: rn_args.no_cache,
                },
            }),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRequest {
    #[serde(rename = "pullRequestId")]
    pub id: i32,
    pub title: String,
    #[serde(rename = "closedDate")]
    pub closed_date: Option<String>,
    #[serde(rename = "createdBy")]
    pub created_by: CreatedBy,
    #[serde(rename = "lastMergeCommit")]
    pub last_merge_commit: Option<MergeCommit>,
    pub labels: Option<Vec<Label>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatedBy {
    #[serde(rename = "displayName")]
    pub display_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MergeCommit {
    #[serde(rename = "commitId")]
    pub commit_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Label {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemRef {
    pub id: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkItem {
    pub id: i32,
    pub fields: WorkItemFields,
    #[serde(skip_deserializing, default)]
    pub history: Vec<WorkItemHistory>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemFields {
    #[serde(rename = "System.Title")]
    pub title: Option<String>,
    #[serde(rename = "System.State")]
    pub state: Option<String>,
    #[serde(rename = "System.WorkItemType", default)]
    pub work_item_type: Option<String>,
    #[serde(rename = "System.AssignedTo", default)]
    pub assigned_to: Option<CreatedBy>,
    #[serde(rename = "System.IterationPath", default)]
    pub iteration_path: Option<String>,
    #[serde(rename = "System.Description", default)]
    pub description: Option<String>,
    #[serde(rename = "Microsoft.VSTS.TCM.ReproSteps", default)]
    pub repro_steps: Option<String>,
    /// State color as RGB tuple (r, g, b), populated from Azure DevOps API
    #[serde(skip_deserializing, default)]
    pub state_color: Option<(u8, u8, u8)>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemHistory {
    pub rev: i32,
    #[serde(rename = "revisedDate")]
    pub revised_date: String,
    #[serde(rename = "fields")]
    pub fields: Option<WorkItemHistoryFields>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemHistoryFields {
    #[serde(rename = "System.State")]
    pub state: Option<WorkItemFieldChange>,
    #[serde(rename = "System.ChangedDate")]
    pub changed_date: Option<WorkItemFieldChange>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemFieldChange {
    #[serde(rename = "newValue")]
    pub new_value: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RepoDetails {
    #[serde(rename = "sshUrl")]
    pub ssh_url: String,
}

#[derive(Debug, Clone)]
pub struct PullRequestWithWorkItems {
    pub pr: PullRequest,
    pub work_items: Vec<WorkItem>,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub enum CherryPickStatus {
    Pending,
    InProgress,
    Success,
    Conflict,
    Skipped,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct CherryPickItem {
    pub commit_id: String,
    pub pr_id: i32,
    pub pr_title: String,
    pub status: CherryPickStatus,
}

#[derive(Debug, Clone)]
pub struct MigrationAnalysis {
    pub eligible_prs: Vec<PullRequestWithWorkItems>,
    pub unsure_prs: Vec<PullRequestWithWorkItems>,
    pub not_merged_prs: Vec<PullRequestWithWorkItems>,
    pub terminal_states: Vec<String>,
    pub unsure_details: Vec<PRAnalysisResult>,
    pub all_details: Vec<PRAnalysisResult>,
    pub manual_overrides: ManualOverrides,
}

#[derive(Debug, Clone, Default)]
pub struct ManualOverrides {
    pub marked_as_eligible: std::collections::HashSet<i32>, // PR IDs manually marked as eligible
    pub marked_as_not_eligible: std::collections::HashSet<i32>, // PR IDs manually marked as not eligible
}

#[derive(Debug, Clone)]
pub struct PRAnalysisResult {
    pub pr: PullRequestWithWorkItems,
    pub all_work_items_terminal: bool,
    pub commit_in_target: bool,
    pub commit_title_in_target: bool,
    pub unsure_reason: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CleanupBranch {
    pub name: String,
    pub target: String,
    pub version: String,
    pub is_merged: bool,
    pub selected: bool,
    pub status: CleanupStatus,
}

#[derive(Debug, Clone)]
pub enum CleanupStatus {
    Pending,
    InProgress,
    Success,
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_sample_args() -> Args {
        Args {
            command: Some(Commands::Merge(MergeArgs {
                shared: SharedArgs {
                    path: Some("/test/repo".to_string()),
                    organization: Some("test-org".to_string()),
                    project: Some("test-project".to_string()),
                    repository: Some("test-repo".to_string()),
                    pat: Some("test-pat".to_string()),
                    dev_branch: Some("dev".to_string()),
                    target_branch: Some("main".to_string()),
                    local_repo: None,
                    tag_prefix: Some("merged-".to_string()),
                    parallel_limit: Some(50),
                    max_concurrent_network: Some(20),
                    max_concurrent_processing: Some(5),
                    since: Some("1w".to_string()),
                    skip_confirmation: true,
                    log_level: None,
                    log_file: None,
                    log_format: None,
                },
                ni: NonInteractiveArgs::default(),
                work_item_state: Some("Done".to_string()),
                run_hooks: false,
                subcommand: None,
            })),
            create_config: false,
        }
    }

    fn create_sample_migrate_args() -> Args {
        Args {
            command: Some(Commands::Migrate(MigrateArgs {
                shared: SharedArgs {
                    path: Some("/test/repo".to_string()),
                    organization: Some("test-org".to_string()),
                    project: Some("test-project".to_string()),
                    repository: Some("test-repo".to_string()),
                    pat: Some("test-pat".to_string()),
                    dev_branch: Some("dev".to_string()),
                    target_branch: Some("main".to_string()),
                    local_repo: None,
                    tag_prefix: Some("merged-".to_string()),
                    parallel_limit: Some(50),
                    max_concurrent_network: Some(20),
                    max_concurrent_processing: Some(5),
                    since: Some("1w".to_string()),
                    skip_confirmation: true,
                    log_level: None,
                    log_file: None,
                    log_format: None,
                },
                terminal_states: "Closed,Done".to_string(),
            })),
            create_config: false,
        }
    }

    fn create_sample_release_notes_args() -> Args {
        Args {
            command: Some(Commands::ReleaseNotes(ReleaseNotesArgs {
                shared: SharedArgs {
                    path: Some("/test/repo".to_string()),
                    organization: Some("test-org".to_string()),
                    project: Some("test-project".to_string()),
                    repository: Some("test-repo".to_string()),
                    pat: Some("test-pat".to_string()),
                    dev_branch: Some("dev".to_string()),
                    target_branch: Some("main".to_string()),
                    local_repo: None,
                    tag_prefix: Some("merged-".to_string()),
                    parallel_limit: Some(50),
                    max_concurrent_network: Some(20),
                    max_concurrent_processing: Some(5),
                    since: Some("1w".to_string()),
                    skip_confirmation: true,
                    log_level: None,
                    log_file: None,
                    log_format: None,
                },
                output: ReleaseNotesOutputFormat::Markdown,
                copy: false,
                group: false,
                include_prs: false,
                from: Some("v1.0.0".to_string()),
                to: Some("v2.0.0".to_string()),
                no_cache: false,
            })),
            create_config: false,
        }
    }

    fn create_sample_pull_request() -> PullRequest {
        PullRequest {
            id: 123,
            title: "Test PR".to_string(),
            closed_date: Some("2024-01-15T10:30:00Z".to_string()),
            created_by: CreatedBy {
                display_name: "Test User".to_string(),
            },
            last_merge_commit: Some(MergeCommit {
                commit_id: "abc123def456".to_string(),
            }),
            labels: Some(vec![Label {
                name: "feature".to_string(),
            }]),
        }
    }

    fn create_sample_work_item() -> WorkItem {
        WorkItem {
            id: 456,
            fields: WorkItemFields {
                title: Some("Test Work Item".to_string()),
                state: Some("Active".to_string()),
                work_item_type: Some("Task".to_string()),
                assigned_to: Some(CreatedBy {
                    display_name: "Assignee".to_string(),
                }),
                iteration_path: Some("Project\\Sprint 1".to_string()),
                description: Some("Test description".to_string()),
                repro_steps: Some("Steps to reproduce".to_string()),
                state_color: None,
            },
            history: vec![],
        }
    }

    // Positive test cases
    /// # Args Parsing with All Flags
    ///
    /// Tests parsing of command line arguments with all possible flags set.
    ///
    /// ## Test Scenario
    /// - Creates Args struct with all optional fields populated
    /// - Validates argument structure and field assignments
    ///
    /// ## Expected Outcome
    /// - All argument fields are correctly assigned
    /// - Args struct properly represents command line input
    #[test]
    fn test_args_parsing_with_all_flags() {
        let args = create_sample_args();

        // Check that it's in merge mode and has correct shared args
        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.path, Some("/test/repo".to_string()));
            assert_eq!(merge_args.shared.organization, Some("test-org".to_string()));
            assert_eq!(merge_args.shared.project, Some("test-project".to_string()));
            assert_eq!(merge_args.shared.repository, Some("test-repo".to_string()));
            assert_eq!(merge_args.shared.pat, Some("test-pat".to_string()));
            assert_eq!(merge_args.shared.parallel_limit, Some(50));
            assert!(merge_args.shared.skip_confirmation);
            assert_eq!(merge_args.work_item_state, Some("Done".to_string()));
        } else {
            panic!("Expected merge command");
        }
    }

    /// # Shared Config Creation
    ///
    /// Tests creation of shared configuration objects.
    ///
    /// ## Test Scenario
    /// - Creates SharedConfig with various field values
    /// - Validates field assignment and structure
    ///
    /// ## Expected Outcome
    /// - SharedConfig is created with correct field values
    /// - All required configuration fields are properly set
    #[test]
    fn test_shared_config_creation() {
        let shared = SharedConfig {
            organization: ParsedProperty::Default("test-org".to_string()),
            project: ParsedProperty::Default("test-project".to_string()),
            repository: ParsedProperty::Default("test-repo".to_string()),
            pat: ParsedProperty::Default("test-pat".to_string()),
            dev_branch: ParsedProperty::Default("dev".to_string()),
            target_branch: ParsedProperty::Default("main".to_string()),
            local_repo: Some(ParsedProperty::Default("/test/repo".to_string())),
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("merged-".to_string()),
            since: None,
            skip_confirmation: false,
        };

        assert_eq!(
            shared.organization,
            ParsedProperty::Default("test-org".to_string())
        );
        assert_eq!(shared.parallel_limit, ParsedProperty::Default(300));
        assert_eq!(shared.max_concurrent_network, ParsedProperty::Default(100));
        assert_eq!(
            shared.max_concurrent_processing,
            ParsedProperty::Default(10)
        );
    }

    /// # Default Config Creation
    ///
    /// Tests creation of default mode configuration objects.
    ///
    /// ## Test Scenario
    /// - Creates DefaultModeConfig with required parameters
    /// - Validates configuration structure and values
    ///
    /// ## Expected Outcome
    /// - DefaultModeConfig is properly created and configured
    /// - Default mode settings are correctly applied
    #[test]
    fn test_default_config_creation() {
        let default_config = DefaultModeConfig {
            work_item_state: ParsedProperty::Default("Done".to_string()),
            run_hooks: ParsedProperty::Default(false),
        };

        assert_eq!(
            default_config.work_item_state,
            ParsedProperty::Default("Done".to_string())
        );
    }

    /// # Migration Config Creation
    ///
    /// Tests creation of migration mode configuration objects.
    ///
    /// ## Test Scenario
    /// - Creates MigrationModeConfig with terminal states
    /// - Validates migration-specific configuration
    ///
    /// ## Expected Outcome
    /// - MigrationModeConfig is properly created
    /// - Migration settings are correctly configured
    #[test]
    fn test_migration_config_creation() {
        let migration_config = MigrationModeConfig {
            terminal_states: ParsedProperty::Default(vec![
                "Closed".to_string(),
                "Done".to_string(),
                "Merged".to_string(),
            ]),
        };

        assert_eq!(
            migration_config.terminal_states,
            ParsedProperty::Default(vec![
                "Closed".to_string(),
                "Done".to_string(),
                "Merged".to_string()
            ])
        );
    }

    /// # App Config Default Mode
    ///
    /// Tests AppConfig in default mode configuration.
    ///
    /// ## Test Scenario
    /// - Creates AppConfig::Default variant with shared and default configs
    /// - Tests mode detection and configuration access
    ///
    /// ## Expected Outcome
    /// - AppConfig correctly identifies as default mode
    /// - Shared configuration is accessible through the config
    #[test]
    fn test_app_config_default_mode() {
        let shared = SharedConfig {
            organization: ParsedProperty::Default("test-org".to_string()),
            project: ParsedProperty::Default("test-project".to_string()),
            repository: ParsedProperty::Default("test-repo".to_string()),
            pat: ParsedProperty::Default("test-pat".to_string()),
            dev_branch: ParsedProperty::Default("dev".to_string()),
            target_branch: ParsedProperty::Default("main".to_string()),
            local_repo: None,
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("merged-".to_string()),
            since: None,
            skip_confirmation: false,
        };

        let config = AppConfig::Default {
            shared: shared.clone(),
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Done".to_string()),
                run_hooks: ParsedProperty::Default(false),
            },
        };

        assert!(!config.is_migration_mode());
        assert_eq!(
            config.shared().organization,
            ParsedProperty::Default("test-org".to_string())
        );
    }

    /// # App Config Migration Mode
    ///
    /// Tests AppConfig in migration mode configuration.
    ///
    /// ## Test Scenario
    /// - Creates AppConfig::Migration variant with shared and migration configs
    /// - Tests mode detection and configuration access
    ///
    /// ## Expected Outcome
    /// - AppConfig correctly identifies as migration mode
    /// - Migration-specific configuration is properly accessible
    #[test]
    fn test_app_config_migration_mode() {
        let shared = SharedConfig {
            organization: ParsedProperty::Default("test-org".to_string()),
            project: ParsedProperty::Default("test-project".to_string()),
            repository: ParsedProperty::Default("test-repo".to_string()),
            pat: ParsedProperty::Default("test-pat".to_string()),
            dev_branch: ParsedProperty::Default("dev".to_string()),
            target_branch: ParsedProperty::Default("main".to_string()),
            local_repo: None,
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("merged-".to_string()),
            since: None,
            skip_confirmation: false,
        };

        let config = AppConfig::Migration {
            shared: shared.clone(),
            migration: MigrationModeConfig {
                terminal_states: ParsedProperty::Default(vec![
                    "Closed".to_string(),
                    "Done".to_string(),
                ]),
            },
        };

        assert!(config.is_migration_mode());
        assert_eq!(
            config.shared().project,
            ParsedProperty::Default("test-project".to_string())
        );
    }

    /// # Pull Request with Work Items Creation
    ///
    /// Tests creation of pull request objects with associated work items.
    ///
    /// ## Test Scenario
    /// - Creates PullRequestWithWorkItems with PR and work item data
    /// - Validates structure and data relationships
    ///
    /// ## Expected Outcome
    /// - PullRequestWithWorkItems is properly created
    /// - Work items are correctly associated with pull request
    #[test]
    fn test_pull_request_with_work_items_creation() {
        let pr = create_sample_pull_request();
        let work_item = create_sample_work_item();

        let pr_with_work_items = PullRequestWithWorkItems {
            pr: pr.clone(),
            work_items: vec![work_item.clone()],
            selected: true,
        };

        assert_eq!(pr_with_work_items.pr.id, 123);
        assert_eq!(pr_with_work_items.work_items.len(), 1);
        assert!(pr_with_work_items.selected);
        assert_eq!(pr_with_work_items.work_items[0].id, 456);
    }

    /// # Cherry Pick Item Creation
    ///
    /// Tests creation of cherry pick item objects for migration tracking.
    ///
    /// ## Test Scenario
    /// - Creates CherryPickItem with PR and status information
    /// - Validates cherry pick tracking structure
    ///
    /// ## Expected Outcome
    /// - CherryPickItem is properly created with correct status
    /// - Cherry pick tracking data is correctly structured
    #[test]
    fn test_cherry_pick_item_creation() {
        let item = CherryPickItem {
            commit_id: "abc123".to_string(),
            pr_id: 123,
            pr_title: "Test PR".to_string(),
            status: CherryPickStatus::Success,
        };

        assert_eq!(item.commit_id, "abc123");
        assert_eq!(item.pr_id, 123);
        assert!(matches!(item.status, CherryPickStatus::Success));
    }

    /// # Manual Overrides Default
    ///
    /// Tests default creation of manual override objects.
    ///
    /// ## Test Scenario
    /// - Creates default ManualOverrides instance
    /// - Validates default state and empty collections
    ///
    /// ## Expected Outcome
    /// - ManualOverrides defaults to empty state
    /// - All override collections are properly initialized
    #[test]
    fn test_manual_overrides_default() {
        let overrides = ManualOverrides::default();

        assert!(overrides.marked_as_eligible.is_empty());
        assert!(overrides.marked_as_not_eligible.is_empty());
    }

    /// # Migration Analysis Creation
    ///
    /// Tests creation of migration analysis result objects.
    ///
    /// ## Test Scenario
    /// - Creates MigrationAnalysis with categorized PRs and details
    /// - Validates analysis structure and data organization
    ///
    /// ## Expected Outcome
    /// - MigrationAnalysis is properly created with all categories
    /// - Analysis results are correctly structured and accessible
    #[test]
    fn test_migration_analysis_creation() {
        let pr_with_work_items = PullRequestWithWorkItems {
            pr: create_sample_pull_request(),
            work_items: vec![create_sample_work_item()],
            selected: false,
        };

        let analysis_result = PRAnalysisResult {
            pr: pr_with_work_items.clone(),
            all_work_items_terminal: true,
            commit_in_target: false,
            commit_title_in_target: true,
            unsure_reason: Some("Mixed signals".to_string()),
            reason: Some("Work items terminal but commit not found".to_string()),
        };

        let analysis = MigrationAnalysis {
            eligible_prs: vec![pr_with_work_items.clone()],
            unsure_prs: vec![],
            not_merged_prs: vec![],
            terminal_states: vec!["Closed".to_string(), "Done".to_string()],
            unsure_details: vec![analysis_result.clone()],
            all_details: vec![analysis_result],
            manual_overrides: ManualOverrides::default(),
        };

        assert_eq!(analysis.eligible_prs.len(), 1);
        assert_eq!(analysis.terminal_states.len(), 2);
        assert_eq!(analysis.all_details.len(), 1);
    }

    // Negative test cases
    /// # Args Resolve Config (Missing Organization)
    ///
    /// Tests configuration resolution when organization parameter is missing.
    ///
    /// ## Test Scenario
    /// - Creates Args with missing organization field
    /// - Attempts to resolve configuration
    ///
    /// ## Expected Outcome
    /// - Configuration resolution fails with appropriate error
    /// - Error message indicates missing organization requirement
    #[test]
    fn test_args_resolve_config_missing_organization() {
        // Create isolated environment with empty config directory
        let temp_dir = TempDir::new().unwrap();

        // Clear all potential sources of configuration
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
            std::env::remove_var("MERGERS_ORGANIZATION");
            std::env::remove_var("MERGERS_PROJECT");
            std::env::remove_var("MERGERS_REPOSITORY");
            std::env::remove_var("MERGERS_PAT");
        }

        let mut args = create_sample_args();
        if let Some(Commands::Merge(ref mut merge_args)) = args.command {
            merge_args.shared.organization = None;
        }

        let result = args.resolve_config();

        // Clean up
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("organization is required")
        );
    }

    /// # Args Resolve Config (Missing Project)
    ///
    /// Tests configuration resolution when project parameter is missing.
    ///
    /// ## Test Scenario
    /// - Creates Args with missing project field
    /// - Attempts to resolve configuration
    ///
    /// ## Expected Outcome
    /// - Configuration resolution fails with appropriate error
    /// - Error message indicates missing project requirement
    #[test]
    fn test_args_resolve_config_missing_project() {
        // Clear environment variables that might interfere
        unsafe {
            std::env::remove_var("MERGERS_ORGANIZATION");
            std::env::remove_var("MERGERS_PROJECT");
            std::env::remove_var("MERGERS_REPOSITORY");
            std::env::remove_var("MERGERS_PAT");
        }

        let mut args = create_sample_args();
        if let Some(Commands::Merge(ref mut merge_args)) = args.command {
            merge_args.shared.project = None;
        }

        let result = args.resolve_config();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("project is required")
        );
    }

    /// # Args Resolve Config (Missing Repository)
    ///
    /// Tests configuration resolution when repository parameter is missing.
    ///
    /// ## Test Scenario
    /// - Creates Args with missing repository field
    /// - Attempts to resolve configuration
    ///
    /// ## Expected Outcome
    /// - Configuration resolution fails with appropriate error
    /// - Error message indicates missing repository requirement
    #[test]
    fn test_args_resolve_config_missing_repository() {
        // Clear environment variables that might interfere
        unsafe {
            std::env::remove_var("MERGERS_ORGANIZATION");
            std::env::remove_var("MERGERS_PROJECT");
            std::env::remove_var("MERGERS_REPOSITORY");
            std::env::remove_var("MERGERS_PAT");
        }

        let mut args = create_sample_args();
        if let Some(Commands::Merge(ref mut merge_args)) = args.command {
            merge_args.shared.repository = None;
        }

        let result = args.resolve_config();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("repository is required")
        );
    }

    /// # Args Resolve Config (Missing PAT)
    ///
    /// Tests configuration resolution when personal access token is missing.
    ///
    /// ## Test Scenario
    /// - Creates Args with missing PAT field
    /// - Attempts to resolve configuration
    ///
    /// ## Expected Outcome
    /// - Configuration resolution fails with appropriate error
    /// - Error message indicates missing PAT requirement
    #[test]
    fn test_args_resolve_config_missing_pat() {
        // Create isolated environment with empty config directory
        let temp_dir = TempDir::new().unwrap();

        // Clear all potential sources of configuration
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
            std::env::remove_var("MERGERS_ORGANIZATION");
            std::env::remove_var("MERGERS_PROJECT");
            std::env::remove_var("MERGERS_REPOSITORY");
            std::env::remove_var("MERGERS_PAT");
        }

        let mut args = create_sample_args();
        if let Some(Commands::Merge(ref mut merge_args)) = args.command {
            merge_args.shared.pat = None;
        }

        let result = args.resolve_config();

        // Clean up
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("pat is required"));
    }

    /// # Args Resolve Config with Defaults
    ///
    /// Tests configuration resolution using default values for optional fields.
    ///
    /// ## Test Scenario
    /// - Creates Args with required fields and no optional fields
    /// - Resolves configuration to apply defaults
    ///
    /// ## Expected Outcome
    /// - Configuration resolves successfully with default values
    /// - All optional fields receive appropriate default values
    #[test]
    fn test_args_resolve_config_with_defaults() {
        let mut args = create_sample_args();
        if let Some(Commands::Merge(ref mut merge_args)) = args.command {
            merge_args.shared.dev_branch = None;
            merge_args.shared.target_branch = None;
            merge_args.shared.parallel_limit = None;
            merge_args.shared.max_concurrent_network = None;
            merge_args.shared.max_concurrent_processing = None;
            merge_args.shared.tag_prefix = None;
        }

        let result = args.resolve_config();
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(
            config.shared().dev_branch,
            ParsedProperty::Default("dev".to_string())
        );
        assert_eq!(
            config.shared().target_branch,
            ParsedProperty::Default("next".to_string())
        );
        assert_eq!(config.shared().parallel_limit, ParsedProperty::Default(300));
        assert_eq!(
            config.shared().max_concurrent_network,
            ParsedProperty::Default(100)
        );
        assert_eq!(
            config.shared().max_concurrent_processing,
            ParsedProperty::Default(10)
        );
        assert_eq!(
            config.shared().tag_prefix,
            ParsedProperty::Default("merged-".to_string())
        );
    }

    /// # Args Resolve Config (Migration Mode)
    ///
    /// Tests configuration resolution in migration mode.
    ///
    /// ## Test Scenario
    /// - Creates Args with migrate flag set to true
    /// - Resolves configuration for migration mode
    ///
    /// ## Expected Outcome
    /// - Configuration resolves to migration mode variant
    /// - Migration-specific settings are properly configured
    #[test]
    fn test_args_resolve_config_migration_mode() {
        let args = create_sample_migrate_args();

        let result = args.resolve_config();
        assert!(result.is_ok());

        let config = result.unwrap();
        assert!(config.is_migration_mode());

        if let AppConfig::Migration { migration, .. } = config {
            assert_eq!(
                migration.terminal_states,
                ParsedProperty::Cli(
                    vec!["Closed".to_string(), "Done".to_string(),],
                    "Closed,Done".to_string()
                )
            );
        } else {
            panic!("Expected migration config");
        }
    }

    /// # Args Resolve Config (Default Mode)
    ///
    /// Tests configuration resolution in default mode.
    ///
    /// ## Test Scenario
    /// - Creates Args with migrate flag set to false
    /// - Resolves configuration for default mode
    ///
    /// ## Expected Outcome
    /// - Configuration resolves to default mode variant
    /// - Default mode settings are properly configured
    #[test]
    fn test_args_resolve_config_default_mode() {
        let args = create_sample_args(); // Already configured for merge mode

        let result = args.resolve_config();
        assert!(result.is_ok());

        let config = result.unwrap();
        assert!(!config.is_migration_mode());

        if let AppConfig::Default { default, .. } = config {
            assert_eq!(
                default.work_item_state,
                ParsedProperty::Cli("Done".to_string(), "Done".to_string())
            );
        } else {
            panic!("Expected default config");
        }
    }

    /// # Cherry Pick Status Variants
    ///
    /// Tests all possible cherry pick status enumeration values.
    ///
    /// ## Test Scenario
    /// - Creates instances of all CherryPickStatus variants
    /// - Validates enum variant creation and representation
    ///
    /// ## Expected Outcome
    /// - All status variants can be created successfully
    /// - Status enumeration covers all possible states
    #[test]
    fn test_cherry_pick_status_variants() {
        let statuses = [
            CherryPickStatus::Pending,
            CherryPickStatus::InProgress,
            CherryPickStatus::Success,
            CherryPickStatus::Conflict,
            CherryPickStatus::Failed("Test error".to_string()),
        ];

        assert!(matches!(statuses[0], CherryPickStatus::Pending));
        assert!(matches!(statuses[1], CherryPickStatus::InProgress));
        assert!(matches!(statuses[2], CherryPickStatus::Success));
        assert!(matches!(statuses[3], CherryPickStatus::Conflict));

        if let CherryPickStatus::Failed(error) = &statuses[4] {
            assert_eq!(error, "Test error");
        } else {
            panic!("Expected Failed status");
        }
    }

    /// # Work Item History Creation
    ///
    /// Tests creation of work item history objects for tracking state changes.
    ///
    /// ## Test Scenario
    /// - Creates WorkItemHistory with revision and state change data
    /// - Validates history tracking structure and fields
    ///
    /// ## Expected Outcome
    /// - WorkItemHistory is properly created with revision data
    /// - State change tracking information is correctly structured
    #[test]
    fn test_work_item_history_creation() {
        let history = WorkItemHistory {
            rev: 1,
            revised_date: "2024-01-15T10:30:00Z".to_string(),
            fields: Some(WorkItemHistoryFields {
                state: Some(WorkItemFieldChange {
                    new_value: Some("Done".to_string()),
                }),
                changed_date: Some(WorkItemFieldChange {
                    new_value: Some("2024-01-15T10:30:00Z".to_string()),
                }),
            }),
        };

        assert_eq!(history.rev, 1);
        assert!(history.fields.is_some());

        if let Some(fields) = history.fields {
            assert!(fields.state.is_some());
            if let Some(state_change) = fields.state {
                assert_eq!(state_change.new_value, Some("Done".to_string()));
            }
        }
    }

    /// # Path Precedence Over Local Repo
    ///
    /// Tests that path parameter takes precedence over local_repo parameter.
    ///
    /// ## Test Scenario
    /// - Creates Args with both path and local_repo fields set
    /// - Tests precedence rules in configuration resolution
    ///
    /// ## Expected Outcome
    /// - Path parameter takes precedence over local_repo
    /// - Configuration uses path when both are provided
    #[test]
    fn test_path_precedence_over_local_repo() {
        let mut args = create_sample_args();
        if let Some(Commands::Merge(ref mut merge_args)) = args.command {
            merge_args.shared.path = Some("/path/from/positional".to_string());
            merge_args.shared.local_repo = Some("/path/from/flag".to_string());
        }

        let result = args.resolve_config();
        assert!(result.is_ok());

        let config = result.unwrap();
        // Path (positional argument) should take precedence over local_repo flag
        assert_eq!(
            config.shared().local_repo,
            Some(ParsedProperty::Cli(
                "/path/from/positional".to_string(),
                "/path/from/positional".to_string()
            ))
        );
    }

    /// # Merge Command Alias
    ///
    /// Tests that the 'm' alias correctly parses as merge command.
    ///
    /// ## Test Scenario
    /// - Parses command line arguments using the 'm' alias
    /// - Verifies the command is correctly interpreted as Merge
    ///
    /// ## Expected Outcome
    /// - The alias 'm' is recognized as merge command
    /// - Arguments are correctly parsed
    #[test]
    fn test_merge_command_alias() {
        let args = Args::parse_from([
            "mergers",
            "m",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
        ]);

        assert!(matches!(args.command, Some(Commands::Merge(_))));
        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.organization, Some("test-org".to_string()));
            assert_eq!(merge_args.shared.project, Some("test-proj".to_string()));
            assert_eq!(merge_args.shared.repository, Some("test-repo".to_string()));
            assert_eq!(merge_args.shared.pat, Some("test-pat".to_string()));
        }
    }

    /// # Migrate Command Alias
    ///
    /// Tests that the 'mi' alias correctly parses as migrate command.
    ///
    /// ## Test Scenario
    /// - Parses command line arguments using the 'mi' alias
    /// - Verifies the command is correctly interpreted as Migrate
    ///
    /// ## Expected Outcome
    /// - The alias 'mi' is recognized as migrate command
    /// - Arguments are correctly parsed
    #[test]
    fn test_migrate_command_alias() {
        let args = Args::parse_from([
            "mergers",
            "mi",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
            "--terminal-states",
            "Closed,Done",
        ]);

        assert!(matches!(args.command, Some(Commands::Migrate(_))));
        if let Some(Commands::Migrate(migrate_args)) = args.command {
            assert_eq!(
                migrate_args.shared.organization,
                Some("test-org".to_string())
            );
            assert_eq!(migrate_args.shared.project, Some("test-proj".to_string()));
            assert_eq!(
                migrate_args.shared.repository,
                Some("test-repo".to_string())
            );
            assert_eq!(migrate_args.shared.pat, Some("test-pat".to_string()));
            assert_eq!(migrate_args.terminal_states, "Closed,Done");
        }
    }

    /// # Full Command Name Parsing
    ///
    /// Tests that full command names work alongside aliases.
    ///
    /// ## Test Scenario
    /// - Parses merge and migrate using full command names
    /// - Ensures backward compatibility with full names
    ///
    /// ## Expected Outcome
    /// - Full command names 'merge' and 'migrate' work correctly
    /// - Both full names and aliases produce the same result
    #[test]
    fn test_full_command_names() {
        // Test full merge command
        let merge_args = Args::parse_from([
            "mergers",
            "merge",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
        ]);

        assert!(matches!(merge_args.command, Some(Commands::Merge(_))));

        // Test full migrate command
        let migrate_args = Args::parse_from([
            "mergers",
            "migrate",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
        ]);

        assert!(matches!(migrate_args.command, Some(Commands::Migrate(_))));
    }

    /// # Command with Positional Path Argument
    ///
    /// Tests that subcommands correctly parse positional path argument.
    ///
    /// ## Test Scenario
    /// - Parses commands with positional path argument
    /// - Tests both merge and migrate commands
    ///
    /// ## Expected Outcome
    /// - Path argument is correctly captured
    /// - Works with both full command names and aliases
    #[test]
    fn test_command_with_path_argument() {
        // Test merge with path
        let merge_args = Args::parse_from([
            "mergers",
            "m",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
            "/path/to/repo",
        ]);

        if let Some(Commands::Merge(args)) = merge_args.command {
            assert_eq!(args.shared.path, Some("/path/to/repo".to_string()));
        } else {
            panic!("Expected merge command");
        }

        // Test migrate with path
        let migrate_args = Args::parse_from([
            "mergers",
            "mi",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
            "/another/path",
        ]);

        if let Some(Commands::Migrate(args)) = migrate_args.command {
            assert_eq!(args.shared.path, Some("/another/path".to_string()));
        } else {
            panic!("Expected migrate command");
        }
    }

    /// # Has Shared Args Trait on MergeArgs
    ///
    /// Tests that the HasSharedArgs trait works correctly on MergeArgs.
    ///
    /// ## Test Scenario
    /// - Creates MergeArgs with shared arguments
    /// - Uses the trait method to extract shared args
    ///
    /// ## Expected Outcome
    /// - Trait method returns correct shared arguments
    #[test]
    fn test_has_shared_args_trait_merge() {
        let merge_args = MergeArgs {
            shared: SharedArgs {
                organization: Some("test-org".to_string()),
                project: Some("test-proj".to_string()),
                repository: Some("test-repo".to_string()),
                pat: Some("test-pat".to_string()),
                ..Default::default()
            },
            ni: NonInteractiveArgs::default(),
            work_item_state: None,
            run_hooks: false,
            subcommand: None,
        };

        // Use the trait method
        let shared = merge_args.shared_args();
        assert_eq!(shared.organization, Some("test-org".to_string()));
        assert_eq!(shared.project, Some("test-proj".to_string()));
    }

    /// # Has Shared Args Trait on MigrateArgs
    ///
    /// Tests that the HasSharedArgs trait works correctly on MigrateArgs.
    ///
    /// ## Test Scenario
    /// - Creates MigrateArgs with shared arguments
    /// - Uses the trait method to extract shared args
    ///
    /// ## Expected Outcome
    /// - Trait method returns correct shared arguments
    #[test]
    fn test_has_shared_args_trait_migrate() {
        let migrate_args = MigrateArgs {
            shared: SharedArgs {
                organization: Some("test-org".to_string()),
                project: Some("test-proj".to_string()),
                repository: Some("test-repo".to_string()),
                pat: Some("test-pat".to_string()),
                ..Default::default()
            },
            terminal_states: "Closed,Done".to_string(),
        };

        // Use the trait method
        let shared = migrate_args.shared_args();
        assert_eq!(shared.organization, Some("test-org".to_string()));
        assert_eq!(shared.project, Some("test-proj".to_string()));
    }

    /// # Commands Shared Args Extraction
    ///
    /// Tests that Commands enum can extract shared args from any variant.
    ///
    /// ## Test Scenario
    /// - Creates both Merge and Migrate command variants
    /// - Uses Commands::shared_args() to extract shared args
    ///
    /// ## Expected Outcome
    /// - Shared args are correctly extracted from both command types
    #[test]
    fn test_commands_shared_args_extraction() {
        let merge_cmd = Commands::Merge(MergeArgs {
            shared: SharedArgs {
                organization: Some("merge-org".to_string()),
                project: Some("merge-proj".to_string()),
                ..Default::default()
            },
            ni: NonInteractiveArgs::default(),
            work_item_state: None,
            run_hooks: false,
            subcommand: None,
        });

        let migrate_cmd = Commands::Migrate(MigrateArgs {
            shared: SharedArgs {
                organization: Some("migrate-org".to_string()),
                project: Some("migrate-proj".to_string()),
                ..Default::default()
            },
            terminal_states: "Closed".to_string(),
        });

        // Extract shared args from both
        assert_eq!(
            merge_cmd.shared_args().organization,
            Some("merge-org".to_string())
        );
        assert_eq!(
            migrate_cmd.shared_args().organization,
            Some("migrate-org".to_string())
        );
    }

    /// # No Subcommand Defaults to Merge Mode
    ///
    /// Tests that when no subcommand is provided, arguments are parsed as MergeArgs.
    ///
    /// ## Test Scenario
    /// - Parses arguments without any subcommand using MergeArgsParser
    /// - Verifies arguments are correctly captured as merge command
    ///
    /// ## Expected Outcome
    /// - Arguments are successfully parsed as MergeArgs
    /// - Configuration defaults to merge mode
    #[test]
    fn test_no_subcommand_defaults_to_merge() {
        // Simulate command line args without subcommand
        let args = Args::parse_from(["mergers"]);

        // With parse_with_default_mode, if we have merge-compatible args, it should parse them
        // For now just verify the structure works
        assert!(args.command.is_none());

        // Test with actual merge args using MergeArgsParser
        let merge_result = MergeArgsParser::try_parse_from([
            "mergers",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
        ]);

        assert!(merge_result.is_ok());
        let merge_args = merge_result.unwrap().merge_args;
        assert_eq!(merge_args.shared.organization, Some("test-org".to_string()));
        assert_eq!(merge_args.shared.project, Some("test-proj".to_string()));
        assert_eq!(merge_args.shared.repository, Some("test-repo".to_string()));
        assert_eq!(merge_args.shared.pat, Some("test-pat".to_string()));
    }

    /// # No Subcommand with Path Argument
    ///
    /// Tests that positional path argument works when parsed as MergeArgs.
    ///
    /// ## Test Scenario
    /// - Parses arguments with positional path as MergeArgs
    /// - Verifies both path and other arguments are captured
    ///
    /// ## Expected Outcome
    /// - Path argument is correctly captured in MergeArgs
    /// - Other arguments are also parsed correctly
    #[test]
    fn test_no_subcommand_with_path() {
        let merge_result = MergeArgsParser::try_parse_from([
            "mergers",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
            "/path/to/repo",
        ]);

        assert!(merge_result.is_ok());
        let merge_args = merge_result.unwrap().merge_args;
        assert_eq!(merge_args.shared.path, Some("/path/to/repo".to_string()));
        assert_eq!(merge_args.shared.organization, Some("test-org".to_string()));
    }

    /// # No Subcommand with Work Item State
    ///
    /// Tests that work_item_state can be specified when parsed as MergeArgs.
    ///
    /// ## Test Scenario
    /// - Parses arguments with work_item_state as MergeArgs
    /// - Verifies the state is correctly captured
    ///
    /// ## Expected Outcome
    /// - work_item_state is parsed in MergeArgs and used in merge mode config
    #[test]
    fn test_no_subcommand_with_work_item_state() {
        let merge_result = MergeArgsParser::try_parse_from([
            "mergers",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
            "--work-item-state",
            "Custom State",
        ]);

        assert!(merge_result.is_ok());
        let merge_args = merge_result.unwrap().merge_args;
        assert_eq!(merge_args.work_item_state, Some("Custom State".to_string()));

        // Create full Args and verify it resolves correctly
        let args = Args {
            command: Some(Commands::Merge(merge_args)),
            create_config: false,
        };

        let result = args.resolve_config();
        assert!(result.is_ok());
        let config = result.unwrap();

        if let AppConfig::Default { default, .. } = config {
            assert_eq!(
                default.work_item_state,
                ParsedProperty::Cli("Custom State".to_string(), "Custom State".to_string())
            );
        } else {
            panic!("Expected default config");
        }
    }

    /// # Merge with Non-Interactive Flag and Positional Path
    ///
    /// Tests `mergers merge -n --version v1.0 /path/to/repo` where the
    /// positional path appears after the non-interactive flags.
    ///
    /// ## Test Scenario
    /// - Path is placed after flags on the `merge` command
    /// - Non-interactive flags are directly on MergeArgs (flattened)
    ///
    /// ## Expected Outcome
    /// - Path is captured in MergeArgs.shared.path
    /// - Non-interactive flag is in MergeArgs.ni.non_interactive
    #[test]
    fn test_merge_non_interactive_with_positional_path() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "-n",
            "--version",
            "v1.0",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
            "/path/to/repo",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(
                merge_args.shared.path,
                Some("/path/to/repo".to_string()),
                "MergeArgs.shared.path should capture the positional path"
            );
            assert!(
                merge_args.ni.non_interactive,
                "Non-interactive flag should be set"
            );
            assert_eq!(
                merge_args.ni.version,
                Some("v1.0".to_string()),
                "Version should be captured"
            );
            assert!(
                merge_args.subcommand.is_none(),
                "No subcommand when using non-interactive mode directly"
            );
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Merge with Path Before Non-Interactive Flags
    ///
    /// Tests `mergers merge /path/to/repo -n --version v1.0` where the
    /// positional path appears before the flags.
    ///
    /// ## Test Scenario
    /// - Path is placed before non-interactive flags
    /// - All flags should still be captured
    ///
    /// ## Expected Outcome
    /// - Path is captured in MergeArgs.shared.path
    /// - Non-interactive flags are correctly parsed
    #[test]
    fn test_merge_path_before_flags() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "/path/to/repo",
            "-n",
            "--version",
            "v1.0",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(
                merge_args.shared.path,
                Some("/path/to/repo".to_string()),
                "MergeArgs.shared.path should capture the path"
            );
            assert!(
                merge_args.ni.non_interactive,
                "Non-interactive flag should be set"
            );
            assert_eq!(
                merge_args.ni.version,
                Some("v1.0".to_string()),
                "Version should be captured"
            );
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Merge with --local-repo Flag
    ///
    /// Tests `mergers merge -n --version v1.0 --local-repo /path/to/repo`
    /// using the explicit flag instead of positional argument.
    ///
    /// ## Test Scenario
    /// - Uses --local-repo flag instead of positional path
    /// - Flag should be captured in MergeArgs.shared.local_repo
    ///
    /// ## Expected Outcome
    /// - local_repo is set, path is None
    /// - Non-interactive mode should use local_repo
    #[test]
    fn test_merge_with_local_repo_flag() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "-n",
            "--version",
            "v1.0",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
            "--local-repo",
            "/path/to/repo",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(
                merge_args.shared.path, None,
                "Positional path should be None when using --local-repo flag"
            );
            assert_eq!(
                merge_args.shared.local_repo,
                Some("/path/to/repo".to_string()),
                "--local-repo flag should be captured"
            );
            assert!(merge_args.ni.non_interactive);
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Merge with --since Parameter
    ///
    /// Tests that --since is correctly parsed in `merge -n` non-interactive mode.
    ///
    /// ## Test Scenario
    /// - Parses `merge -n --version v1.0 --since 6mo`
    /// - Verifies --since is captured in MergeArgs.shared.since
    ///
    /// ## Expected Outcome
    /// - since parameter is correctly parsed and available
    #[test]
    fn test_merge_with_since_parameter() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "-n",
            "--version",
            "v1.0",
            "--since",
            "6mo",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(
                merge_args.shared.since,
                Some("6mo".to_string()),
                "--since should be captured in MergeArgs"
            );
            assert!(merge_args.ni.non_interactive);
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Merge with All Non-Interactive Parameters
    ///
    /// Tests that all parameters specific to non-interactive mode are parsed.
    ///
    /// ## Test Scenario
    /// - Parses merge with every possible parameter set
    /// - Verifies each parameter is correctly captured
    ///
    /// ## Expected Outcome
    /// - All parameters are correctly parsed and available
    #[test]
    fn test_merge_all_parameters() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "-n",
            "--version",
            "v2.0.0",
            "--organization",
            "my-org",
            "--project",
            "my-project",
            "--repository",
            "my-repo",
            "--pat",
            "secret-token",
            "--dev-branch",
            "develop",
            "--target-branch",
            "release",
            "--tag-prefix",
            "released-",
            "--work-item-state",
            "Done",
            "--select-by-state",
            "Ready for Next",
            "--since",
            "2w",
            "--max-concurrent-network",
            "50",
            "--max-concurrent-processing",
            "5",
            "--run-hooks",
            "--output",
            "json",
            "--quiet",
            "/path/to/repo",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            // Shared args
            assert_eq!(merge_args.shared.path, Some("/path/to/repo".to_string()));
            assert_eq!(merge_args.shared.organization, Some("my-org".to_string()));
            assert_eq!(merge_args.shared.project, Some("my-project".to_string()));
            assert_eq!(merge_args.shared.repository, Some("my-repo".to_string()));
            assert_eq!(merge_args.shared.pat, Some("secret-token".to_string()));
            assert_eq!(merge_args.shared.dev_branch, Some("develop".to_string()));
            assert_eq!(merge_args.shared.target_branch, Some("release".to_string()));
            assert_eq!(merge_args.shared.tag_prefix, Some("released-".to_string()));
            assert_eq!(merge_args.shared.since, Some("2w".to_string()));
            assert_eq!(merge_args.shared.max_concurrent_network, Some(50));
            assert_eq!(merge_args.shared.max_concurrent_processing, Some(5));

            // Merge-specific args
            assert_eq!(merge_args.work_item_state, Some("Done".to_string()));
            assert!(merge_args.run_hooks);

            // Non-interactive args (ni)
            assert_eq!(
                merge_args.ni.select_by_state,
                Some("Ready for Next".to_string())
            );
            assert_eq!(merge_args.ni.version, Some("v2.0.0".to_string()));
            assert!(merge_args.ni.non_interactive);
            assert!(merge_args.ni.quiet);
            assert_eq!(merge_args.ni.output, OutputFormat::Json);
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Shorthand Non-Interactive Flag via MergeArgsParser
    ///
    /// Tests that `mergers /path -n --since 6mo` now works because `-n` is
    /// flattened into MergeArgs via NonInteractiveArgs.
    ///
    /// ## Test Scenario
    /// - Parses args with -n using MergeArgsParser fallback
    /// - This validates the fix for the original bug
    ///
    /// ## Expected Outcome
    /// - Parsing succeeds
    /// - Path, -n, --since, and version are all captured correctly
    #[test]
    fn test_shorthand_non_interactive_flag_via_parser() {
        // MergeArgsParser wraps MergeArgs which now HAS -n flag (flattened)
        let result = MergeArgsParser::try_parse_from([
            "mergers",
            "/path/to/repo",
            "-n",
            "--version",
            "v1.0",
            "--since",
            "6mo",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
        ]);

        assert!(result.is_ok(), "Should succeed: -n flag is now available");

        let parser = result.unwrap();
        assert_eq!(
            parser.merge_args.shared.path,
            Some("/path/to/repo".to_string())
        );
        assert!(parser.merge_args.ni.non_interactive);
        assert_eq!(parser.merge_args.ni.version, Some("v1.0".to_string()));
        assert_eq!(parser.merge_args.shared.since, Some("6mo".to_string()));
    }

    /// # Merge Effective Path Precedence
    ///
    /// Tests path resolution from either positional arg or --local-repo flag.
    ///
    /// ## Test Scenario
    /// - Tests positional path takes precedence over --local-repo
    /// - Tests --local-repo works when positional is absent
    /// - Tests None when neither is provided
    ///
    /// ## Expected Outcome
    /// - Positional path takes precedence
    /// - Falls back to --local-repo
    /// - Returns None when neither is set
    #[test]
    fn test_merge_effective_path_precedence() {
        // Case 1: Both positional and --local-repo set → positional wins
        let args_both = Args::parse_from([
            "mergers",
            "merge",
            "-n",
            "--version",
            "v1.0",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
            "--local-repo",
            "/from/flag",
            "/from/positional",
        ]);

        if let Some(Commands::Merge(merge_args)) = args_both.command {
            let effective = merge_args
                .shared
                .path
                .as_ref()
                .or(merge_args.shared.local_repo.as_ref());
            assert_eq!(
                effective,
                Some(&"/from/positional".to_string()),
                "Positional path should take precedence over --local-repo"
            );
        } else {
            panic!("Expected Merge command");
        }

        // Case 2: Only --local-repo → flag is used
        let args_flag_only = Args::parse_from([
            "mergers",
            "merge",
            "-n",
            "--version",
            "v1.0",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
            "--local-repo",
            "/from/flag",
        ]);

        if let Some(Commands::Merge(merge_args)) = args_flag_only.command {
            let effective = merge_args
                .shared
                .path
                .as_ref()
                .or(merge_args.shared.local_repo.as_ref());
            assert_eq!(
                effective,
                Some(&"/from/flag".to_string()),
                "--local-repo should be used when positional is absent"
            );
        } else {
            panic!("Expected Merge command");
        }

        // Case 3: Neither → None
        let args_none = Args::parse_from([
            "mergers",
            "merge",
            "-n",
            "--version",
            "v1.0",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
        ]);

        if let Some(Commands::Merge(merge_args)) = args_none.command {
            let effective = merge_args
                .shared
                .path
                .as_ref()
                .or(merge_args.shared.local_repo.as_ref());
            assert_eq!(
                effective, None,
                "Should be None when neither positional nor --local-repo is set"
            );
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Fallback Parse with Default Mode
    ///
    /// Tests that `mergers /path -n --version v1.0` works via parse_with_default_mode
    /// which tries normal parse first, then falls back to MergeArgsParser.
    ///
    /// ## Test Scenario
    /// - Simulates the actual CLI invocation pattern `mergers /path -n --version v1.0`
    /// - Uses Args::parse_with_default_mode() to test the fallback mechanism
    ///
    /// ## Expected Outcome
    /// - Path is captured in MergeArgs.shared.path
    /// - Non-interactive flag and version are captured in MergeArgs.ni
    #[test]
    fn test_fallback_parse_shorthand_invocation() {
        // This simulates: mergers /path/to/repo -n --version v1.0 ...
        // which should work via the MergeArgsParser fallback
        let parser = MergeArgsParser::try_parse_from([
            "mergers",
            "/path/to/repo",
            "-n",
            "--version",
            "v1.0",
            "--organization",
            "test-org",
            "--project",
            "test-proj",
            "--repository",
            "test-repo",
            "--pat",
            "test-pat",
            "--since",
            "6mo",
        ]);

        assert!(parser.is_ok(), "Fallback parser should succeed");
        let parser = parser.unwrap();

        // Verify path is captured
        assert_eq!(
            parser.merge_args.shared.path,
            Some("/path/to/repo".to_string()),
            "Path should be captured as positional argument"
        );

        // Verify non-interactive args
        assert!(
            parser.merge_args.ni.non_interactive,
            "-n flag should set non_interactive"
        );
        assert_eq!(
            parser.merge_args.ni.version,
            Some("v1.0".to_string()),
            "--version should be captured"
        );

        // Verify shared args
        assert_eq!(
            parser.merge_args.shared.since,
            Some("6mo".to_string()),
            "--since should be captured"
        );
        assert_eq!(
            parser.merge_args.shared.organization,
            Some("test-org".to_string())
        );
    }

    /// # All Invocation Patterns Work Consistently
    ///
    /// Tests multiple equivalent invocation patterns to ensure they all
    /// parse correctly with the new flattened structure.
    ///
    /// ## Test Scenario
    /// - Pattern 1: `mergers merge -n --version v1.0 /path`
    /// - Pattern 2: `mergers merge /path -n --version v1.0`
    /// - Pattern 3: `mergers /path -n --version v1.0` (via fallback)
    ///
    /// ## Expected Outcome
    /// - All patterns capture the same path and flags
    #[test]
    fn test_all_invocation_patterns_consistent() {
        let required_args = [
            "--organization",
            "org",
            "--project",
            "proj",
            "--repository",
            "repo",
            "--pat",
            "pat",
        ];

        // Pattern 1: mergers merge -n --version v1.0 /path
        let args1 = Args::parse_from(
            ["mergers", "merge", "-n", "--version", "v1.0"]
                .iter()
                .chain(required_args.iter())
                .chain(["/path/to/repo"].iter()),
        );

        // Pattern 2: mergers merge /path -n --version v1.0
        let args2 = Args::parse_from(
            [
                "mergers",
                "merge",
                "/path/to/repo",
                "-n",
                "--version",
                "v1.0",
            ]
            .iter()
            .chain(required_args.iter()),
        );

        // Extract MergeArgs from both patterns
        let merge_args1 = match args1.command {
            Some(Commands::Merge(m)) => m,
            _ => panic!("Expected Merge command"),
        };
        let merge_args2 = match args2.command {
            Some(Commands::Merge(m)) => m,
            _ => panic!("Expected Merge command"),
        };

        // Pattern 3: mergers /path -n --version v1.0 (via MergeArgsParser)
        let parser3 = MergeArgsParser::try_parse_from(
            ["mergers", "/path/to/repo", "-n", "--version", "v1.0"]
                .iter()
                .chain(required_args.iter()),
        )
        .expect("Fallback parser should work");
        let merge_args3 = &parser3.merge_args;

        // Verify all patterns capture the same values
        for (name, merge_args) in [
            ("Pattern 1", &merge_args1),
            ("Pattern 2", &merge_args2),
            ("Pattern 3", merge_args3),
        ] {
            assert_eq!(
                merge_args.shared.path,
                Some("/path/to/repo".to_string()),
                "{}: path mismatch",
                name
            );
            assert!(
                merge_args.ni.non_interactive,
                "{}: non_interactive should be true",
                name
            );
            assert_eq!(
                merge_args.ni.version,
                Some("v1.0".to_string()),
                "{}: version mismatch",
                name
            );
        }
    }

    // ========================================================================
    // Short flag parsing tests
    // ========================================================================

    /// # Short Flags for Azure DevOps Connection
    ///
    /// Tests that short flags -o, -p, -r, -t correctly map to their long
    /// counterparts for Azure DevOps connection arguments.
    ///
    /// ## Test Scenario
    /// - Parses merge command using only short flags
    /// - Verifies each short flag maps to the correct field
    ///
    /// ## Expected Outcome
    /// - -o maps to organization
    /// - -p maps to project
    /// - -r maps to repository
    /// - -t maps to pat
    #[test]
    fn test_short_flags_azure_devops_connection() {
        let args = Args::parse_from([
            "mergers", "merge", "-o", "my-org", "-p", "my-proj", "-r", "my-repo", "-t", "my-token",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.organization, Some("my-org".to_string()));
            assert_eq!(merge_args.shared.project, Some("my-proj".to_string()));
            assert_eq!(merge_args.shared.repository, Some("my-repo".to_string()));
            assert_eq!(merge_args.shared.pat, Some("my-token".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Short Flags Mixed with Long Flags
    ///
    /// Tests that short and long flags can be freely mixed in the same command.
    ///
    /// ## Test Scenario
    /// - Uses -o (short) and --project (long) in the same invocation
    /// - Mixes short and long throughout the argument list
    ///
    /// ## Expected Outcome
    /// - All flags are correctly parsed regardless of short/long form
    #[test]
    fn test_short_and_long_flags_mixed() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "-o",
            "my-org",
            "--project",
            "my-proj",
            "-r",
            "my-repo",
            "--pat",
            "my-token",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.organization, Some("my-org".to_string()));
            assert_eq!(merge_args.shared.project, Some("my-proj".to_string()));
            assert_eq!(merge_args.shared.repository, Some("my-repo".to_string()));
            assert_eq!(merge_args.shared.pat, Some("my-token".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Short Flags -n and -q on Merge
    ///
    /// Tests that -n (non-interactive) and -q (quiet) short flags work on merge.
    ///
    /// ## Test Scenario
    /// - Parses `merge -n -q` with required args
    /// - Both boolean short flags should activate their respective modes
    ///
    /// ## Expected Outcome
    /// - non_interactive is true
    /// - quiet is true
    #[test]
    fn test_short_flags_non_interactive_and_quiet() {
        let args = Args::parse_from([
            "mergers", "merge", "-n", "-q", "-o", "org", "-p", "proj", "-r", "repo", "-t", "pat",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert!(merge_args.ni.non_interactive);
            assert!(merge_args.ni.quiet);
        } else {
            panic!("Expected Merge command");
        }
    }

    // ========================================================================
    // Default value tests
    // ========================================================================

    /// # SharedArgs Default Trait Values
    ///
    /// Tests that SharedArgs::default() produces the expected default values
    /// for all fields.
    ///
    /// ## Test Scenario
    /// - Creates SharedArgs using Default trait
    /// - Checks every field for its expected default
    ///
    /// ## Expected Outcome
    /// - All Option fields are None
    /// - Boolean fields are false
    /// - tag_prefix is None (default_value is applied by clap at parse time)
    #[test]
    fn test_shared_args_default_values() {
        let shared = SharedArgs::default();

        assert_eq!(shared.path, None);
        assert_eq!(shared.organization, None);
        assert_eq!(shared.project, None);
        assert_eq!(shared.repository, None);
        assert_eq!(shared.pat, None);
        assert_eq!(shared.dev_branch, None);
        assert_eq!(shared.target_branch, None);
        assert_eq!(shared.local_repo, None);
        // tag_prefix: default_value on clap attribute means clap sets it at parse time;
        // Default trait gives None
        assert_eq!(shared.tag_prefix, None);
        assert_eq!(shared.parallel_limit, None);
        assert_eq!(shared.max_concurrent_network, None);
        assert_eq!(shared.max_concurrent_processing, None);
        assert_eq!(shared.since, None);
        assert!(!shared.skip_confirmation);
        assert_eq!(shared.log_level, None);
        assert_eq!(shared.log_file, None);
        assert_eq!(shared.log_format, None);
    }

    /// # NonInteractiveArgs Default Trait Values
    ///
    /// Tests that NonInteractiveArgs::default() produces the expected defaults.
    ///
    /// ## Test Scenario
    /// - Creates NonInteractiveArgs using Default trait
    /// - Checks every field for its expected default
    ///
    /// ## Expected Outcome
    /// - Boolean fields are false
    /// - Option fields are None
    /// - Output format defaults to Text
    #[test]
    fn test_non_interactive_args_default_values() {
        let ni = NonInteractiveArgs::default();

        assert!(!ni.non_interactive);
        assert_eq!(ni.version, None);
        assert_eq!(ni.select_by_state, None);
        assert_eq!(ni.output, OutputFormat::Text);
        assert!(!ni.quiet);
    }

    /// # Clap Default Values Applied at Parse Time
    ///
    /// Tests that clap's default_value attributes are applied when args are
    /// parsed without explicit values for those fields.
    ///
    /// ## Test Scenario
    /// - Parses a minimal merge command (no optional flags)
    /// - Checks that clap-level defaults are applied
    ///
    /// ## Expected Outcome
    /// - tag_prefix gets "merged-" from clap default_value
    /// - output gets Text from default_value_t
    /// - Boolean flags default to false
    #[test]
    fn test_clap_default_values_at_parse_time() {
        let args = Args::parse_from(["mergers", "merge"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.tag_prefix, Some("merged-".to_string()));
            assert_eq!(merge_args.ni.output, OutputFormat::Text);
            assert!(!merge_args.ni.non_interactive);
            assert!(!merge_args.ni.quiet);
            assert!(!merge_args.run_hooks);
            assert!(!merge_args.shared.skip_confirmation);
            assert_eq!(merge_args.shared.dev_branch, None);
            assert_eq!(merge_args.shared.target_branch, None);
            assert_eq!(merge_args.work_item_state, None);
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Migrate Default Terminal States
    ///
    /// Tests that --terminal-states defaults to "Closed,Next Closed,Next Merged"
    /// when not explicitly provided.
    ///
    /// ## Test Scenario
    /// - Parses migrate command without --terminal-states
    /// - Checks the default value is applied
    ///
    /// ## Expected Outcome
    /// - terminal_states is "Closed,Next Closed,Next Merged"
    #[test]
    fn test_migrate_default_terminal_states() {
        let args = Args::parse_from(["mergers", "migrate"]);

        if let Some(Commands::Migrate(migrate_args)) = args.command {
            assert_eq!(
                migrate_args.terminal_states,
                "Closed,Next Closed,Next Merged"
            );
        } else {
            panic!("Expected Migrate command");
        }
    }

    // ========================================================================
    // Cleanup command tests
    // ========================================================================

    /// # Cleanup Command Parsing
    ///
    /// Tests that the cleanup command parses correctly with shared args.
    ///
    /// ## Test Scenario
    /// - Parses `mergers cleanup` with standard shared arguments
    /// - Verifies command is recognized as Cleanup variant
    ///
    /// ## Expected Outcome
    /// - Command is Cleanup variant
    /// - Shared args are correctly populated
    #[test]
    fn test_cleanup_command_parsing() {
        let args = Args::parse_from([
            "mergers", "cleanup", "-o", "my-org", "-p", "my-proj", "-r", "my-repo", "-t", "my-pat",
        ]);

        assert!(matches!(args.command, Some(Commands::Cleanup(_))));
        if let Some(Commands::Cleanup(cleanup_args)) = args.command {
            assert_eq!(cleanup_args.shared.organization, Some("my-org".to_string()));
            assert_eq!(cleanup_args.shared.project, Some("my-proj".to_string()));
            assert_eq!(cleanup_args.shared.repository, Some("my-repo".to_string()));
            assert_eq!(cleanup_args.shared.pat, Some("my-pat".to_string()));
        }
    }

    /// # Cleanup Command Alias
    ///
    /// Tests that the 'c' alias correctly parses as cleanup command.
    ///
    /// ## Test Scenario
    /// - Parses `mergers c` with standard arguments
    /// - Verifies alias maps to Cleanup
    ///
    /// ## Expected Outcome
    /// - The alias 'c' is recognized as cleanup command
    #[test]
    fn test_cleanup_command_alias() {
        let args = Args::parse_from(["mergers", "c", "-o", "org", "-p", "proj"]);

        assert!(matches!(args.command, Some(Commands::Cleanup(_))));
    }

    /// # Cleanup --target Flag
    ///
    /// Tests that the --target flag on cleanup is correctly parsed.
    ///
    /// ## Test Scenario
    /// - Parses cleanup with --target flag
    /// - Verifies the target field is populated
    ///
    /// ## Expected Outcome
    /// - target field contains the specified branch name
    #[test]
    fn test_cleanup_target_flag() {
        let args = Args::parse_from(["mergers", "cleanup", "--target", "release/v2"]);

        if let Some(Commands::Cleanup(cleanup_args)) = args.command {
            assert_eq!(cleanup_args.target, Some("release/v2".to_string()));
        } else {
            panic!("Expected Cleanup command");
        }
    }

    /// # Cleanup with Positional Path
    ///
    /// Tests that cleanup command accepts a positional path argument.
    ///
    /// ## Test Scenario
    /// - Parses cleanup with a positional path argument
    /// - Verifies path is captured in shared.path
    ///
    /// ## Expected Outcome
    /// - Path argument is correctly captured
    #[test]
    fn test_cleanup_with_positional_path() {
        let args = Args::parse_from(["mergers", "cleanup", "/path/to/repo"]);

        if let Some(Commands::Cleanup(cleanup_args)) = args.command {
            assert_eq!(cleanup_args.shared.path, Some("/path/to/repo".to_string()));
        } else {
            panic!("Expected Cleanup command");
        }
    }

    /// # HasSharedArgs Trait on CleanupArgs
    ///
    /// Tests that the HasSharedArgs trait works correctly on CleanupArgs.
    ///
    /// ## Test Scenario
    /// - Creates CleanupArgs with shared arguments
    /// - Uses the trait method to extract and mutate shared args
    ///
    /// ## Expected Outcome
    /// - Trait methods return correct shared arguments
    /// - Mutable access works correctly
    #[test]
    fn test_has_shared_args_trait_cleanup() {
        let mut cleanup_args = CleanupArgs {
            shared: SharedArgs {
                organization: Some("cleanup-org".to_string()),
                project: Some("cleanup-proj".to_string()),
                ..Default::default()
            },
            target: Some("main".to_string()),
        };

        assert_eq!(
            cleanup_args.shared_args().organization,
            Some("cleanup-org".to_string())
        );

        // Test mutable access
        cleanup_args.shared_args_mut().organization = Some("modified-org".to_string());
        assert_eq!(
            cleanup_args.shared_args().organization,
            Some("modified-org".to_string())
        );
    }

    /// # Commands Shared Args Extraction Includes Cleanup
    ///
    /// Tests that Commands::shared_args() works for the Cleanup variant.
    ///
    /// ## Test Scenario
    /// - Creates a Cleanup command variant
    /// - Uses Commands::shared_args() to extract shared args
    ///
    /// ## Expected Outcome
    /// - Shared args are correctly extracted from Cleanup
    #[test]
    fn test_commands_shared_args_extraction_cleanup() {
        let cleanup_cmd = Commands::Cleanup(CleanupArgs {
            shared: SharedArgs {
                organization: Some("cleanup-org".to_string()),
                ..Default::default()
            },
            target: None,
        });

        assert_eq!(
            cleanup_cmd.shared_args().organization,
            Some("cleanup-org".to_string())
        );
    }

    // ========================================================================
    // Merge subcommand parsing tests
    // ========================================================================

    /// # Merge Continue Subcommand Parsing
    ///
    /// Tests that `merge continue` subcommand parses correctly with all its flags.
    ///
    /// ## Test Scenario
    /// - Parses `mergers merge continue --repo /path --output json --quiet`
    /// - Verifies all fields are captured
    ///
    /// ## Expected Outcome
    /// - Subcommand is Continue variant
    /// - repo, output, and quiet are correctly parsed
    #[test]
    fn test_merge_continue_subcommand_parsing() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "continue",
            "--repo",
            "/path/to/repo",
            "--output",
            "json",
            "-q",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            if let Some(MergeSubcommand::Continue(continue_args)) = merge_args.subcommand {
                assert_eq!(continue_args.repo, Some("/path/to/repo".to_string()));
                assert_eq!(continue_args.output, OutputFormat::Json);
                assert!(continue_args.quiet);
            } else {
                panic!("Expected Continue subcommand");
            }
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Merge Continue Subcommand Defaults
    ///
    /// Tests that `merge continue` without flags uses defaults.
    ///
    /// ## Test Scenario
    /// - Parses `mergers merge continue` with no extra flags
    ///
    /// ## Expected Outcome
    /// - repo is None
    /// - output defaults to Text
    /// - quiet defaults to false
    #[test]
    fn test_merge_continue_subcommand_defaults() {
        let args = Args::parse_from(["mergers", "merge", "continue"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            if let Some(MergeSubcommand::Continue(continue_args)) = merge_args.subcommand {
                assert_eq!(continue_args.repo, None);
                assert_eq!(continue_args.output, OutputFormat::Text);
                assert!(!continue_args.quiet);
            } else {
                panic!("Expected Continue subcommand");
            }
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Merge Abort Subcommand Parsing
    ///
    /// Tests that `merge abort` subcommand parses correctly with all its flags.
    ///
    /// ## Test Scenario
    /// - Parses `mergers merge abort --repo /path --output ndjson`
    /// - Verifies all fields are captured
    ///
    /// ## Expected Outcome
    /// - Subcommand is Abort variant
    /// - repo and output are correctly parsed
    #[test]
    fn test_merge_abort_subcommand_parsing() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "abort",
            "--repo",
            "/path/to/repo",
            "--output",
            "ndjson",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            if let Some(MergeSubcommand::Abort(abort_args)) = merge_args.subcommand {
                assert_eq!(abort_args.repo, Some("/path/to/repo".to_string()));
                assert_eq!(abort_args.output, OutputFormat::Ndjson);
            } else {
                panic!("Expected Abort subcommand");
            }
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Merge Abort Subcommand Defaults
    ///
    /// Tests that `merge abort` without flags uses defaults.
    ///
    /// ## Test Scenario
    /// - Parses `mergers merge abort` with no extra flags
    ///
    /// ## Expected Outcome
    /// - repo is None
    /// - output defaults to Text
    #[test]
    fn test_merge_abort_subcommand_defaults() {
        let args = Args::parse_from(["mergers", "merge", "abort"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            if let Some(MergeSubcommand::Abort(abort_args)) = merge_args.subcommand {
                assert_eq!(abort_args.repo, None);
                assert_eq!(abort_args.output, OutputFormat::Text);
            } else {
                panic!("Expected Abort subcommand");
            }
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Merge Status Subcommand Parsing
    ///
    /// Tests that `merge status` subcommand parses correctly with all its flags.
    ///
    /// ## Test Scenario
    /// - Parses `mergers merge status --repo /path --output json`
    /// - Verifies all fields are captured
    ///
    /// ## Expected Outcome
    /// - Subcommand is Status variant
    /// - repo and output are correctly parsed
    #[test]
    fn test_merge_status_subcommand_parsing() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "status",
            "--repo",
            "/path/to/repo",
            "--output",
            "json",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            if let Some(MergeSubcommand::Status(status_args)) = merge_args.subcommand {
                assert_eq!(status_args.repo, Some("/path/to/repo".to_string()));
                assert_eq!(status_args.output, OutputFormat::Json);
            } else {
                panic!("Expected Status subcommand");
            }
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Merge Status Subcommand Defaults
    ///
    /// Tests that `merge status` without flags uses defaults.
    ///
    /// ## Test Scenario
    /// - Parses `mergers merge status` with no extra flags
    ///
    /// ## Expected Outcome
    /// - repo is None
    /// - output defaults to Text
    #[test]
    fn test_merge_status_subcommand_defaults() {
        let args = Args::parse_from(["mergers", "merge", "status"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            if let Some(MergeSubcommand::Status(status_args)) = merge_args.subcommand {
                assert_eq!(status_args.repo, None);
                assert_eq!(status_args.output, OutputFormat::Text);
            } else {
                panic!("Expected Status subcommand");
            }
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Merge Complete Subcommand Parsing
    ///
    /// Tests that `merge complete` subcommand parses correctly with all its flags.
    ///
    /// ## Test Scenario
    /// - Parses `mergers merge complete --next-state Done --repo /path --output ndjson -q`
    /// - Verifies all fields including required --next-state
    ///
    /// ## Expected Outcome
    /// - Subcommand is Complete variant
    /// - next_state, repo, output, and quiet are correctly parsed
    #[test]
    fn test_merge_complete_subcommand_parsing() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "complete",
            "--next-state",
            "Done",
            "--repo",
            "/path/to/repo",
            "--output",
            "ndjson",
            "-q",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            if let Some(MergeSubcommand::Complete(complete_args)) = merge_args.subcommand {
                assert_eq!(complete_args.next_state, "Done");
                assert_eq!(complete_args.repo, Some("/path/to/repo".to_string()));
                assert_eq!(complete_args.output, OutputFormat::Ndjson);
                assert!(complete_args.quiet);
            } else {
                panic!("Expected Complete subcommand");
            }
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Merge Complete Requires --next-state
    ///
    /// Tests that `merge complete` fails without --next-state (required argument).
    ///
    /// ## Test Scenario
    /// - Attempts to parse `mergers merge complete` without --next-state
    /// - Verifies parsing fails
    ///
    /// ## Expected Outcome
    /// - Parsing fails with an error about missing --next-state
    #[test]
    fn test_merge_complete_requires_next_state() {
        let result = Args::try_parse_from(["mergers", "merge", "complete"]);
        match result {
            Err(err) => {
                let err_msg = err.to_string();
                assert!(
                    err_msg.contains("--next-state"),
                    "Error should mention --next-state: {}",
                    err_msg
                );
            }
            Ok(_) => panic!("Expected parsing to fail without --next-state"),
        }
    }

    // ========================================================================
    // OutputFormat value_enum tests
    // ========================================================================

    /// # OutputFormat Enum Parsing
    ///
    /// Tests that all OutputFormat variants can be parsed from command line.
    ///
    /// ## Test Scenario
    /// - Parses merge command with --output set to each valid value
    /// - Tests text, json, and ndjson
    ///
    /// ## Expected Outcome
    /// - Each value maps to the correct OutputFormat variant
    #[test]
    fn test_output_format_enum_parsing() {
        for (input, expected) in [
            ("text", OutputFormat::Text),
            ("json", OutputFormat::Json),
            ("ndjson", OutputFormat::Ndjson),
        ] {
            let args = Args::parse_from(["mergers", "merge", "--output", input]);

            if let Some(Commands::Merge(merge_args)) = args.command {
                assert_eq!(
                    merge_args.ni.output, expected,
                    "Output format '{}' should parse to {:?}",
                    input, expected
                );
            } else {
                panic!("Expected Merge command for output '{}'", input);
            }
        }
    }

    /// # Invalid OutputFormat Value Rejected
    ///
    /// Tests that invalid --output values are rejected by clap.
    ///
    /// ## Test Scenario
    /// - Attempts to parse merge command with --output invalid_format
    ///
    /// ## Expected Outcome
    /// - Parsing fails with an error
    #[test]
    fn test_invalid_output_format_rejected() {
        let result = Args::try_parse_from(["mergers", "merge", "--output", "xml"]);
        assert!(result.is_err());
    }

    /// # OutputFormat Display Trait
    ///
    /// Tests the Display implementation for all OutputFormat variants.
    ///
    /// ## Test Scenario
    /// - Calls to_string() on each OutputFormat variant
    ///
    /// ## Expected Outcome
    /// - Text displays as "text"
    /// - Json displays as "json"
    /// - Ndjson displays as "ndjson"
    #[test]
    fn test_output_format_display() {
        assert_eq!(OutputFormat::Text.to_string(), "text");
        assert_eq!(OutputFormat::Json.to_string(), "json");
        assert_eq!(OutputFormat::Ndjson.to_string(), "ndjson");
    }

    /// # OutputFormat on Merge Subcommands
    ///
    /// Tests that --output works on continue, abort, and status subcommands.
    ///
    /// ## Test Scenario
    /// - Parses each subcommand with different output formats
    ///
    /// ## Expected Outcome
    /// - Each subcommand correctly handles its own --output flag
    #[test]
    fn test_output_format_on_merge_subcommands() {
        // Continue with ndjson
        let args = Args::parse_from(["mergers", "merge", "continue", "--output", "ndjson"]);
        if let Some(Commands::Merge(m)) = args.command {
            if let Some(MergeSubcommand::Continue(c)) = m.subcommand {
                assert_eq!(c.output, OutputFormat::Ndjson);
            } else {
                panic!("Expected Continue");
            }
        }

        // Abort with json
        let args = Args::parse_from(["mergers", "merge", "abort", "--output", "json"]);
        if let Some(Commands::Merge(m)) = args.command {
            if let Some(MergeSubcommand::Abort(a)) = m.subcommand {
                assert_eq!(a.output, OutputFormat::Json);
            } else {
                panic!("Expected Abort");
            }
        }

        // Status with text
        let args = Args::parse_from(["mergers", "merge", "status", "--output", "text"]);
        if let Some(Commands::Merge(m)) = args.command {
            if let Some(MergeSubcommand::Status(s)) = m.subcommand {
                assert_eq!(s.output, OutputFormat::Text);
            } else {
                panic!("Expected Status");
            }
        }

        // Complete with ndjson
        let args = Args::parse_from([
            "mergers",
            "merge",
            "complete",
            "--next-state",
            "Done",
            "--output",
            "ndjson",
        ]);
        if let Some(Commands::Merge(m)) = args.command {
            if let Some(MergeSubcommand::Complete(c)) = m.subcommand {
                assert_eq!(c.output, OutputFormat::Ndjson);
            } else {
                panic!("Expected Complete");
            }
        }
    }

    // ========================================================================
    // Logging argument tests
    // ========================================================================

    /// # Log Level Parsing
    ///
    /// Tests that --log-level is correctly parsed.
    ///
    /// ## Test Scenario
    /// - Parses merge command with --log-level set to various values
    ///
    /// ## Expected Outcome
    /// - log_level field contains the specified value
    #[test]
    fn test_log_level_parsing() {
        for level in ["trace", "debug", "info", "warn", "error"] {
            let args = Args::parse_from(["mergers", "merge", "--log-level", level]);

            if let Some(Commands::Merge(merge_args)) = args.command {
                assert_eq!(merge_args.shared.log_level, Some(level.to_string()));
            } else {
                panic!("Expected Merge command");
            }
        }
    }

    /// # Log File Parsing
    ///
    /// Tests that --log-file is correctly parsed.
    ///
    /// ## Test Scenario
    /// - Parses merge command with --log-file flag
    ///
    /// ## Expected Outcome
    /// - log_file field contains the specified path
    #[test]
    fn test_log_file_parsing() {
        let args = Args::parse_from(["mergers", "merge", "--log-file", "/var/log/mergers.log"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(
                merge_args.shared.log_file,
                Some("/var/log/mergers.log".to_string())
            );
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Log Format Valid Values
    ///
    /// Tests that --log-format accepts only "text" and "json".
    ///
    /// ## Test Scenario
    /// - Parses with valid log-format values (text, json)
    /// - Attempts parsing with invalid value
    ///
    /// ## Expected Outcome
    /// - Valid values are accepted
    /// - Invalid values cause parsing to fail
    #[test]
    fn test_log_format_valid_values() {
        // Test valid "text"
        let args = Args::parse_from(["mergers", "merge", "--log-format", "text"]);
        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.log_format, Some("text".to_string()));
        }

        // Test valid "json"
        let args = Args::parse_from(["mergers", "merge", "--log-format", "json"]);
        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.log_format, Some("json".to_string()));
        }
    }

    /// # Log Format Invalid Value Rejected
    ///
    /// Tests that --log-format rejects values not in the value_parser list.
    ///
    /// ## Test Scenario
    /// - Attempts to parse with --log-format yaml (not in allowed list)
    ///
    /// ## Expected Outcome
    /// - Parsing fails with an error
    #[test]
    fn test_log_format_invalid_value_rejected() {
        let result = Args::try_parse_from(["mergers", "merge", "--log-format", "yaml"]);
        assert!(result.is_err());
    }

    // ========================================================================
    // Numeric argument edge cases
    // ========================================================================

    /// # Numeric Arguments Parsed Correctly
    ///
    /// Tests that --parallel-limit, --max-concurrent-network, and
    /// --max-concurrent-processing are parsed as usize values.
    ///
    /// ## Test Scenario
    /// - Parses each numeric flag individually
    /// - Verifies values are correctly converted to usize
    ///
    /// ## Expected Outcome
    /// - Each numeric field contains the parsed value
    #[test]
    fn test_numeric_arguments_parsing() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "--parallel-limit",
            "500",
            "--max-concurrent-network",
            "200",
            "--max-concurrent-processing",
            "25",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.parallel_limit, Some(500));
            assert_eq!(merge_args.shared.max_concurrent_network, Some(200));
            assert_eq!(merge_args.shared.max_concurrent_processing, Some(25));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Numeric Argument Zero Value
    ///
    /// Tests that numeric arguments accept zero.
    ///
    /// ## Test Scenario
    /// - Parses --parallel-limit 0
    ///
    /// ## Expected Outcome
    /// - Value is parsed as 0
    #[test]
    fn test_numeric_argument_zero() {
        let args = Args::parse_from(["mergers", "merge", "--parallel-limit", "0"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.parallel_limit, Some(0));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Non-Numeric Value for Numeric Argument Rejected
    ///
    /// Tests that non-numeric values for --parallel-limit are rejected.
    ///
    /// ## Test Scenario
    /// - Attempts to parse --parallel-limit with a non-numeric string
    ///
    /// ## Expected Outcome
    /// - Parsing fails with an error about invalid value
    #[test]
    fn test_non_numeric_parallel_limit_rejected() {
        let result = Args::try_parse_from(["mergers", "merge", "--parallel-limit", "abc"]);
        assert!(result.is_err());
    }

    /// # Negative Value for Numeric Argument Rejected
    ///
    /// Tests that negative values are rejected for usize arguments.
    ///
    /// ## Test Scenario
    /// - Attempts to parse --max-concurrent-network with a negative number
    ///
    /// ## Expected Outcome
    /// - Parsing fails because usize cannot be negative
    #[test]
    fn test_negative_numeric_argument_rejected() {
        let result = Args::try_parse_from(["mergers", "merge", "--max-concurrent-network", "-5"]);
        assert!(result.is_err());
    }

    // ========================================================================
    // Boolean flag tests
    // ========================================================================

    /// # Boolean Flags Default to False
    ///
    /// Tests that all boolean flags default to false when not specified.
    ///
    /// ## Test Scenario
    /// - Parses a minimal merge command without boolean flags
    ///
    /// ## Expected Outcome
    /// - skip_confirmation, run_hooks, non_interactive, quiet all false
    #[test]
    fn test_boolean_flags_default_false() {
        let args = Args::parse_from(["mergers", "merge"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert!(!merge_args.shared.skip_confirmation);
            assert!(!merge_args.run_hooks);
            assert!(!merge_args.ni.non_interactive);
            assert!(!merge_args.ni.quiet);
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Boolean Flags Activated by Presence
    ///
    /// Tests that boolean flags are set to true when present.
    ///
    /// ## Test Scenario
    /// - Parses merge command with all boolean flags present
    ///
    /// ## Expected Outcome
    /// - All boolean flags are true
    #[test]
    fn test_boolean_flags_activated() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "--skip-confirmation",
            "--run-hooks",
            "-n",
            "-q",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert!(merge_args.shared.skip_confirmation);
            assert!(merge_args.run_hooks);
            assert!(merge_args.ni.non_interactive);
            assert!(merge_args.ni.quiet);
        } else {
            panic!("Expected Merge command");
        }
    }

    // ========================================================================
    // --create-config flag tests
    // ========================================================================

    /// # Create Config Flag
    ///
    /// Tests that --create-config flag is parsed at the top level.
    ///
    /// ## Test Scenario
    /// - Parses `mergers --create-config`
    ///
    /// ## Expected Outcome
    /// - create_config is true
    /// - No command is set
    #[test]
    fn test_create_config_flag() {
        let args = Args::parse_from(["mergers", "--create-config"]);

        assert!(args.create_config);
        assert!(args.command.is_none());
    }

    /// # Create Config Flag with Command
    ///
    /// Tests that --create-config can coexist with a command.
    ///
    /// ## Test Scenario
    /// - Parses `mergers --create-config merge`
    ///
    /// ## Expected Outcome
    /// - create_config is true
    /// - Command is still parsed
    #[test]
    fn test_create_config_flag_with_command() {
        let args = Args::parse_from(["mergers", "--create-config", "merge"]);

        assert!(args.create_config);
        assert!(matches!(args.command, Some(Commands::Merge(_))));
    }

    // ========================================================================
    // Invalid/error argument tests
    // ========================================================================

    /// # Unknown Flag Rejected
    ///
    /// Tests that unrecognized flags are rejected by clap.
    ///
    /// ## Test Scenario
    /// - Attempts to parse with an unknown --foo flag
    ///
    /// ## Expected Outcome
    /// - Parsing fails with an error
    #[test]
    fn test_unknown_flag_rejected() {
        let result = Args::try_parse_from(["mergers", "merge", "--foo", "bar"]);
        assert!(result.is_err());
    }

    /// # Unknown Subcommand Rejected
    ///
    /// Tests that an unknown subcommand is rejected.
    ///
    /// ## Test Scenario
    /// - Attempts to parse `mergers unknown`
    ///
    /// ## Expected Outcome
    /// - Parsing fails
    #[test]
    fn test_unknown_subcommand_rejected() {
        let result = Args::try_parse_from(["mergers", "unknown"]);
        assert!(result.is_err());
    }

    /// # Flag Without Required Value Rejected
    ///
    /// Tests that flags requiring values fail when no value is provided.
    ///
    /// ## Test Scenario
    /// - Attempts to parse --organization without a value
    ///
    /// ## Expected Outcome
    /// - Parsing fails with an error about missing value
    #[test]
    fn test_flag_without_required_value_rejected() {
        // --organization requires a value
        let result = Args::try_parse_from(["mergers", "merge", "--organization"]);
        assert!(result.is_err());
    }

    /// # Unknown Merge Subcommand Rejected
    ///
    /// Tests that an unknown merge subcommand is rejected.
    ///
    /// ## Test Scenario
    /// - Attempts to parse `mergers merge unknown`
    ///
    /// ## Expected Outcome
    /// - Parsing fails because "unknown" is not a valid merge subcommand
    ///   and cannot be interpreted as a path-like positional argument either
    ///   (clap will attempt positional matching first)
    #[test]
    fn test_unknown_merge_subcommand_treated_as_path() {
        // "unknown" isn't a recognized subcommand, but the positional `path`
        // argument will capture it since it's a free-form string
        let result = Args::try_parse_from(["mergers", "merge", "unknown"]);
        // This actually succeeds because "unknown" gets captured as the path positional
        assert!(result.is_ok());
        if let Ok(args) = result
            && let Some(Commands::Merge(merge_args)) = args.command
        {
            assert_eq!(merge_args.shared.path, Some("unknown".to_string()));
        }
    }

    // ========================================================================
    // Argument order tests
    // ========================================================================

    /// # Flags Can Appear in Any Order
    ///
    /// Tests that named flags can appear in any order and produce the same result.
    ///
    /// ## Test Scenario
    /// - Parses the same flags in two different orders
    /// - Compares the resulting values
    ///
    /// ## Expected Outcome
    /// - Both orderings produce identical parsed values
    #[test]
    fn test_flags_any_order() {
        // Order 1: org, proj, repo, pat
        let args1 = Args::parse_from([
            "mergers",
            "merge",
            "-o",
            "org",
            "-p",
            "proj",
            "-r",
            "repo",
            "-t",
            "pat",
            "--dev-branch",
            "develop",
        ]);

        // Order 2: pat, repo, dev-branch, proj, org (reversed and shuffled)
        let args2 = Args::parse_from([
            "mergers",
            "merge",
            "-t",
            "pat",
            "--dev-branch",
            "develop",
            "-r",
            "repo",
            "-p",
            "proj",
            "-o",
            "org",
        ]);

        let m1 = match args1.command {
            Some(Commands::Merge(m)) => m,
            _ => panic!("Expected Merge"),
        };
        let m2 = match args2.command {
            Some(Commands::Merge(m)) => m,
            _ => panic!("Expected Merge"),
        };

        assert_eq!(m1.shared.organization, m2.shared.organization);
        assert_eq!(m1.shared.project, m2.shared.project);
        assert_eq!(m1.shared.repository, m2.shared.repository);
        assert_eq!(m1.shared.pat, m2.shared.pat);
        assert_eq!(m1.shared.dev_branch, m2.shared.dev_branch);
    }

    /// # Positional Path Before and After Flags
    ///
    /// Tests that the positional path argument can appear before or after flags.
    ///
    /// ## Test Scenario
    /// - Parses path before flags and after flags
    /// - Verifies both produce the same path value
    ///
    /// ## Expected Outcome
    /// - Path is captured regardless of position relative to flags
    #[test]
    fn test_positional_path_before_and_after_flags() {
        let args_before = Args::parse_from(["mergers", "merge", "/my/path", "-o", "org"]);
        let args_after = Args::parse_from(["mergers", "merge", "-o", "org", "/my/path"]);

        let m_before = match args_before.command {
            Some(Commands::Merge(m)) => m,
            _ => panic!("Expected Merge"),
        };
        let m_after = match args_after.command {
            Some(Commands::Merge(m)) => m,
            _ => panic!("Expected Merge"),
        };

        assert_eq!(m_before.shared.path, Some("/my/path".to_string()));
        assert_eq!(m_after.shared.path, Some("/my/path".to_string()));
    }

    // ========================================================================
    // Edge case / special value tests
    // ========================================================================

    /// # Empty String Arguments
    ///
    /// Tests that empty strings are accepted as argument values.
    ///
    /// ## Test Scenario
    /// - Parses --organization with an empty string
    ///
    /// ## Expected Outcome
    /// - The field contains Some("") - clap does not reject empty strings
    #[test]
    fn test_empty_string_argument() {
        let args = Args::parse_from(["mergers", "merge", "--organization", ""]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.organization, Some("".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Arguments with Spaces
    ///
    /// Tests that argument values containing spaces are handled correctly.
    ///
    /// ## Test Scenario
    /// - Parses --work-item-state with a value containing spaces
    /// - Parses --select-by-state with spaces
    ///
    /// ## Expected Outcome
    /// - Values with spaces are preserved as-is
    #[test]
    fn test_arguments_with_spaces() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "--work-item-state",
            "Ready for Next",
            "--select-by-state",
            "Ready for Next,In Review",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(
                merge_args.work_item_state,
                Some("Ready for Next".to_string())
            );
            assert_eq!(
                merge_args.ni.select_by_state,
                Some("Ready for Next,In Review".to_string())
            );
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Arguments with Special Characters
    ///
    /// Tests that argument values with special characters are preserved.
    ///
    /// ## Test Scenario
    /// - Parses arguments with special chars like @, #, !, etc.
    ///
    /// ## Expected Outcome
    /// - Special characters are preserved in the parsed values
    #[test]
    fn test_arguments_with_special_characters() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "-o",
            "org@special#chars",
            "--tag-prefix",
            "v/release-",
            "--pat",
            "token!with$pecial",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(
                merge_args.shared.organization,
                Some("org@special#chars".to_string())
            );
            assert_eq!(merge_args.shared.tag_prefix, Some("v/release-".to_string()));
            assert_eq!(merge_args.shared.pat, Some("token!with$pecial".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Path with Relative Notation
    ///
    /// Tests that paths with . and .. are preserved as-is (no canonicalization).
    ///
    /// ## Test Scenario
    /// - Parses a relative path as the positional argument
    ///
    /// ## Expected Outcome
    /// - Path is stored exactly as provided, without resolution
    #[test]
    fn test_relative_path_preserved() {
        let args = Args::parse_from(["mergers", "merge", "../my-repo"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.path, Some("../my-repo".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Dot Path Preserved
    ///
    /// Tests that "." as a path is preserved.
    ///
    /// ## Test Scenario
    /// - Parses "." as the positional path argument
    ///
    /// ## Expected Outcome
    /// - Path contains exactly "."
    #[test]
    fn test_dot_path_preserved() {
        let args = Args::parse_from(["mergers", "merge", "."]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.path, Some(".".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    // ========================================================================
    // MergeArgsParser fallback tests
    // ========================================================================

    /// # MergeArgsParser Rejects Unknown Flags
    ///
    /// Tests that the fallback parser also rejects unknown flags.
    ///
    /// ## Test Scenario
    /// - Attempts to parse with MergeArgsParser using an unknown flag
    ///
    /// ## Expected Outcome
    /// - Parsing fails
    #[test]
    fn test_merge_args_parser_rejects_unknown_flags() {
        let result = MergeArgsParser::try_parse_from(["mergers", "--unknown-flag", "value"]);
        assert!(result.is_err());
    }

    /// # MergeArgsParser with Minimal Args
    ///
    /// Tests that MergeArgsParser works with no arguments at all (all defaults).
    ///
    /// ## Test Scenario
    /// - Parses just "mergers" with MergeArgsParser
    ///
    /// ## Expected Outcome
    /// - Parsing succeeds with all defaults
    #[test]
    fn test_merge_args_parser_minimal() {
        let result = MergeArgsParser::try_parse_from(["mergers"]);

        assert!(result.is_ok());
        let parser = result.unwrap();
        assert_eq!(parser.merge_args.shared.path, None);
        assert_eq!(parser.merge_args.shared.organization, None);
        assert!(!parser.merge_args.ni.non_interactive);
        assert_eq!(
            parser.merge_args.shared.tag_prefix,
            Some("merged-".to_string())
        );
    }

    // ========================================================================
    // Branch configuration tests
    // ========================================================================

    /// # Dev Branch and Target Branch Parsing
    ///
    /// Tests that --dev-branch and --target-branch are correctly parsed.
    ///
    /// ## Test Scenario
    /// - Parses merge command with custom branch names
    ///
    /// ## Expected Outcome
    /// - Both branches are captured with their specified values
    #[test]
    fn test_branch_configuration_parsing() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "--dev-branch",
            "develop",
            "--target-branch",
            "release/v3",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.dev_branch, Some("develop".to_string()));
            assert_eq!(
                merge_args.shared.target_branch,
                Some("release/v3".to_string())
            );
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Branch Names with Slashes
    ///
    /// Tests that branch names containing slashes are preserved.
    ///
    /// ## Test Scenario
    /// - Parses branch names like feature/my-branch
    ///
    /// ## Expected Outcome
    /// - Branch names with slashes are stored as-is
    #[test]
    fn test_branch_names_with_slashes() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "--dev-branch",
            "feature/my-branch",
            "--target-branch",
            "release/2024/q1",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(
                merge_args.shared.dev_branch,
                Some("feature/my-branch".to_string())
            );
            assert_eq!(
                merge_args.shared.target_branch,
                Some("release/2024/q1".to_string())
            );
        } else {
            panic!("Expected Merge command");
        }
    }

    // ========================================================================
    // Migrate command detailed tests
    // ========================================================================

    /// # Migrate with Custom Terminal States
    ///
    /// Tests that --terminal-states overrides the default value.
    ///
    /// ## Test Scenario
    /// - Parses migrate command with custom terminal states
    ///
    /// ## Expected Outcome
    /// - terminal_states contains the custom value
    #[test]
    fn test_migrate_custom_terminal_states() {
        let args = Args::parse_from(["mergers", "migrate", "--terminal-states", "Done,Resolved"]);

        if let Some(Commands::Migrate(migrate_args)) = args.command {
            assert_eq!(migrate_args.terminal_states, "Done,Resolved");
        } else {
            panic!("Expected Migrate command");
        }
    }

    /// # Migrate with All Shared Args
    ///
    /// Tests that migrate command correctly receives all shared arguments.
    ///
    /// ## Test Scenario
    /// - Parses migrate with shared args and migrate-specific args
    ///
    /// ## Expected Outcome
    /// - All shared args and terminal_states are correctly populated
    #[test]
    fn test_migrate_with_all_shared_args() {
        let args = Args::parse_from([
            "mergers",
            "migrate",
            "-o",
            "org",
            "-p",
            "proj",
            "-r",
            "repo",
            "-t",
            "pat",
            "--dev-branch",
            "dev",
            "--target-branch",
            "next",
            "--parallel-limit",
            "100",
            "--since",
            "2w",
            "--skip-confirmation",
            "--terminal-states",
            "Done",
            "/path/to/repo",
        ]);

        if let Some(Commands::Migrate(migrate_args)) = args.command {
            assert_eq!(migrate_args.shared.organization, Some("org".to_string()));
            assert_eq!(migrate_args.shared.project, Some("proj".to_string()));
            assert_eq!(migrate_args.shared.repository, Some("repo".to_string()));
            assert_eq!(migrate_args.shared.pat, Some("pat".to_string()));
            assert_eq!(migrate_args.shared.dev_branch, Some("dev".to_string()));
            assert_eq!(migrate_args.shared.target_branch, Some("next".to_string()));
            assert_eq!(migrate_args.shared.parallel_limit, Some(100));
            assert_eq!(migrate_args.shared.since, Some("2w".to_string()));
            assert!(migrate_args.shared.skip_confirmation);
            assert_eq!(migrate_args.terminal_states, "Done");
            assert_eq!(migrate_args.shared.path, Some("/path/to/repo".to_string()));
        } else {
            panic!("Expected Migrate command");
        }
    }

    // ========================================================================
    // No subcommand (bare mergers invocation) tests
    // ========================================================================

    /// # Bare Invocation Has No Command
    ///
    /// Tests that `mergers` with no arguments produces command: None.
    ///
    /// ## Test Scenario
    /// - Parses just "mergers" with no args
    ///
    /// ## Expected Outcome
    /// - command is None, create_config is false
    #[test]
    fn test_bare_invocation_no_command() {
        let args = Args::parse_from(["mergers"]);

        assert!(args.command.is_none());
        assert!(!args.create_config);
    }

    // ========================================================================
    // Merge-specific flag tests
    // ========================================================================

    /// # Work Item State Parsing
    ///
    /// Tests that --work-item-state is correctly parsed on merge command.
    ///
    /// ## Test Scenario
    /// - Parses merge with --work-item-state
    /// - Tests that it's None when not provided
    ///
    /// ## Expected Outcome
    /// - work_item_state contains the specified value when provided
    /// - work_item_state is None when not provided
    #[test]
    fn test_work_item_state_parsing() {
        // With value
        let args = Args::parse_from(["mergers", "merge", "--work-item-state", "Next Merged"]);
        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.work_item_state, Some("Next Merged".to_string()));
        } else {
            panic!("Expected Merge command");
        }

        // Without value (should be None)
        let args = Args::parse_from(["mergers", "merge"]);
        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.work_item_state, None);
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Run Hooks Flag Parsing
    ///
    /// Tests that --run-hooks flag activates hook running.
    ///
    /// ## Test Scenario
    /// - Parses merge with and without --run-hooks
    ///
    /// ## Expected Outcome
    /// - run_hooks is true when flag present, false when absent
    #[test]
    fn test_run_hooks_flag_parsing() {
        let args_with = Args::parse_from(["mergers", "merge", "--run-hooks"]);
        let args_without = Args::parse_from(["mergers", "merge"]);

        if let Some(Commands::Merge(m)) = args_with.command {
            assert!(m.run_hooks);
        }
        if let Some(Commands::Merge(m)) = args_without.command {
            assert!(!m.run_hooks);
        }
    }

    /// # Select By State Parsing
    ///
    /// Tests that --select-by-state is correctly parsed.
    ///
    /// ## Test Scenario
    /// - Parses merge with --select-by-state containing comma-separated values
    ///
    /// ## Expected Outcome
    /// - select_by_state contains the full comma-separated string
    #[test]
    fn test_select_by_state_parsing() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "--select-by-state",
            "Ready for Next,In Progress",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(
                merge_args.ni.select_by_state,
                Some("Ready for Next,In Progress".to_string())
            );
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Version Flag Parsing
    ///
    /// Tests that --version on merge (not the binary --version) is parsed.
    ///
    /// ## Test Scenario
    /// - Parses merge command with --version value
    ///
    /// ## Expected Outcome
    /// - version field contains the specified value
    #[test]
    fn test_merge_version_flag_parsing() {
        let args = Args::parse_from(["mergers", "merge", "--version", "v3.2.1"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.ni.version, Some("v3.2.1".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    // ========================================================================
    // Quiet flag on subcommands tests
    // ========================================================================

    /// # Quiet Flag on Continue Subcommand
    ///
    /// Tests that -q works on merge continue subcommand.
    ///
    /// ## Test Scenario
    /// - Parses `merge continue -q`
    ///
    /// ## Expected Outcome
    /// - quiet is true on the continue args
    #[test]
    fn test_quiet_flag_on_continue() {
        let args = Args::parse_from(["mergers", "merge", "continue", "-q"]);

        if let Some(Commands::Merge(m)) = args.command {
            if let Some(MergeSubcommand::Continue(c)) = m.subcommand {
                assert!(c.quiet);
            } else {
                panic!("Expected Continue");
            }
        } else {
            panic!("Expected Merge");
        }
    }

    /// # Quiet Flag on Complete Subcommand
    ///
    /// Tests that -q works on merge complete subcommand.
    ///
    /// ## Test Scenario
    /// - Parses `merge complete --next-state Done -q`
    ///
    /// ## Expected Outcome
    /// - quiet is true on the complete args
    #[test]
    fn test_quiet_flag_on_complete() {
        let args = Args::parse_from(["mergers", "merge", "complete", "--next-state", "Done", "-q"]);

        if let Some(Commands::Merge(m)) = args.command {
            if let Some(MergeSubcommand::Complete(c)) = m.subcommand {
                assert!(c.quiet);
                assert_eq!(c.next_state, "Done");
            } else {
                panic!("Expected Complete");
            }
        } else {
            panic!("Expected Merge");
        }
    }

    // ========================================================================
    // Commands::shared_args_mut tests
    // ========================================================================

    /// # Commands Shared Args Mut for All Variants
    ///
    /// Tests that Commands::shared_args_mut() works for all command variants.
    ///
    /// ## Test Scenario
    /// - Creates each command variant
    /// - Mutates shared args through the trait method
    ///
    /// ## Expected Outcome
    /// - Mutations are visible through shared_args()
    #[test]
    fn test_commands_shared_args_mut_all_variants() {
        let mut merge_cmd = Commands::Merge(MergeArgs {
            shared: SharedArgs::default(),
            ni: NonInteractiveArgs::default(),
            work_item_state: None,
            run_hooks: false,
            subcommand: None,
        });
        merge_cmd.shared_args_mut().organization = Some("mutated".to_string());
        assert_eq!(
            merge_cmd.shared_args().organization,
            Some("mutated".to_string())
        );

        let mut migrate_cmd = Commands::Migrate(MigrateArgs {
            shared: SharedArgs::default(),
            terminal_states: "Closed".to_string(),
        });
        migrate_cmd.shared_args_mut().project = Some("mutated".to_string());
        assert_eq!(
            migrate_cmd.shared_args().project,
            Some("mutated".to_string())
        );

        let mut cleanup_cmd = Commands::Cleanup(CleanupArgs {
            shared: SharedArgs::default(),
            target: None,
        });
        cleanup_cmd.shared_args_mut().repository = Some("mutated".to_string());
        assert_eq!(
            cleanup_cmd.shared_args().repository,
            Some("mutated".to_string())
        );
    }

    // ========================================================================
    // Comprehensive integration: parse then resolve
    // ========================================================================

    /// # Cleanup with Shared Args Resolves Config
    ///
    /// Tests that a cleanup command with required fields resolves config.
    ///
    /// ## Test Scenario
    /// - Creates Args with Cleanup command and required shared fields
    /// - Resolves config
    ///
    /// ## Expected Outcome
    /// - Config resolution succeeds for cleanup mode
    #[test]
    fn test_cleanup_resolves_config() {
        let args = Args {
            command: Some(Commands::Cleanup(CleanupArgs {
                shared: SharedArgs {
                    organization: Some("org".to_string()),
                    project: Some("proj".to_string()),
                    repository: Some("repo".to_string()),
                    pat: Some("pat".to_string()),
                    ..Default::default()
                },
                target: Some("main".to_string()),
            })),
            create_config: false,
        };

        let result = args.resolve_config();
        assert!(result.is_ok());
        let config = result.unwrap();
        assert!(!config.is_migration_mode());
    }

    /// # Full Parse-to-Config Pipeline for Merge
    ///
    /// Tests the full pipeline: parse_from -> resolve_config for merge command.
    ///
    /// ## Test Scenario
    /// - Parses a complete merge command from CLI args
    /// - Resolves the configuration
    /// - Verifies CLI values are marked as ParsedProperty::Cli
    ///
    /// ## Expected Outcome
    /// - Config resolves successfully
    /// - CLI values have Cli source annotation
    #[test]
    fn test_full_parse_to_config_pipeline_merge() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "-o",
            "test-org",
            "-p",
            "test-proj",
            "-r",
            "test-repo",
            "-t",
            "test-pat",
            "--dev-branch",
            "develop",
            "--target-branch",
            "release",
            "--parallel-limit",
            "150",
        ]);

        let result = args.resolve_config();
        assert!(result.is_ok());

        let config = result.unwrap();
        assert!(!config.is_migration_mode());
        assert_eq!(
            config.shared().organization,
            ParsedProperty::Cli("test-org".to_string(), "test-org".to_string())
        );
        assert_eq!(
            config.shared().dev_branch,
            ParsedProperty::Cli("develop".to_string(), "develop".to_string())
        );
        assert_eq!(
            config.shared().parallel_limit,
            ParsedProperty::Cli(150, "150".to_string())
        );
    }

    /// # Full Parse-to-Config Pipeline for Migrate
    ///
    /// Tests the full pipeline: parse_from -> resolve_config for migrate command.
    ///
    /// ## Test Scenario
    /// - Parses a migrate command from CLI args with custom terminal states
    /// - Resolves the configuration
    ///
    /// ## Expected Outcome
    /// - Config resolves to migration mode
    /// - Terminal states are correctly split and annotated as Cli source
    #[test]
    fn test_full_parse_to_config_pipeline_migrate() {
        let args = Args::parse_from([
            "mergers",
            "migrate",
            "-o",
            "org",
            "-p",
            "proj",
            "-r",
            "repo",
            "-t",
            "pat",
            "--terminal-states",
            "Done,Resolved,Closed",
        ]);

        let result = args.resolve_config();
        assert!(result.is_ok());

        let config = result.unwrap();
        assert!(config.is_migration_mode());

        if let AppConfig::Migration { migration, .. } = config {
            assert_eq!(
                migration.terminal_states,
                ParsedProperty::Cli(
                    vec![
                        "Done".to_string(),
                        "Resolved".to_string(),
                        "Closed".to_string()
                    ],
                    "Done,Resolved,Closed".to_string()
                )
            );
        } else {
            panic!("Expected migration config");
        }
    }

    // ========================================================================
    // Logging args on non-merge commands
    // ========================================================================

    /// # Logging Args on Migrate Command
    ///
    /// Tests that logging arguments are available on migrate command.
    ///
    /// ## Test Scenario
    /// - Parses migrate with logging flags
    ///
    /// ## Expected Outcome
    /// - Logging fields are populated on migrate's shared args
    #[test]
    fn test_logging_args_on_migrate() {
        let args = Args::parse_from([
            "mergers",
            "migrate",
            "--log-level",
            "debug",
            "--log-file",
            "/tmp/migrate.log",
            "--log-format",
            "json",
        ]);

        if let Some(Commands::Migrate(migrate_args)) = args.command {
            assert_eq!(migrate_args.shared.log_level, Some("debug".to_string()));
            assert_eq!(
                migrate_args.shared.log_file,
                Some("/tmp/migrate.log".to_string())
            );
            assert_eq!(migrate_args.shared.log_format, Some("json".to_string()));
        } else {
            panic!("Expected Migrate command");
        }
    }

    /// # Logging Args on Cleanup Command
    ///
    /// Tests that logging arguments are available on cleanup command.
    ///
    /// ## Test Scenario
    /// - Parses cleanup with --log-level
    ///
    /// ## Expected Outcome
    /// - log_level is populated on cleanup's shared args
    #[test]
    fn test_logging_args_on_cleanup() {
        let args = Args::parse_from(["mergers", "cleanup", "--log-level", "warn"]);

        if let Some(Commands::Cleanup(cleanup_args)) = args.command {
            assert_eq!(cleanup_args.shared.log_level, Some("warn".to_string()));
        } else {
            panic!("Expected Cleanup command");
        }
    }

    // ========================================================================
    // Since parameter parsing tests
    // ========================================================================

    /// # Since Parameter Various Formats
    ///
    /// Tests that --since accepts various date format strings.
    ///
    /// ## Test Scenario
    /// - Parses --since with different format strings (relative dates, ISO dates)
    ///
    /// ## Expected Outcome
    /// - All formats are accepted as-is (validation happens later in config resolution)
    #[test]
    fn test_since_parameter_various_formats() {
        for since_val in ["1mo", "2w", "3d", "6mo", "2025-01-15"] {
            let args = Args::parse_from(["mergers", "merge", "--since", since_val]);

            if let Some(Commands::Merge(merge_args)) = args.command {
                assert_eq!(
                    merge_args.shared.since,
                    Some(since_val.to_string()),
                    "Since value '{}' should be preserved",
                    since_val
                );
            } else {
                panic!("Expected Merge command for since '{}'", since_val);
            }
        }
    }

    // ========================================================================
    // Concurrent network/processing on different commands
    // ========================================================================

    /// # Performance Tuning Args on Migrate
    ///
    /// Tests that performance tuning arguments work on the migrate command.
    ///
    /// ## Test Scenario
    /// - Parses migrate with --parallel-limit, --max-concurrent-network,
    ///   --max-concurrent-processing
    ///
    /// ## Expected Outcome
    /// - All numeric performance args are captured correctly
    #[test]
    fn test_performance_tuning_on_migrate() {
        let args = Args::parse_from([
            "mergers",
            "migrate",
            "--parallel-limit",
            "400",
            "--max-concurrent-network",
            "150",
            "--max-concurrent-processing",
            "20",
        ]);

        if let Some(Commands::Migrate(migrate_args)) = args.command {
            assert_eq!(migrate_args.shared.parallel_limit, Some(400));
            assert_eq!(migrate_args.shared.max_concurrent_network, Some(150));
            assert_eq!(migrate_args.shared.max_concurrent_processing, Some(20));
        } else {
            panic!("Expected Migrate command");
        }
    }

    // ========================================================================
    // Tag prefix tests
    // ========================================================================

    /// # Custom Tag Prefix
    ///
    /// Tests that --tag-prefix overrides the default "merged-" value.
    ///
    /// ## Test Scenario
    /// - Parses merge with a custom --tag-prefix
    ///
    /// ## Expected Outcome
    /// - tag_prefix contains the custom value
    #[test]
    fn test_custom_tag_prefix() {
        let args = Args::parse_from(["mergers", "merge", "--tag-prefix", "release-v"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.tag_prefix, Some("release-v".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Tag Prefix Default Value at Parse Time
    ///
    /// Tests that tag_prefix gets its default "merged-" from clap parse.
    ///
    /// ## Test Scenario
    /// - Parses merge without --tag-prefix
    /// - Checks the default value was applied
    ///
    /// ## Expected Outcome
    /// - tag_prefix is Some("merged-") (clap default_value applied at parse)
    #[test]
    fn test_tag_prefix_default_at_parse_time() {
        let args = Args::parse_from(["mergers", "merge"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.tag_prefix, Some("merged-".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    // ========================================================================
    // Skip confirmation on different commands
    // ========================================================================

    /// # Skip Confirmation on Migrate
    ///
    /// Tests that --skip-confirmation works on the migrate command.
    ///
    /// ## Test Scenario
    /// - Parses migrate with --skip-confirmation
    ///
    /// ## Expected Outcome
    /// - skip_confirmation is true on migrate's shared args
    #[test]
    fn test_skip_confirmation_on_migrate() {
        let args = Args::parse_from(["mergers", "migrate", "--skip-confirmation"]);

        if let Some(Commands::Migrate(migrate_args)) = args.command {
            assert!(migrate_args.shared.skip_confirmation);
        } else {
            panic!("Expected Migrate command");
        }
    }

    /// # Skip Confirmation on Cleanup
    ///
    /// Tests that --skip-confirmation works on the cleanup command.
    ///
    /// ## Test Scenario
    /// - Parses cleanup with --skip-confirmation
    ///
    /// ## Expected Outcome
    /// - skip_confirmation is true on cleanup's shared args
    #[test]
    fn test_skip_confirmation_on_cleanup() {
        let args = Args::parse_from(["mergers", "cleanup", "--skip-confirmation"]);

        if let Some(Commands::Cleanup(cleanup_args)) = args.command {
            assert!(cleanup_args.shared.skip_confirmation);
        } else {
            panic!("Expected Cleanup command");
        }
    }

    // ========================================================================
    // ReleaseNotes command parsing tests
    // ========================================================================

    /// # Release Notes Basic Command Parsing
    ///
    /// Tests that `mergers release-notes` is recognized as the ReleaseNotes command.
    ///
    /// ## Test Scenario
    /// - Parses `mergers release-notes` with no extra flags
    ///
    /// ## Expected Outcome
    /// - Command is recognized as Commands::ReleaseNotes
    #[test]
    fn test_release_notes_command_basic_parsing() {
        let args = Args::parse_from(["mergers", "release-notes"]);

        assert!(matches!(args.command, Some(Commands::ReleaseNotes(_))));
    }

    /// # Release Notes Command Alias
    ///
    /// Tests that the 'rn' alias correctly parses as release-notes command.
    ///
    /// ## Test Scenario
    /// - Parses `mergers rn`
    ///
    /// ## Expected Outcome
    /// - The alias 'rn' is recognized as ReleaseNotes command
    #[test]
    fn test_release_notes_command_alias_rn() {
        let args = Args::parse_from(["mergers", "rn"]);

        assert!(matches!(args.command, Some(Commands::ReleaseNotes(_))));
    }

    /// # Release Notes with Shared Args
    ///
    /// Tests that shared arguments (-o, -p, -r, -t) work on release-notes.
    ///
    /// ## Test Scenario
    /// - Parses release-notes with all short shared flags
    ///
    /// ## Expected Outcome
    /// - All shared arg fields are correctly populated
    #[test]
    fn test_release_notes_with_shared_args() {
        let args = Args::parse_from([
            "mergers",
            "release-notes",
            "-o",
            "my-org",
            "-p",
            "my-proj",
            "-r",
            "my-repo",
            "-t",
            "my-token",
        ]);

        if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
            assert_eq!(rn_args.shared.organization, Some("my-org".to_string()));
            assert_eq!(rn_args.shared.project, Some("my-proj".to_string()));
            assert_eq!(rn_args.shared.repository, Some("my-repo".to_string()));
            assert_eq!(rn_args.shared.pat, Some("my-token".to_string()));
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    /// # Release Notes with Positional Path
    ///
    /// Tests that a positional path argument is captured on release-notes.
    ///
    /// ## Test Scenario
    /// - Parses `mergers release-notes /path/to/repo`
    ///
    /// ## Expected Outcome
    /// - Path is captured in shared.path
    #[test]
    fn test_release_notes_with_positional_path() {
        let args = Args::parse_from(["mergers", "release-notes", "/path/to/repo"]);

        if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
            assert_eq!(rn_args.shared.path, Some("/path/to/repo".to_string()));
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    /// # Release Notes All Fields Explicit
    ///
    /// Tests that every release-notes flag is correctly parsed when explicitly set.
    ///
    /// ## Test Scenario
    /// - Parses release-notes with all possible flags
    ///
    /// ## Expected Outcome
    /// - Every field has the explicit value provided
    #[test]
    fn test_release_notes_all_fields_explicit() {
        let args = Args::parse_from([
            "mergers",
            "release-notes",
            "--output",
            "json",
            "--copy",
            "--group",
            "--include-prs",
            "--from",
            "v1.0.0",
            "--to",
            "v2.0.0",
            "--no-cache",
            "-o",
            "org",
            "-p",
            "proj",
            "-r",
            "repo",
            "-t",
            "pat",
            "--dev-branch",
            "develop",
            "--target-branch",
            "release",
            "--tag-prefix",
            "released-",
            "--since",
            "2w",
            "--skip-confirmation",
            "/path/to/repo",
        ]);

        if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
            // Release-notes specific
            assert_eq!(rn_args.output, ReleaseNotesOutputFormat::Json);
            assert!(rn_args.copy);
            assert!(rn_args.group);
            assert!(rn_args.include_prs);
            assert_eq!(rn_args.from, Some("v1.0.0".to_string()));
            assert_eq!(rn_args.to, Some("v2.0.0".to_string()));
            assert!(rn_args.no_cache);
            // Shared args
            assert_eq!(rn_args.shared.organization, Some("org".to_string()));
            assert_eq!(rn_args.shared.project, Some("proj".to_string()));
            assert_eq!(rn_args.shared.repository, Some("repo".to_string()));
            assert_eq!(rn_args.shared.pat, Some("pat".to_string()));
            assert_eq!(rn_args.shared.dev_branch, Some("develop".to_string()));
            assert_eq!(rn_args.shared.target_branch, Some("release".to_string()));
            assert_eq!(rn_args.shared.tag_prefix, Some("released-".to_string()));
            assert_eq!(rn_args.shared.since, Some("2w".to_string()));
            assert!(rn_args.shared.skip_confirmation);
            assert_eq!(rn_args.shared.path, Some("/path/to/repo".to_string()));
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    /// # Release Notes Default Values
    ///
    /// Tests that release-notes fields use correct defaults when no flags provided.
    ///
    /// ## Test Scenario
    /// - Parses `mergers release-notes` with no optional flags
    ///
    /// ## Expected Outcome
    /// - output defaults to Markdown, booleans to false, from/to to None
    #[test]
    fn test_release_notes_default_values() {
        let args = Args::parse_from(["mergers", "release-notes"]);

        if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
            assert_eq!(rn_args.output, ReleaseNotesOutputFormat::Markdown);
            assert!(!rn_args.copy);
            assert!(!rn_args.group);
            assert!(!rn_args.include_prs);
            assert_eq!(rn_args.from, None);
            assert_eq!(rn_args.to, None);
            assert!(!rn_args.no_cache);
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    /// # Release Notes Boolean Flags Default False
    ///
    /// Tests that all boolean flags on release-notes default to false.
    ///
    /// ## Test Scenario
    /// - Parses `mergers rn` with no boolean flags
    ///
    /// ## Expected Outcome
    /// - copy, group, include_prs, no_cache are all false
    #[test]
    fn test_release_notes_boolean_flags_default_false() {
        let args = Args::parse_from(["mergers", "rn"]);

        if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
            assert!(!rn_args.copy);
            assert!(!rn_args.group);
            assert!(!rn_args.include_prs);
            assert!(!rn_args.no_cache);
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    /// # Release Notes Boolean Flags Activated
    ///
    /// Tests that all boolean flags are true when present.
    ///
    /// ## Test Scenario
    /// - Parses release-notes with all boolean flags
    ///
    /// ## Expected Outcome
    /// - copy, group, include_prs, no_cache are all true
    #[test]
    fn test_release_notes_boolean_flags_activated() {
        let args = Args::parse_from([
            "mergers",
            "rn",
            "--copy",
            "--group",
            "--include-prs",
            "--no-cache",
        ]);

        if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
            assert!(rn_args.copy);
            assert!(rn_args.group);
            assert!(rn_args.include_prs);
            assert!(rn_args.no_cache);
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    /// # Release Notes From and To Version Range
    ///
    /// Tests that --from and --to capture version range.
    ///
    /// ## Test Scenario
    /// - Parses `mergers rn --from v1.0.0 --to v2.0.0`
    ///
    /// ## Expected Outcome
    /// - Both from and to contain the specified versions
    #[test]
    fn test_release_notes_from_and_to_version_range() {
        let args = Args::parse_from(["mergers", "rn", "--from", "v1.0.0", "--to", "v2.0.0"]);

        if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
            assert_eq!(rn_args.from, Some("v1.0.0".to_string()));
            assert_eq!(rn_args.to, Some("v2.0.0".to_string()));
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    /// # Release Notes From Without To
    ///
    /// Tests that --from can be specified alone (--to defaults to HEAD at runtime).
    ///
    /// ## Test Scenario
    /// - Parses `mergers rn --from v1.0.0`
    ///
    /// ## Expected Outcome
    /// - from is Some, to is None
    #[test]
    fn test_release_notes_from_without_to() {
        let args = Args::parse_from(["mergers", "rn", "--from", "v1.0.0"]);

        if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
            assert_eq!(rn_args.from, Some("v1.0.0".to_string()));
            assert_eq!(rn_args.to, None);
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    /// # Release Notes To Without From
    ///
    /// Tests that --to can be specified alone.
    ///
    /// ## Test Scenario
    /// - Parses `mergers rn --to v2.0.0`
    ///
    /// ## Expected Outcome
    /// - from is None, to is Some
    #[test]
    fn test_release_notes_to_without_from() {
        let args = Args::parse_from(["mergers", "rn", "--to", "v2.0.0"]);

        if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
            assert_eq!(rn_args.from, None);
            assert_eq!(rn_args.to, Some("v2.0.0".to_string()));
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    /// # Release Notes Path Before and After Flags
    ///
    /// Tests that the positional path works regardless of position relative to flags.
    ///
    /// ## Test Scenario
    /// - Parses path before flags and after flags
    ///
    /// ## Expected Outcome
    /// - Path is captured in both orderings
    #[test]
    fn test_release_notes_path_before_and_after_flags() {
        // Path before flags
        let args_before =
            Args::parse_from(["mergers", "rn", "/my/repo", "--from", "v1.0", "--group"]);
        // Path after flags
        let args_after =
            Args::parse_from(["mergers", "rn", "--from", "v1.0", "--group", "/my/repo"]);

        if let Some(Commands::ReleaseNotes(rn_before)) = args_before.command {
            assert_eq!(rn_before.shared.path, Some("/my/repo".to_string()));
        } else {
            panic!("Expected ReleaseNotes command (before)");
        }

        if let Some(Commands::ReleaseNotes(rn_after)) = args_after.command {
            assert_eq!(rn_after.shared.path, Some("/my/repo".to_string()));
        } else {
            panic!("Expected ReleaseNotes command (after)");
        }
    }

    // ========================================================================
    // ReleaseNotesOutputFormat enum tests
    // ========================================================================

    /// # Release Notes Output Format All Values
    ///
    /// Tests that all ReleaseNotesOutputFormat values parse correctly.
    ///
    /// ## Test Scenario
    /// - Parses --output with each valid value (markdown, json, plain)
    ///
    /// ## Expected Outcome
    /// - Each string maps to the correct enum variant
    #[test]
    fn test_release_notes_output_format_all_values() {
        for (input, expected) in [
            ("markdown", ReleaseNotesOutputFormat::Markdown),
            ("json", ReleaseNotesOutputFormat::Json),
            ("plain", ReleaseNotesOutputFormat::Plain),
        ] {
            let args = Args::parse_from(["mergers", "rn", "--output", input]);

            if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
                assert_eq!(
                    rn_args.output, expected,
                    "Output format '{}' should parse to {:?}",
                    input, expected
                );
            } else {
                panic!("Expected ReleaseNotes command for output '{}'", input);
            }
        }
    }

    /// # Release Notes Output Format Invalid Rejected
    ///
    /// Tests that invalid --output values are rejected.
    ///
    /// ## Test Scenario
    /// - Attempts to parse --output with invalid value "xml"
    ///
    /// ## Expected Outcome
    /// - Parsing fails
    #[test]
    fn test_release_notes_output_format_invalid_rejected() {
        let result = Args::try_parse_from(["mergers", "rn", "--output", "xml"]);
        assert!(result.is_err());
    }

    /// # Release Notes Output Format Default Markdown
    ///
    /// Tests that output defaults to Markdown when --output is not specified.
    ///
    /// ## Test Scenario
    /// - Parses release-notes without --output
    ///
    /// ## Expected Outcome
    /// - output field is ReleaseNotesOutputFormat::Markdown
    #[test]
    fn test_release_notes_output_format_default_markdown() {
        let args = Args::parse_from(["mergers", "rn"]);

        if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
            assert_eq!(rn_args.output, ReleaseNotesOutputFormat::Markdown);
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    /// # Release Notes Output Format Display
    ///
    /// Tests the Display implementation for ReleaseNotesOutputFormat.
    ///
    /// ## Test Scenario
    /// - Calls to_string() on each variant
    ///
    /// ## Expected Outcome
    /// - Markdown -> "markdown", Json -> "json", Plain -> "plain"
    #[test]
    fn test_release_notes_output_format_display() {
        assert_eq!(ReleaseNotesOutputFormat::Markdown.to_string(), "markdown");
        assert_eq!(ReleaseNotesOutputFormat::Json.to_string(), "json");
        assert_eq!(ReleaseNotesOutputFormat::Plain.to_string(), "plain");
    }

    // ========================================================================
    // TaskGroup enum tests
    // ========================================================================

    /// # TaskGroup All Variants
    ///
    /// Tests that all TaskGroup variants can be constructed and matched.
    ///
    /// ## Test Scenario
    /// - Creates each TaskGroup variant
    /// - Pattern matches to verify correctness
    ///
    /// ## Expected Outcome
    /// - All variants are constructible and matchable
    #[test]
    fn test_task_group_all_variants() {
        let groups = [
            TaskGroup::Feature,
            TaskGroup::Fix,
            TaskGroup::Refactor,
            TaskGroup::Other,
        ];

        assert!(matches!(groups[0], TaskGroup::Feature));
        assert!(matches!(groups[1], TaskGroup::Fix));
        assert!(matches!(groups[2], TaskGroup::Refactor));
        assert!(matches!(groups[3], TaskGroup::Other));
    }

    /// # TaskGroup Default Is Other
    ///
    /// Tests that TaskGroup::default() returns Other.
    ///
    /// ## Test Scenario
    /// - Creates a default TaskGroup
    ///
    /// ## Expected Outcome
    /// - Default variant is Other
    #[test]
    fn test_task_group_default_is_other() {
        assert_eq!(TaskGroup::default(), TaskGroup::Other);
    }

    /// # TaskGroup Display
    ///
    /// Tests the Display implementation for all TaskGroup variants.
    ///
    /// ## Test Scenario
    /// - Calls to_string() on each variant
    ///
    /// ## Expected Outcome
    /// - Feature -> "Features", Fix -> "Fixes", Refactor -> "Refactors", Other -> "Other"
    #[test]
    fn test_task_group_display() {
        assert_eq!(TaskGroup::Feature.to_string(), "Features");
        assert_eq!(TaskGroup::Fix.to_string(), "Fixes");
        assert_eq!(TaskGroup::Refactor.to_string(), "Refactors");
        assert_eq!(TaskGroup::Other.to_string(), "Other");
    }

    // ========================================================================
    // Commands::ReleaseNotes integration tests
    // ========================================================================

    /// # Commands is_release_notes True
    ///
    /// Tests that is_release_notes() returns true for ReleaseNotes command.
    ///
    /// ## Test Scenario
    /// - Creates a Commands::ReleaseNotes variant
    /// - Calls is_release_notes()
    ///
    /// ## Expected Outcome
    /// - Returns true
    #[test]
    fn test_commands_is_release_notes_true() {
        let cmd = Commands::ReleaseNotes(ReleaseNotesArgs {
            shared: SharedArgs::default(),
            output: ReleaseNotesOutputFormat::Markdown,
            copy: false,
            group: false,
            include_prs: false,
            from: None,
            to: None,
            no_cache: false,
        });

        assert!(cmd.is_release_notes());
    }

    /// # Commands is_release_notes False for Others
    ///
    /// Tests that is_release_notes() returns false for non-ReleaseNotes commands.
    ///
    /// ## Test Scenario
    /// - Creates Merge, Migrate, and Cleanup commands
    /// - Calls is_release_notes() on each
    ///
    /// ## Expected Outcome
    /// - Returns false for all three
    #[test]
    fn test_commands_is_release_notes_false_for_others() {
        let merge_cmd = Commands::Merge(MergeArgs {
            shared: SharedArgs::default(),
            ni: NonInteractiveArgs::default(),
            work_item_state: None,
            run_hooks: false,
            subcommand: None,
        });
        let migrate_cmd = Commands::Migrate(MigrateArgs {
            shared: SharedArgs::default(),
            terminal_states: "Closed".to_string(),
        });
        let cleanup_cmd = Commands::Cleanup(CleanupArgs {
            shared: SharedArgs::default(),
            target: None,
        });

        assert!(!merge_cmd.is_release_notes());
        assert!(!migrate_cmd.is_release_notes());
        assert!(!cleanup_cmd.is_release_notes());
    }

    /// # Commands Shared Args Extraction for ReleaseNotes
    ///
    /// Tests that Commands::shared_args() works for the ReleaseNotes variant.
    ///
    /// ## Test Scenario
    /// - Creates a Commands::ReleaseNotes with organization set
    /// - Extracts shared args via Commands::shared_args()
    ///
    /// ## Expected Outcome
    /// - Organization field is correctly extracted
    #[test]
    fn test_commands_shared_args_extraction_release_notes() {
        let rn_cmd = Commands::ReleaseNotes(ReleaseNotesArgs {
            shared: SharedArgs {
                organization: Some("rn-org".to_string()),
                ..Default::default()
            },
            output: ReleaseNotesOutputFormat::Markdown,
            copy: false,
            group: false,
            include_prs: false,
            from: None,
            to: None,
            no_cache: false,
        });

        assert_eq!(
            rn_cmd.shared_args().organization,
            Some("rn-org".to_string())
        );
    }

    /// # Commands Shared Args Mut for ReleaseNotes
    ///
    /// Tests that Commands::shared_args_mut() allows mutation on ReleaseNotes.
    ///
    /// ## Test Scenario
    /// - Creates a mutable Commands::ReleaseNotes
    /// - Mutates organization via shared_args_mut()
    ///
    /// ## Expected Outcome
    /// - Mutation is visible via shared_args()
    #[test]
    fn test_commands_shared_args_mut_release_notes() {
        let mut rn_cmd = Commands::ReleaseNotes(ReleaseNotesArgs {
            shared: SharedArgs::default(),
            output: ReleaseNotesOutputFormat::Markdown,
            copy: false,
            group: false,
            include_prs: false,
            from: None,
            to: None,
            no_cache: false,
        });

        rn_cmd.shared_args_mut().organization = Some("mutated-org".to_string());
        assert_eq!(
            rn_cmd.shared_args().organization,
            Some("mutated-org".to_string())
        );
    }

    /// # HasSharedArgs Trait on ReleaseNotesArgs
    ///
    /// Tests that the HasSharedArgs trait works correctly on ReleaseNotesArgs.
    ///
    /// ## Test Scenario
    /// - Creates ReleaseNotesArgs with shared arguments
    /// - Uses trait methods to access and mutate shared args
    ///
    /// ## Expected Outcome
    /// - Trait methods return correct shared arguments
    /// - Mutable access works correctly
    #[test]
    fn test_has_shared_args_trait_release_notes() {
        let mut rn_args = ReleaseNotesArgs {
            shared: SharedArgs {
                organization: Some("rn-org".to_string()),
                project: Some("rn-proj".to_string()),
                ..Default::default()
            },
            output: ReleaseNotesOutputFormat::Markdown,
            copy: false,
            group: false,
            include_prs: false,
            from: None,
            to: None,
            no_cache: false,
        };

        assert_eq!(
            rn_args.shared_args().organization,
            Some("rn-org".to_string())
        );
        assert_eq!(rn_args.shared_args().project, Some("rn-proj".to_string()));

        rn_args.shared_args_mut().organization = Some("modified-org".to_string());
        assert_eq!(
            rn_args.shared_args().organization,
            Some("modified-org".to_string())
        );
    }

    // ========================================================================
    // Release-notes config resolution tests
    // ========================================================================

    /// # Release Notes Resolve Config Success
    ///
    /// Tests that release-notes args resolve config successfully.
    ///
    /// ## Test Scenario
    /// - Uses create_sample_release_notes_args() with all required fields
    /// - Calls resolve_config()
    ///
    /// ## Expected Outcome
    /// - Config resolves successfully
    /// - Result is AppConfig::ReleaseNotes variant
    #[test]
    fn test_release_notes_resolve_config_success() {
        let args = create_sample_release_notes_args();
        let result = args.resolve_config();
        assert!(result.is_ok());

        let config = result.unwrap();
        assert!(matches!(config, AppConfig::ReleaseNotes { .. }));
        assert!(!config.is_migration_mode());
    }

    /// # Release Notes App Config Fields
    ///
    /// Tests that shared config fields have the correct Cli source annotation.
    ///
    /// ## Test Scenario
    /// - Resolves config from sample release-notes args
    /// - Checks shared config field sources
    ///
    /// ## Expected Outcome
    /// - Organization and project are marked as Cli source
    #[test]
    fn test_release_notes_app_config_fields() {
        let args = create_sample_release_notes_args();
        let config = args.resolve_config().unwrap();

        assert_eq!(
            config.shared().organization,
            ParsedProperty::Cli("test-org".to_string(), "test-org".to_string())
        );
        assert_eq!(
            config.shared().project,
            ParsedProperty::Cli("test-project".to_string(), "test-project".to_string())
        );
    }

    /// # Release Notes Mode Config Maps from Args
    ///
    /// Tests that ReleaseNotesModeConfig fields correctly map from CLI args.
    ///
    /// ## Test Scenario
    /// - Resolves config from sample release-notes args
    /// - Checks each ReleaseNotesModeConfig field
    ///
    /// ## Expected Outcome
    /// - All mode config fields match the original args
    #[test]
    fn test_release_notes_mode_config_maps_from_args() {
        let args = create_sample_release_notes_args();
        let config = args.resolve_config().unwrap();

        if let AppConfig::ReleaseNotes { release_notes, .. } = config {
            assert_eq!(release_notes.from_version, Some("v1.0.0".to_string()));
            assert_eq!(release_notes.to_version, Some("v2.0.0".to_string()));
            assert_eq!(
                release_notes.output_format,
                ReleaseNotesOutputFormat::Markdown
            );
            assert!(!release_notes.grouped);
            assert!(!release_notes.include_prs);
            assert!(!release_notes.copy_to_clipboard);
            assert!(!release_notes.no_cache);
        } else {
            panic!("Expected ReleaseNotes config");
        }
    }

    // ========================================================================
    // Release-notes error/edge case tests
    // ========================================================================

    /// # Release Notes Unknown Flag Rejected
    ///
    /// Tests that unrecognized flags on release-notes are rejected.
    ///
    /// ## Test Scenario
    /// - Attempts to parse release-notes with an unknown --foo flag
    ///
    /// ## Expected Outcome
    /// - Parsing fails
    #[test]
    fn test_release_notes_unknown_flag_rejected() {
        let result = Args::try_parse_from(["mergers", "release-notes", "--foo", "bar"]);
        assert!(result.is_err());
    }

    /// # Release Notes From/To with Spaces Preserved
    ///
    /// Tests that --from and --to values with spaces are preserved as-is.
    ///
    /// ## Test Scenario
    /// - Parses release-notes with space-containing version strings
    ///
    /// ## Expected Outcome
    /// - Values are preserved exactly including spaces
    #[test]
    fn test_release_notes_from_to_with_spaces_preserved() {
        let args = Args::parse_from([
            "mergers",
            "rn",
            "--from",
            "v1.0.0 beta",
            "--to",
            "v2.0.0 release candidate",
        ]);

        if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
            assert_eq!(rn_args.from, Some("v1.0.0 beta".to_string()));
            assert_eq!(rn_args.to, Some("v2.0.0 release candidate".to_string()));
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    /// # Release Notes Not Available via MergeArgsParser Fallback
    ///
    /// Tests that release-notes-specific flags fail on MergeArgsParser.
    ///
    /// ## Test Scenario
    /// - Attempts to parse release-notes flags via MergeArgsParser
    /// - MergeArgsParser only knows MergeArgs, not ReleaseNotesArgs
    ///
    /// ## Expected Outcome
    /// - Parsing fails because --from is not a valid MergeArgs flag
    #[test]
    fn test_release_notes_not_on_merge_args_parser_fallback() {
        let result =
            MergeArgsParser::try_parse_from(["mergers", "release-notes", "--from", "v1.0"]);
        assert!(result.is_err());
    }

    /// # Release Notes with Logging Args
    ///
    /// Tests that logging arguments work on the release-notes command.
    ///
    /// ## Test Scenario
    /// - Parses release-notes with --log-level, --log-file, --log-format
    ///
    /// ## Expected Outcome
    /// - All logging fields are populated on shared args
    #[test]
    fn test_release_notes_with_logging_args() {
        let args = Args::parse_from([
            "mergers",
            "release-notes",
            "--log-level",
            "debug",
            "--log-file",
            "/tmp/rn.log",
            "--log-format",
            "json",
        ]);

        if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
            assert_eq!(rn_args.shared.log_level, Some("debug".to_string()));
            assert_eq!(rn_args.shared.log_file, Some("/tmp/rn.log".to_string()));
            assert_eq!(rn_args.shared.log_format, Some("json".to_string()));
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    // ========================================================================
    // Cross-cutting clap behavior tests
    // ========================================================================

    /// # Equals Syntax for Flags
    ///
    /// Tests that `--flag=value` syntax is accepted alongside `--flag value`.
    ///
    /// ## Test Scenario
    /// - Parses flags using `=` separator instead of space
    /// - Tests multiple flags with equals syntax
    ///
    /// ## Expected Outcome
    /// - All values are correctly parsed
    #[test]
    fn test_equals_syntax_on_flags() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "--organization=my-org",
            "--project=my-proj",
            "--repository=my-repo",
            "--pat=my-token",
            "--dev-branch=develop",
            "--parallel-limit=200",
            "--tag-prefix=rel-",
            "--work-item-state=Done",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.organization, Some("my-org".to_string()));
            assert_eq!(merge_args.shared.project, Some("my-proj".to_string()));
            assert_eq!(merge_args.shared.repository, Some("my-repo".to_string()));
            assert_eq!(merge_args.shared.pat, Some("my-token".to_string()));
            assert_eq!(merge_args.shared.dev_branch, Some("develop".to_string()));
            assert_eq!(merge_args.shared.parallel_limit, Some(200));
            assert_eq!(merge_args.shared.tag_prefix, Some("rel-".to_string()));
            assert_eq!(merge_args.work_item_state, Some("Done".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Equals Syntax on Release Notes
    ///
    /// Tests that `--flag=value` syntax works on release-notes command.
    ///
    /// ## Test Scenario
    /// - Parses release-notes flags using equals syntax
    ///
    /// ## Expected Outcome
    /// - Values parsed correctly
    #[test]
    fn test_equals_syntax_on_release_notes() {
        let args = Args::parse_from(["mergers", "rn", "--from=v1.0", "--to=v2.0", "--output=json"]);

        if let Some(Commands::ReleaseNotes(rn_args)) = args.command {
            assert_eq!(rn_args.from, Some("v1.0".to_string()));
            assert_eq!(rn_args.to, Some("v2.0".to_string()));
            assert_eq!(rn_args.output, ReleaseNotesOutputFormat::Json);
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    /// # Equals Syntax on Merge Subcommands
    ///
    /// Tests that `--flag=value` works on merge subcommands.
    ///
    /// ## Test Scenario
    /// - Parses merge complete with equals syntax
    ///
    /// ## Expected Outcome
    /// - Values parsed correctly
    #[test]
    fn test_equals_syntax_on_merge_subcommands() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "complete",
            "--next-state=Done",
            "--repo=/path/to/repo",
            "--output=ndjson",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            if let Some(MergeSubcommand::Complete(complete_args)) = merge_args.subcommand {
                assert_eq!(complete_args.next_state, "Done");
                assert_eq!(complete_args.repo, Some("/path/to/repo".to_string()));
                assert_eq!(complete_args.output, OutputFormat::Ndjson);
            } else {
                panic!("Expected Complete subcommand");
            }
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Repeated Flags Are Rejected
    ///
    /// Tests that providing the same flag multiple times is rejected by clap.
    ///
    /// ## Test Scenario
    /// - Provides --organization twice with different values
    ///
    /// ## Expected Outcome
    /// - Parsing fails because clap 4 does not allow repeated flags by default
    #[test]
    fn test_repeated_flags_rejected() {
        let result =
            Args::try_parse_from(["mergers", "merge", "-o", "first-org", "-o", "second-org"]);
        assert!(result.is_err(), "Repeated flags should be rejected by clap");
    }

    /// # Double Dash Ends Options
    ///
    /// Tests that `--` stops flag parsing, treating subsequent args as positional.
    ///
    /// ## Test Scenario
    /// - Parses `mergers merge -- -o` where `-o` should be treated as positional path
    ///
    /// ## Expected Outcome
    /// - `-o` is captured as the positional path, not as the organization flag
    #[test]
    fn test_double_dash_ends_options() {
        let args = Args::parse_from(["mergers", "merge", "--", "-o"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            // After --, "-o" should be treated as a positional argument (path)
            assert_eq!(
                merge_args.shared.path,
                Some("-o".to_string()),
                "-o after -- should be treated as positional path, not flag"
            );
            assert_eq!(
                merge_args.shared.organization, None,
                "Organization should be None since -o was after --"
            );
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Combined Short Boolean Flags
    ///
    /// Tests that boolean short flags can be combined as `-nq` instead of `-n -q`.
    ///
    /// ## Test Scenario
    /// - Parses `mergers merge -nq` with combined flags
    ///
    /// ## Expected Outcome
    /// - Both non_interactive and quiet are true
    #[test]
    fn test_combined_short_boolean_flags() {
        let args = Args::parse_from([
            "mergers", "merge", "-nq", "-o", "org", "-p", "proj", "-r", "repo", "-t", "pat",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert!(
                merge_args.ni.non_interactive,
                "-nq should activate non_interactive"
            );
            assert!(merge_args.ni.quiet, "-nq should activate quiet");
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Unicode Values in Arguments
    ///
    /// Tests that unicode/UTF-8 values in arguments are preserved correctly.
    ///
    /// ## Test Scenario
    /// - Parses arguments with non-ASCII characters (Chinese, emoji, accented)
    ///
    /// ## Expected Outcome
    /// - All unicode values are preserved as-is
    #[test]
    fn test_unicode_values_in_arguments() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "-o",
            "组织名",
            "-p",
            "项目",
            "--work-item-state",
            "Résolu",
            "--tag-prefix",
            "版本-",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.organization, Some("组织名".to_string()));
            assert_eq!(merge_args.shared.project, Some("项目".to_string()));
            assert_eq!(merge_args.work_item_state, Some("Résolu".to_string()));
            assert_eq!(merge_args.shared.tag_prefix, Some("版本-".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Long Non-Interactive Flag
    ///
    /// Tests that --non-interactive works as the long form of -n.
    ///
    /// ## Test Scenario
    /// - Parses merge with --non-interactive (long form)
    ///
    /// ## Expected Outcome
    /// - non_interactive is true, same as with -n
    #[test]
    fn test_long_non_interactive_flag() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "--non-interactive",
            "--version",
            "v1.0",
            "-o",
            "org",
            "-p",
            "proj",
            "-r",
            "repo",
            "-t",
            "pat",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert!(merge_args.ni.non_interactive);
            assert_eq!(merge_args.ni.version, Some("v1.0".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Local Repo Flag on Non-Merge Commands
    ///
    /// Tests that --local-repo works on migrate, cleanup, and release-notes.
    ///
    /// ## Test Scenario
    /// - Parses each non-merge command with --local-repo
    ///
    /// ## Expected Outcome
    /// - local_repo is captured in shared args for all commands
    #[test]
    fn test_local_repo_on_all_commands() {
        // Migrate
        let args = Args::parse_from(["mergers", "migrate", "--local-repo", "/path/to/repo"]);
        if let Some(Commands::Migrate(m)) = args.command {
            assert_eq!(m.shared.local_repo, Some("/path/to/repo".to_string()));
        } else {
            panic!("Expected Migrate command");
        }

        // Cleanup
        let args = Args::parse_from(["mergers", "cleanup", "--local-repo", "/path/to/repo"]);
        if let Some(Commands::Cleanup(c)) = args.command {
            assert_eq!(c.shared.local_repo, Some("/path/to/repo".to_string()));
        } else {
            panic!("Expected Cleanup command");
        }

        // Release-notes
        let args = Args::parse_from(["mergers", "release-notes", "--local-repo", "/path/to/repo"]);
        if let Some(Commands::ReleaseNotes(rn)) = args.command {
            assert_eq!(rn.shared.local_repo, Some("/path/to/repo".to_string()));
        } else {
            panic!("Expected ReleaseNotes command");
        }
    }

    /// # Cleanup Alias with Target Combined
    ///
    /// Tests that the 'c' alias works together with --target and shared args.
    ///
    /// ## Test Scenario
    /// - Parses `mergers c --target main -o org`
    ///
    /// ## Expected Outcome
    /// - Command is Cleanup, target and shared args both populated
    #[test]
    fn test_cleanup_alias_with_target_and_shared_args() {
        let args = Args::parse_from([
            "mergers",
            "c",
            "--target",
            "main",
            "-o",
            "org",
            "-p",
            "proj",
            "-r",
            "repo",
            "-t",
            "pat",
            "/path/to/repo",
        ]);

        if let Some(Commands::Cleanup(cleanup_args)) = args.command {
            assert_eq!(cleanup_args.target, Some("main".to_string()));
            assert_eq!(cleanup_args.shared.organization, Some("org".to_string()));
            assert_eq!(cleanup_args.shared.path, Some("/path/to/repo".to_string()));
        } else {
            panic!("Expected Cleanup command");
        }
    }

    /// # Equals Syntax Mixed with Space Syntax
    ///
    /// Tests that equals and space flag syntax can be mixed freely.
    ///
    /// ## Test Scenario
    /// - Uses --org=val for some flags and --proj val for others
    ///
    /// ## Expected Outcome
    /// - All values parsed correctly regardless of syntax
    #[test]
    fn test_equals_and_space_syntax_mixed() {
        let args = Args::parse_from([
            "mergers",
            "merge",
            "--organization=my-org",
            "--project",
            "my-proj",
            "--repository=my-repo",
            "--pat",
            "my-token",
        ]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.organization, Some("my-org".to_string()));
            assert_eq!(merge_args.shared.project, Some("my-proj".to_string()));
            assert_eq!(merge_args.shared.repository, Some("my-repo".to_string()));
            assert_eq!(merge_args.shared.pat, Some("my-token".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Equals Syntax with Empty Value
    ///
    /// Tests that `--flag=` (equals with empty string) is handled.
    ///
    /// ## Test Scenario
    /// - Parses --organization= (empty value after equals)
    ///
    /// ## Expected Outcome
    /// - Value is Some("") — an empty string
    #[test]
    fn test_equals_syntax_with_empty_value() {
        let args = Args::parse_from(["mergers", "merge", "--organization="]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.organization, Some("".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Repeated Boolean Flags Are Rejected
    ///
    /// Tests that specifying a boolean flag multiple times is rejected.
    ///
    /// ## Test Scenario
    /// - Provides --skip-confirmation twice
    ///
    /// ## Expected Outcome
    /// - Parsing fails because clap 4 does not allow repeated flags by default
    #[test]
    fn test_repeated_boolean_flags_rejected() {
        let result = Args::try_parse_from([
            "mergers",
            "merge",
            "--skip-confirmation",
            "--skip-confirmation",
        ]);
        assert!(
            result.is_err(),
            "Repeated boolean flags should be rejected by clap"
        );
    }

    /// # Numeric Argument with Equals Syntax
    ///
    /// Tests that numeric arguments work with equals syntax.
    ///
    /// ## Test Scenario
    /// - Parses --parallel-limit=999
    ///
    /// ## Expected Outcome
    /// - Value is parsed as 999
    #[test]
    fn test_numeric_argument_equals_syntax() {
        let args = Args::parse_from(["mergers", "merge", "--parallel-limit=999"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.shared.parallel_limit, Some(999));
        } else {
            panic!("Expected Merge command");
        }
    }

    /// # Value Enum with Equals Syntax
    ///
    /// Tests that value_enum arguments work with equals syntax.
    ///
    /// ## Test Scenario
    /// - Parses --output=json and --log-format=text using equals
    ///
    /// ## Expected Outcome
    /// - Enums are correctly parsed
    #[test]
    fn test_value_enum_equals_syntax() {
        let args = Args::parse_from(["mergers", "merge", "--output=json", "--log-format=text"]);

        if let Some(Commands::Merge(merge_args)) = args.command {
            assert_eq!(merge_args.ni.output, OutputFormat::Json);
            assert_eq!(merge_args.shared.log_format, Some("text".to_string()));
        } else {
            panic!("Expected Merge command");
        }
    }
}
