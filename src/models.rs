use clap::Parser;
use serde::Deserialize;
use crate::config::Config;
use anyhow::Result;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Args {
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

    /// Target state for work items after successful merge
    #[arg(long)]
    pub work_item_state: Option<String>,

    /// Create a sample configuration file
    #[arg(long)]
    pub create_config: bool,
}

/// Resolved configuration with all required fields
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub organization: String,
    pub project: String,
    pub repository: String,
    pub pat: String,
    pub dev_branch: String,
    pub target_branch: String,
    pub local_repo: Option<String>,
    pub work_item_state: String,
}

impl Args {
    /// Resolve configuration from CLI args, environment variables, and config file
    /// Priority: CLI args > environment variables > config file > defaults
    pub fn resolve_config(self) -> Result<ResolvedConfig> {
        // Load from config file (lowest priority)
        let file_config = Config::load_from_file()?;
        
        // Load from environment variables (medium priority)
        let env_config = Config::load_from_env();
        
        // Convert CLI args to config format (highest priority)
        let cli_config = Config {
            organization: self.organization,
            project: self.project,
            repository: self.repository,
            pat: self.pat,
            dev_branch: self.dev_branch,
            target_branch: self.target_branch,
            local_repo: self.local_repo,
            work_item_state: self.work_item_state,
        };
        
        // Merge configs: file < env < cli
        let merged_config = file_config.merge(env_config).merge(cli_config);
        
        // Validate required fields
        let organization = merged_config.organization
            .ok_or_else(|| anyhow::anyhow!("organization is required (use --organization, MERGERS_ORGANIZATION env var, or config file)"))?;
        let project = merged_config.project
            .ok_or_else(|| anyhow::anyhow!("project is required (use --project, MERGERS_PROJECT env var, or config file)"))?;
        let repository = merged_config.repository
            .ok_or_else(|| anyhow::anyhow!("repository is required (use --repository, MERGERS_REPOSITORY env var, or config file)"))?;
        let pat = merged_config.pat
            .ok_or_else(|| anyhow::anyhow!("pat is required (use --pat, MERGERS_PAT env var, or config file)"))?;
        
        Ok(ResolvedConfig {
            organization,
            project,
            repository,
            pat,
            dev_branch: merged_config.dev_branch.unwrap_or_else(|| "dev".to_string()),
            target_branch: merged_config.target_branch.unwrap_or_else(|| "next".to_string()),
            local_repo: merged_config.local_repo,
            work_item_state: merged_config.work_item_state.unwrap_or_else(|| "Next Merged".to_string()),
        })
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
    #[serde(rename = "System.CreatedDate", default)]
    pub created_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemHistory {
    pub id: i32,
    #[serde(rename = "workItemId")]
    pub work_item_id: i32,
    pub rev: i32,
    #[serde(rename = "revisedDate")]
    pub revised_date: String,
    #[serde(rename = "revisedBy")]
    pub revised_by: Option<CreatedBy>,
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
    #[serde(rename = "oldValue")]
    pub old_value: Option<String>,
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
