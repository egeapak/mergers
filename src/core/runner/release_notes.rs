//! Release notes runner for CLI usage.
//!
//! Generates release notes from Azure DevOps PR labels and work items.

use anyhow::{Context, Result};

use crate::api::{AzureDevOpsClient, extract_merged_tags, filter_prs_with_tag};
use crate::models::ReleaseNotesOutputFormat;
use crate::release_notes;
use crate::release_notes::cache::WorkItemCache;

/// Configuration for the release notes runner.
pub struct ReleaseNotesRunnerConfig {
    pub organization: String,
    pub project: String,
    pub repository: String,
    pub pat: String,
    pub dev_branch: String,
    pub tag_prefix: String,
    pub from_version: Option<String>,
    pub to_version: Option<String>,
    pub output_format: ReleaseNotesOutputFormat,
    pub grouped: bool,
    pub include_prs: bool,
    pub copy_to_clipboard: bool,
    pub no_cache: bool,
    pub max_concurrent_network: usize,
    pub max_concurrent_processing: usize,
}

/// Release notes runner.
pub struct ReleaseNotesRunner {
    config: ReleaseNotesRunnerConfig,
}

impl ReleaseNotesRunner {
    pub fn new(config: ReleaseNotesRunnerConfig) -> Self {
        Self { config }
    }

    pub async fn run(&self) -> Result<String> {
        let client = AzureDevOpsClient::new(
            self.config.organization.clone(),
            self.config.project.clone(),
            self.config.repository.clone(),
            self.config.pat.clone(),
        )?;

        eprintln!("Fetching pull requests from Azure DevOps...");
        let all_prs = client
            .fetch_pull_requests(&self.config.dev_branch, None)
            .await?;

        let all_tags = extract_merged_tags(&all_prs, &self.config.tag_prefix);

        if all_tags.is_empty() {
            anyhow::bail!(
                "No PRs found with '{}' tag prefix. Tag PRs first using the merge workflow.",
                self.config.tag_prefix
            );
        }

        let (target_tag, _version_label) = self.resolve_target_tag(&all_tags)?;

        let tagged_prs = filter_prs_with_tag(&all_prs, &target_tag);

        if tagged_prs.is_empty() {
            anyhow::bail!("No PRs found with tag '{}'", target_tag);
        }

        eprintln!("Found {} PR(s) with tag '{}'", tagged_prs.len(), target_tag);

        let owned_prs: Vec<_> = tagged_prs.into_iter().cloned().collect();
        let prs_with_wi = client
            .fetch_work_items_for_prs_parallel(
                &owned_prs,
                self.config.max_concurrent_network,
                self.config.max_concurrent_processing,
            )
            .await;

        if !self.config.no_cache {
            self.update_cache(&prs_with_wi);
        }

        let entries = release_notes::build_entries_from_prs(
            &prs_with_wi,
            &self.config.organization,
            &self.config.project,
        );

        let formatted =
            release_notes::format_output(&entries, self.config.output_format, self.config.grouped)?;

        if self.config.copy_to_clipboard {
            release_notes::copy_to_clipboard(&formatted)?;
            eprintln!("Output copied to clipboard.");
        }

        Ok(formatted)
    }

    fn resolve_target_tag(&self, all_tags: &[String]) -> Result<(String, String)> {
        let prefix = &self.config.tag_prefix;

        match (&self.config.from_version, &self.config.to_version) {
            (Some(_from), Some(to)) => {
                let to_tag = Self::normalize_tag(prefix, to);

                if !all_tags.contains(&to_tag) {
                    anyhow::bail!("Tag '{}' not found in PRs", to_tag);
                }

                let version = to.strip_prefix(prefix).unwrap_or(to);
                Ok((to_tag, version.to_string()))
            }
            (Some(from), None) => {
                let from_tag = Self::normalize_tag(prefix, from);
                let from_idx = all_tags
                    .iter()
                    .position(|t| *t == from_tag)
                    .with_context(|| format!("Tag '{}' not found in PRs", from_tag))?;

                if from_idx + 1 < all_tags.len() {
                    let tag = &all_tags[from_idx + 1];
                    let version = tag.strip_prefix(prefix).unwrap_or(tag);
                    Ok((tag.clone(), version.to_string()))
                } else {
                    let version = from.strip_prefix(prefix).unwrap_or(from);
                    Ok((from_tag, version.to_string()))
                }
            }
            (None, Some(to)) => {
                let to_tag = Self::normalize_tag(prefix, to);
                if !all_tags.contains(&to_tag) {
                    anyhow::bail!("Tag '{}' not found in PRs", to_tag);
                }
                let version = to.strip_prefix(prefix).unwrap_or(to);
                Ok((to_tag, version.to_string()))
            }
            (None, None) => {
                let tag = all_tags.last().unwrap();
                let version = tag.strip_prefix(prefix).unwrap_or(tag);
                Ok((tag.clone(), version.to_string()))
            }
        }
    }

    fn normalize_tag(prefix: &str, input: &str) -> String {
        if input.starts_with(prefix) {
            input.to_string()
        } else {
            format!("{}{}", prefix, input)
        }
    }

    fn update_cache(&self, prs_with_wi: &[crate::models::PullRequestWithWorkItems]) {
        let mut cache = WorkItemCache::load().unwrap_or_default();
        for pr_with_wi in prs_with_wi {
            for wi in &pr_with_wi.work_items {
                if let Some(ref title) = wi.fields.title {
                    cache.set(
                        wi.id,
                        title,
                        wi.fields.state.as_deref(),
                        wi.fields.work_item_type.as_deref(),
                    );
                }
            }
        }
        if let Err(e) = cache.save() {
            eprintln!("Warning: Failed to save cache: {}", e);
        }
    }
}
