//! Migration mode application state.
//!
//! This module provides [`MigrationApp`] which contains state specific to
//! the migration analysis mode.

use crate::{
    api::AzureDevOpsClient,
    models::{MigrationAnalysis, MigrationConfig},
    ui::{AppBase, AppMode, browser::BrowserOpener},
};
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

/// Application state for migration mode.
///
/// `MigrationApp` handles the workflow of analyzing which PRs need to be
/// migrated to a target branch. It categorizes PRs based on their commit
/// presence and work item states.
///
/// # Type Safety
///
/// `MigrationApp` uses `MigrationConfig` as its configuration type, providing
/// compile-time type safety. The terminal_states is accessed directly
/// without pattern matching.
///
/// # Field Access
///
/// Mode-specific fields are accessed directly on `MigrationApp`:
/// ```ignore
/// if let Some(analysis) = &app.migration_analysis {
///     // use analysis...
/// }
/// let states = app.terminal_states(); // Direct access, no pattern matching
/// ```
///
/// Shared fields are accessed via `Deref` to [`AppBase`]:
/// ```ignore
/// let org = app.organization();
/// let prs = &app.pull_requests;
/// ```
pub struct MigrationApp {
    /// Shared application state with MigrationConfig.
    base: AppBase<MigrationConfig>,

    /// Migration analysis results, if analysis has been performed.
    pub migration_analysis: Option<MigrationAnalysis>,
}

impl MigrationApp {
    /// Creates a new MigrationApp with the given configuration, client, and browser opener.
    pub fn new(
        config: Arc<MigrationConfig>,
        client: AzureDevOpsClient,
        browser: Box<dyn BrowserOpener>,
    ) -> Self {
        Self {
            base: AppBase::new(config, client, browser),
            migration_analysis: None,
        }
    }

    /// Returns the terminal states for work items (states that indicate completion).
    ///
    /// This provides direct, type-safe access to the migration-specific
    /// terminal_states configuration without runtime pattern matching.
    pub fn terminal_states(&self) -> &[String] {
        self.config().terminal_states.value()
    }

    /// Mark a PR as manually eligible - moves it to eligible regardless of automatic analysis.
    pub fn mark_pr_as_eligible(&mut self, pr_id: i32) {
        if let Some(analysis) = &mut self.migration_analysis {
            // Remove from not eligible set if present
            analysis
                .manual_overrides
                .marked_as_not_eligible
                .remove(&pr_id);
            // Add to eligible set
            analysis.manual_overrides.marked_as_eligible.insert(pr_id);
            // Recategorize with new overrides
            self.recategorize_prs();
        }
    }

    /// Mark a PR as manually not eligible - moves it to not merged regardless of automatic analysis.
    pub fn mark_pr_as_not_eligible(&mut self, pr_id: i32) {
        if let Some(analysis) = &mut self.migration_analysis {
            // Remove from eligible set if present
            analysis.manual_overrides.marked_as_eligible.remove(&pr_id);
            // Add to not eligible set
            analysis
                .manual_overrides
                .marked_as_not_eligible
                .insert(pr_id);
            // Recategorize with new overrides
            self.recategorize_prs();
        }
    }

    /// Remove manual override for a PR - returns it to automatic categorization.
    pub fn remove_manual_override(&mut self, pr_id: i32) {
        if let Some(analysis) = &mut self.migration_analysis {
            analysis.manual_overrides.marked_as_eligible.remove(&pr_id);
            analysis
                .manual_overrides
                .marked_as_not_eligible
                .remove(&pr_id);
            // Recategorize with updated overrides
            self.recategorize_prs();
        }
    }

    /// Check if a PR has a manual override.
    /// Returns Some(true) if marked eligible, Some(false) if marked not eligible, None if no override.
    pub fn has_manual_override(&self, pr_id: i32) -> Option<bool> {
        if let Some(analysis) = &self.migration_analysis {
            if analysis
                .manual_overrides
                .marked_as_eligible
                .contains(&pr_id)
            {
                Some(true) // manually marked eligible
            } else if analysis
                .manual_overrides
                .marked_as_not_eligible
                .contains(&pr_id)
            {
                Some(false) // manually marked not eligible
            } else {
                None // no manual override
            }
        } else {
            None
        }
    }

    /// Recategorize all PRs with current manual overrides.
    fn recategorize_prs(&mut self) {
        if let Some(analysis) = self.migration_analysis.take() {
            // Create a new analyzer instance with the same parameters
            let analyzer = crate::migration::MigrationAnalyzer::new(
                self.client.clone(),
                analysis.terminal_states.clone(),
            );

            // Recategorize with current overrides
            if let Ok(new_analysis) = analyzer.categorize_prs_with_overrides(
                analysis.all_details.clone(),
                analysis.manual_overrides.clone(),
            ) {
                self.migration_analysis = Some(new_analysis);
            } else {
                // If recategorization fails, restore the original analysis
                self.migration_analysis = Some(analysis);
            }
        }
    }

    /// Returns the number of eligible PRs in the migration analysis.
    pub fn eligible_count(&self) -> usize {
        self.migration_analysis
            .as_ref()
            .map(|a| a.eligible_prs.len())
            .unwrap_or(0)
    }

    /// Returns the number of not eligible PRs in the migration analysis.
    pub fn not_eligible_count(&self) -> usize {
        self.migration_analysis
            .as_ref()
            .map(|a| a.not_merged_prs.len())
            .unwrap_or(0)
    }

    /// Returns the number of unsure PRs in the migration analysis.
    pub fn unsure_count(&self) -> usize {
        self.migration_analysis
            .as_ref()
            .map(|a| a.unsure_prs.len())
            .unwrap_or(0)
    }

