use crate::config::Config;
use anyhow::Result;
use clap::Parser;
use serde::Deserialize;

#[derive(Parser, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Local repository path (optional positional argument)
    pub path: Option<String>,

    /// Azure DevOps organization
    #[arg(short, long)]
    pub organization: Option<String>,

    /// Azure DevOps project
    #[arg(short, long)]
    pub project: Option<String>,

    /// Repository name
    #[arg(short, long)]
    pub repository: Option<String>,

    /// Personal Access Token
    #[arg(short = 't', long)]
    pub pat: Option<String>,

    /// Development branch name
    #[arg(long)]
    pub dev_branch: Option<String>,

    /// Target branch name
    #[arg(long)]
    pub target_branch: Option<String>,

    /// Local repository path (if provided, uses git worktree instead of cloning)
    #[arg(long)]
    pub local_repo: Option<String>,

    /// Target state for work items after successful merge (default mode only)
    #[arg(long)]
    pub work_item_state: Option<String>,

    /// Migration mode - analyze PRs for migration eligibility
    #[arg(long)]
    pub migrate: bool,

    /// Terminal work item states (comma-separated, migration mode only)
    #[arg(long, default_value = "Closed,Next Closed,Next Merged")]
    pub terminal_states: String,

    /// Tag prefix for PR tagging (both default and migration modes)
    #[arg(long, default_value = "merged-")]
    pub tag_prefix: Option<String>,

    /// Maximum number of parallel operations for API calls
    #[arg(long)]
    pub parallel_limit: Option<usize>,

    /// Maximum number of concurrent network operations
    #[arg(long)]
    pub max_concurrent_network: Option<usize>,

    /// Maximum number of concurrent processing operations
    #[arg(long)]
    pub max_concurrent_processing: Option<usize>,

    /// Create a sample configuration file
    #[arg(long)]
    pub create_config: bool,

    /// Limit fetching to items created after this date (e.g., "1mo", "2w", "2025-07-01")
    #[arg(long)]
    pub since: Option<String>,

    /// Skip the settings confirmation page and proceed directly
    #[arg(long)]
    pub skip_confirmation: bool,
}

/// Shared configuration used by both modes
#[derive(Debug, Clone)]
pub struct SharedConfig {
    pub organization: String,
    pub project: String,
    pub repository: String,
    pub pat: String,
    pub dev_branch: String,
    pub target_branch: String,
    pub local_repo: Option<String>,
    pub parallel_limit: usize,
    pub max_concurrent_network: usize,
    pub max_concurrent_processing: usize,
    pub tag_prefix: String,
    pub since: Option<String>,
    pub skip_confirmation: bool,
}

/// Configuration specific to default mode
#[derive(Debug, Clone)]
pub struct DefaultModeConfig {
    pub work_item_state: String,
}

/// Configuration specific to migration mode
#[derive(Debug, Clone)]
pub struct MigrationModeConfig {
    pub terminal_states: String,
}

/// Resolved configuration with mode-specific settings
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
}

impl AppConfig {
    pub fn shared(&self) -> &SharedConfig {
        match self {
            AppConfig::Default { shared, .. } => shared,
            AppConfig::Migration { shared, .. } => shared,
        }
    }

    pub fn is_migration_mode(&self) -> bool {
        matches!(self, AppConfig::Migration { .. })
    }
}

impl Args {
    /// Resolve configuration from CLI args, environment variables, config file, and git remote
    /// Priority: CLI args > environment variables > git remote > config file > defaults
    pub fn resolve_config(self) -> Result<AppConfig> {
        // Determine local_repo path (positional arg takes precedence over --local-repo flag)
        let local_repo_path = self.path.clone().or(self.local_repo.clone());

        // Load from config file (lowest priority)
        let file_config = Config::load_from_file()?;

        // Load from environment variables
        let env_config = Config::load_from_env();

        // Try to detect from git remote if we have a local repo path
        let git_config = if let Some(ref repo_path) = local_repo_path {
            Config::detect_from_git_remote(repo_path)
        } else {
            Config::default()
        };

        // Convert CLI args to config format (highest priority)
        let cli_config = Config {
            organization: self.organization.clone(),
            project: self.project.clone(),
            repository: self.repository.clone(),
            pat: self.pat.clone(),
            dev_branch: self.dev_branch.clone(),
            target_branch: self.target_branch.clone(),
            local_repo: local_repo_path.clone(),
            work_item_state: self.work_item_state.clone(),
            parallel_limit: self.parallel_limit,
            max_concurrent_network: self.max_concurrent_network,
            max_concurrent_processing: self.max_concurrent_processing,
            tag_prefix: self.tag_prefix.clone(),
        };

        // Merge configs: file < git_remote < env < cli
        let merged_config = file_config.merge(git_config).merge(env_config).merge(cli_config);

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

        let shared = SharedConfig {
            organization,
            project,
            repository,
            pat,
            dev_branch: merged_config
                .dev_branch
                .unwrap_or_else(|| "dev".to_string()),
            target_branch: merged_config
                .target_branch
                .unwrap_or_else(|| "next".to_string()),
            local_repo: local_repo_path,
            parallel_limit: merged_config.parallel_limit.unwrap_or(300),
            max_concurrent_network: merged_config.max_concurrent_network.unwrap_or(100),
            max_concurrent_processing: merged_config.max_concurrent_processing.unwrap_or(10),
            tag_prefix: merged_config
                .tag_prefix
                .unwrap_or_else(|| "merged-".to_string()),
            since: self.since.clone(),
            skip_confirmation: self.skip_confirmation,
        };

        // Return appropriate configuration based on mode
        if self.migrate {
            Ok(AppConfig::Migration {
                shared,
                migration: MigrationModeConfig {
                    terminal_states: self.terminal_states,
                },
            })
        } else {
            Ok(AppConfig::Default {
                shared,
                default: DefaultModeConfig {
                    work_item_state: merged_config
                        .work_item_state
                        .unwrap_or_else(|| "Next Merged".to_string()),
                },
            })
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
    pub symmetric_diff: SymmetricDiffResult,
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
pub struct SymmetricDiffResult {
    pub commits_in_dev_not_target: Vec<String>,
    pub commits_in_target_not_dev: Vec<String>,
    pub common_commits: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PRAnalysisResult {
    pub pr: PullRequestWithWorkItems,
    pub all_work_items_terminal: bool,
    pub terminal_work_items: Vec<WorkItem>,
    pub non_terminal_work_items: Vec<WorkItem>,
    pub commit_in_target: bool,
    pub commit_title_in_target: bool,
    pub commit_id: String,
    pub unsure_reason: Option<String>,
    pub reason: Option<String>,
}
