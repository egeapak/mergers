use crate::{config::Config, parsed_property::ParsedProperty, utils::parse_since_date};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::{Args as ClapArgs, Parser, Subcommand};
use serde::Deserialize;

/// Shared arguments used by all commands
#[derive(ClapArgs, Clone, Default, Debug)]
pub struct SharedArgs {
    /// Local repository path (positional argument, takes precedence over --local-repo)
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
}

/// Arguments specific to merge mode
#[derive(ClapArgs, Clone)]
pub struct MergeArgs {
    #[command(flatten)]
    pub shared: SharedArgs,

    /// State to set work items to after successful merge [default: Next Merged]
    #[arg(long, help_heading = "Merge Options")]
    pub work_item_state: Option<String>,

    /// Run git hooks during cherry-pick operations (hooks are skipped by default)
    #[arg(long, help_heading = "Merge Options")]
    pub run_hooks: bool,
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

/// Arguments for the `merge run` subcommand.
#[derive(ClapArgs, Clone, Debug)]
pub struct MergeRunArgs {
    #[command(flatten)]
    pub shared: SharedArgs,

    /// State to set work items to after successful merge [default: Next Merged]
    #[arg(long, help_heading = "Merge Options")]
    pub work_item_state: Option<String>,

    /// Run git hooks during cherry-pick operations (hooks are skipped by default)
    #[arg(long, help_heading = "Merge Options")]
    pub run_hooks: bool,

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

impl From<MergeRunArgs> for MergeArgs {
    fn from(args: MergeRunArgs) -> Self {
        MergeArgs {
            shared: args.shared,
            work_item_state: args.work_item_state,
            run_hooks: args.run_hooks,
        }
    }
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
    /// Start a new merge operation
    #[command(
        about = "Start a new merge operation",
        long_about = "Start a new merge operation, either interactively (TUI) or non-interactively.\n\n\
            In non-interactive mode (-n), PRs are filtered by work item state and cherry-picked\n\
            automatically. On conflict, the process exits with code 2 for resolution."
    )]
    Run(Box<MergeRunArgs>),

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
        after_help = "EXAMPLES:\n    \
            # Basic merge with all required args\n    \
            mergers merge -o myorg -p myproject -r myrepo -t <PAT> /path/to/repo\n\n    \
            # Merge with custom branches and work item state\n    \
            mergers merge -o myorg -p myproject -r myrepo -t <PAT> \\\n      \
            --dev-branch develop --target-branch release \\\n      \
            --work-item-state \"Ready for Test\" /path/to/repo\n\n    \
            # Merge PRs from the last 2 weeks only\n    \
            mergers m -o myorg -p proj -r repo -t <PAT> --since 2w"
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
        after_help = "EXAMPLES:\n    \
            # Analyze migrations with default terminal states\n    \
            mergers migrate -o myorg -p myproject -r myrepo -t <PAT>\n\n    \
            # Custom terminal states for work items\n    \
            mergers migrate -o myorg -p myproject -r myrepo -t <PAT> \\\n      \
            --terminal-states \"Closed,Done,Resolved\"\n\n    \
            # Analyze only recent PRs\n    \
            mergers mi -o myorg -p proj -r repo -t <PAT> --since 1mo"
    )]
    Migrate(MigrateArgs),

    /// Clean up merged patch branches from the repository
    #[command(
        visible_alias = "c",
        long_about = "Clean up patch branches that have been merged to the target branch.\n\n\
            This mode identifies local branches matching the tag prefix pattern (default: merged-*)\n\
            that have been fully merged into the target branch, and offers to delete them.\n\
            Useful for maintaining a clean repository after completing merge operations.",
        after_help = "EXAMPLES:\n    \
            # Cleanup branches merged to default target\n    \
            mergers cleanup -o myorg -p myproject -r myrepo -t <PAT> /path/to/repo\n\n    \
            # Cleanup branches merged to a specific target\n    \
            mergers cleanup -o myorg -p proj -r repo -t <PAT> --target main\n\n    \
            # Cleanup with custom tag prefix\n    \
            mergers c -o myorg -p proj -r repo -t <PAT> --tag-prefix patch-"
    )]
    Cleanup(CleanupArgs),
}