    // ========================================================================
    // Field Accessors (for AppState compatibility)
    // ========================================================================

    /// Returns a reference to the migration analysis, if available.
    pub fn migration_analysis(&self) -> Option<&MigrationAnalysis> {
        self.migration_analysis.as_ref()
    }

    /// Returns a mutable reference to the migration analysis, if available.
    pub fn migration_analysis_mut(&mut self) -> Option<&mut MigrationAnalysis> {
        self.migration_analysis.as_mut()
    }

    /// Sets the migration analysis.
    pub fn set_migration_analysis(&mut self, analysis: Option<MigrationAnalysis>) {
        self.migration_analysis = analysis;
    }
}

impl Deref for MigrationApp {
    type Target = AppBase<MigrationConfig>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for MigrationApp {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

impl AppMode for MigrationApp {
    type Config = MigrationConfig;

    fn base(&self) -> &AppBase<MigrationConfig> {
        &self.base
    }

    fn base_mut(&mut self) -> &mut AppBase<MigrationConfig> {
        &mut self.base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::SharedConfig, parsed_property::ParsedProperty, ui::browser::MockBrowserOpener,
    };

    fn create_test_config() -> Arc<MigrationConfig> {
        Arc::new(MigrationConfig {
            shared: SharedConfig {
                organization: ParsedProperty::Default("test_org".to_string()),
                project: ParsedProperty::Default("test_project".to_string()),
                repository: ParsedProperty::Default("test_repo".to_string()),
                pat: ParsedProperty::Default("test_pat".to_string()),
                dev_branch: ParsedProperty::Default("develop".to_string()),
                target_branch: ParsedProperty::Default("main".to_string()),
                local_repo: None,
                parallel_limit: ParsedProperty::Default(300),
                max_concurrent_network: ParsedProperty::Default(100),
                max_concurrent_processing: ParsedProperty::Default(10),
                tag_prefix: ParsedProperty::Default("merged/".to_string()),
                since: None,
                skip_confirmation: false,
            },
            terminal_states: ParsedProperty::Default(vec![
                "Closed".to_string(),
                "Resolved".to_string(),
            ]),
        })
    }

    fn create_test_client() -> AzureDevOpsClient {
        AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap()
    }

    /// # MigrationApp Initialization
    ///
    /// Tests that MigrationApp initializes correctly.
    ///
    /// ## Test Scenario
    /// - Creates MigrationApp with test config
    /// - Verifies all fields are properly initialized
    ///
    /// ## Expected Outcome
    /// - migration_analysis is None initially
    #[test]
    fn test_migration_app_initialization() {
        let app = MigrationApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        assert!(app.migration_analysis.is_none());
    }

    /// # MigrationApp Deref to AppBase
    ///
    /// Tests that Deref works for accessing AppBase fields.
    ///
    /// ## Test Scenario
    /// - Creates MigrationApp and accesses AppBase methods via Deref
    ///
    /// ## Expected Outcome
    /// - Can call AppBase methods directly on MigrationApp
    #[test]
    fn test_deref_to_app_base() {
        let app = MigrationApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        // Access AppBase methods via Deref
        assert_eq!(app.organization(), "test_org");
        assert_eq!(app.project(), "test_project");
        assert_eq!(app.dev_branch(), "develop");
    }

    /// # MigrationApp Terminal States
    ///
    /// Tests the terminal_states() getter.
    ///
    /// ## Test Scenario
    /// - Creates MigrationApp with configured terminal states
    /// - Verifies getter returns correct values
    ///
    /// ## Expected Outcome
    /// - Returns configured terminal states
    #[test]
    fn test_terminal_states() {
        let app = MigrationApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        let states = app.terminal_states();
        assert_eq!(states.len(), 2);
        assert!(states.contains(&"Closed".to_string()));
        assert!(states.contains(&"Resolved".to_string()));
    }

    /// # MigrationApp Manual Override Detection
    ///
    /// Tests has_manual_override() when no analysis exists.
    ///
    /// ## Test Scenario
    /// - Creates MigrationApp without analysis
    /// - Calls has_manual_override()
    ///
    /// ## Expected Outcome
    /// - Returns None when no analysis
    #[test]
    fn test_has_manual_override_no_analysis() {
        let app = MigrationApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        assert!(app.has_manual_override(123).is_none());
    }

    /// # MigrationApp Count Methods Without Analysis
    ///
    /// Tests count methods when no analysis exists.
    ///
    /// ## Test Scenario
    /// - Creates MigrationApp without analysis
    /// - Calls count methods
    ///
    /// ## Expected Outcome
    /// - All counts return 0
    #[test]
    fn test_count_methods_without_analysis() {
        let app = MigrationApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        assert_eq!(app.eligible_count(), 0);
        assert_eq!(app.not_eligible_count(), 0);
        assert_eq!(app.unsure_count(), 0);
    }

    /// # MigrationApp AppMode Trait
    ///
    /// Tests AppMode trait implementation.
    ///
    /// ## Test Scenario
    /// - Creates MigrationApp and uses trait methods
    ///
    /// ## Expected Outcome
    /// - base() and base_mut() work correctly
    #[test]
    fn test_app_mode_trait() {
        let mut app = MigrationApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        // Test base()
        assert_eq!(app.base().organization(), "test_org");

        // Test base_mut()
        app.base_mut().version = Some("1.0.0".to_string());
        assert_eq!(app.version, Some("1.0.0".to_string()));
    }
}
