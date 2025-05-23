use clap::Parser;
use serde::Deserialize;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Azure DevOps organization
    #[arg(short, long)]
    pub organization: String,

    /// Azure DevOps project
    #[arg(short, long)]
    pub project: String,

    /// Repository name
    #[arg(short, long)]
    pub repository: String,

    /// Personal Access Token
    #[arg(short = 't', long)]
    pub pat: String,

    /// Development branch name
    #[arg(long, default_value = "dev")]
    pub dev_branch: String,

    /// Target branch name
    #[arg(long, default_value = "next")]
    pub target_branch: String,

    /// Local repository path (if provided, uses git worktree instead of cloning)
    #[arg(long)]
    pub local_repo: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRequest {
    #[serde(rename = "pullRequestId")]
    pub id: i32,
    pub title: String,
    #[serde(rename = "creationDate")]
    pub creation_date: String,
    #[serde(rename = "closedDate")]
    pub closed_date: String,
    #[serde(rename = "createdBy")]
    pub created_by: CreatedBy,
    #[serde(rename = "lastMergeCommit")]
    pub last_merge_commit: Option<MergeCommit>,
    pub labels: Option<Vec<Label>>,
    #[serde(rename = "targetRefName")]
    pub target_ref_name: Option<String>,
    #[serde(rename = "sourceRefName")]
    pub source_ref_name: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatedBy {
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "uniqueName")]
    pub unique_name: Option<String>,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemFields {
    #[serde(rename = "System.Title")]
    pub title: Option<String>,
    #[serde(rename = "System.State")]
    pub state: Option<String>,
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
    Skipped,
}

#[derive(Debug, Clone)]
pub struct CherryPickItem {
    pub commit_id: String,
    pub pr_id: i32,
    pub pr_title: String,
    pub status: CherryPickStatus,
}