impl Commands {
    /// Extract shared arguments from any command variant
    pub fn shared_args(&self) -> &SharedArgs {
        match self {
            Commands::Merge(args) => args.shared_args(),
            Commands::Migrate(args) => args.shared_args(),
            Commands::Cleanup(args) => args.shared_args(),
        }
    }

    /// Extract mutable shared arguments from any command variant
    pub fn shared_args_mut(&mut self) -> &mut SharedArgs {
        match self {
            Commands::Merge(args) => args.shared_args_mut(),
            Commands::Migrate(args) => args.shared_args_mut(),
            Commands::Cleanup(args) => args.shared_args_mut(),
        }
    }
}

#[derive(Parser, Clone)]
#[command(
    author,
    version,
    about = "Manage Azure DevOps pull request merging and migration workflows",
    long_about = "A CLI/TUI tool for managing Azure DevOps pull request merging and migration workflows.\n\n\
        Mergers helps you:\n  \
        • Cherry-pick merged PRs from dev to target branches\n  \
        • Analyze PRs for migration eligibility\n  \
        • Clean up merged patch branches\n\n\
        Configuration can be provided via CLI arguments, environment variables (MERGERS_*),\n\
        config file (~/.config/mergers/config.toml), or auto-detected from git remotes.",
    after_help = "EXAMPLES:\n    \
        # Merge mode with Azure DevOps credentials\n    \
        mergers merge -o myorg -p myproject -r myrepo -t <PAT> /path/to/repo\n\n    \
        # Migration analysis mode\n    \
        mergers migrate -o myorg -p myproject -r myrepo -t <PAT> --since 1mo\n\n    \
        # Cleanup merged branches\n    \
        mergers cleanup -o myorg -p myproject -r myrepo -t <PAT>\n\n    \
        # Create sample config file\n    \
        mergers --create-config\n\n\
        For more information, see: https://github.com/egeapak/mergers"
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
#[command(name = "mergers", about = None, long_about = None)]
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
}

impl AppConfig {
    pub fn shared(&self) -> &SharedConfig {
        match self {
            AppConfig::Default { shared, .. } => shared,
            AppConfig::Migration { shared, .. } => shared,
            AppConfig::Cleanup { shared, .. } => shared,
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
            AppConfig::Default { shared, .. } => shared,
            AppConfig::Migration { shared, .. } => shared,
            AppConfig::Cleanup { shared, .. } => shared,
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
                work_item_state: None,
                run_hooks: false,
            })
        });

        // Access shared args through the command using the trait
        let shared = mode_command.shared_args();

        // Determine local_repo path (positional arg takes precedence over --local-repo flag)
        let local_repo_path = shared.path.as_ref().or(shared.local_repo.as_ref());

        // Load from config file (lowest priority)
        let file_config = Config::load_from_file()?;

        // Load from environment variables
        let env_config = Config::load_from_env();

        // Try to detect from git remote if we have a local repo path
        let git_config = if let Some(repo_path) = local_repo_path {
            Config::detect_from_git_remote(repo_path)
        } else {
            Config::default()
        };

        let cli_config = Config {
            organization: shared
                .organization
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            project: shared
                .project
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            repository: shared
                .repository
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            pat: shared
                .pat
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            dev_branch: shared
                .dev_branch
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            target_branch: shared
                .target_branch
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            local_repo: local_repo_path.map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            work_item_state: None, // Will be set based on command
            parallel_limit: shared
                .parallel_limit
                .map(|v| ParsedProperty::Cli(v, v.to_string())),
            max_concurrent_network: shared
                .max_concurrent_network
                .map(|v| ParsedProperty::Cli(v, v.to_string())),
            max_concurrent_processing: shared
                .max_concurrent_processing
                .map(|v| ParsedProperty::Cli(v, v.to_string())),
            tag_prefix: shared
                .tag_prefix
                .as_ref()
                .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
            // run_hooks is handled separately per command (MergeArgs has it, not SharedArgs)
            run_hooks: None,
        };

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
            local_repo: local_repo_path.map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
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
                },
                work_item_state: Some("Done".to_string()),
                run_hooks: false,
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
                },
                terminal_states: "Closed,Done".to_string(),
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
            work_item_state: None,
            run_hooks: false,
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
            work_item_state: None,
            run_hooks: false,
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
}
