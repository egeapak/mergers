# Channel-Based Wizard - Test Specification

## Overview

This document specifies all tests required to verify the channel-based wizard implementation. Tests are organized by category and phase.

---

## 1. Unit Tests - Message Types

### 1.1 ProgressMessage Tests

```rust
#[cfg(test)]
mod progress_message_tests {
    use super::*;

    /// # ProgressMessage StepStarted Variant
    ///
    /// Tests that StepStarted variant correctly holds step information.
    ///
    /// ## Test Scenario
    /// - Create StepStarted for each WizardStep variant
    /// - Verify step is correctly stored
    ///
    /// ## Expected Outcome
    /// - Each step variant correctly represented
    #[test]
    fn test_step_started_variants() {
        let steps = [
            WizardStep::FetchDetails,
            WizardStep::CheckPrerequisites,
            WizardStep::FetchTargetBranch,
            WizardStep::CloneOrWorktree,
            WizardStep::ConfigureRepository,
            WizardStep::CreateBranch,
            WizardStep::PrepareCherryPicks,
            WizardStep::InitializeState,
        ];

        for step in steps {
            let msg = ProgressMessage::StepStarted(step);
            if let ProgressMessage::StepStarted(s) = msg {
                assert_eq!(s, step);
            } else {
                panic!("Expected StepStarted");
            }
        }
    }

    /// # ProgressMessage StepCompleted with Results
    ///
    /// Tests StepCompleted variant with various result types.
    ///
    /// ## Test Scenario
    /// - Create StepCompleted with each StepResult variant
    /// - Verify step and result correctly stored
    ///
    /// ## Expected Outcome
    /// - Step and result match input
    #[test]
    fn test_step_completed_with_results() {
        let msg = ProgressMessage::StepCompleted {
            step: WizardStep::FetchDetails,
            result: StepResult::FetchDetails {
                ssh_url: "git@ssh.dev.azure.com:v3/org/proj/repo".to_string(),
            },
        };

        if let ProgressMessage::StepCompleted { step, result } = msg {
            assert_eq!(step, WizardStep::FetchDetails);
            if let StepResult::FetchDetails { ssh_url } = result {
                assert!(ssh_url.contains("azure"));
            } else {
                panic!("Expected FetchDetails result");
            }
        }
    }

    /// # ProgressMessage Error Variant
    ///
    /// Tests Error variant preserves step and error information.
    ///
    /// ## Test Scenario
    /// - Create Error with different error types
    /// - Verify step and error preserved
    ///
    /// ## Expected Outcome
    /// - Error information accessible
    #[test]
    fn test_error_variant() {
        let msg = ProgressMessage::Error {
            step: WizardStep::CreateBranch,
            error: SetupError::Setup(git::RepositorySetupError::BranchExists(
                "patch/main-v1.0.0".to_string(),
            )),
        };

        if let ProgressMessage::Error { step, error } = msg {
            assert_eq!(step, WizardStep::CreateBranch);
            assert!(matches!(error, SetupError::Setup(_)));
        }
    }

    /// # ProgressMessage is Send + 'static
    ///
    /// Compile-time test that ProgressMessage can be sent through channels.
    ///
    /// ## Test Scenario
    /// - Attempt to use ProgressMessage with mpsc channel
    ///
    /// ## Expected Outcome
    /// - Compiles successfully
    #[test]
    fn test_progress_message_is_send() {
        fn assert_send<T: Send + 'static>() {}
        assert_send::<ProgressMessage>();
    }
}
```

### 1.2 StepResult Tests

```rust
#[cfg(test)]
mod step_result_tests {
    use super::*;
    use tempfile::TempDir;

    /// # StepResult FetchDetails
    ///
    /// Tests FetchDetails result stores SSH URL correctly.
    #[test]
    fn test_fetch_details_result() {
        let result = StepResult::FetchDetails {
            ssh_url: "git@example.com:repo.git".to_string(),
        };

        if let StepResult::FetchDetails { ssh_url } = result {
            assert_eq!(ssh_url, "git@example.com:repo.git");
        }
    }

    /// # StepResult CloneComplete
    ///
    /// Tests CloneComplete result stores path and temp_dir.
    #[test]
    fn test_clone_complete_result() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().to_path_buf();

        let result = StepResult::CloneComplete {
            path: path.clone(),
            temp_dir: temp,
        };

        if let StepResult::CloneComplete { path: p, .. } = result {
            assert_eq!(p, path);
        }
    }

    /// # StepResult WorktreeComplete
    ///
    /// Tests WorktreeComplete result stores both paths.
    #[test]
    fn test_worktree_complete_result() {
        let result = StepResult::WorktreeComplete {
            path: PathBuf::from("/repo/.worktrees/next-v1.0.0"),
            base_path: PathBuf::from("/repo"),
        };

        if let StepResult::WorktreeComplete { path, base_path } = result {
            assert!(path.ends_with("next-v1.0.0"));
            assert_eq!(base_path, PathBuf::from("/repo"));
        }
    }

    /// # StepResult CherryPicksPrepared
    ///
    /// Tests CherryPicksPrepared result stores items.
    #[test]
    fn test_cherry_picks_prepared_result() {
        let items = vec![
            CherryPickItem {
                commit_id: "abc123".to_string(),
                pr_id: 42,
                pr_title: "Fix bug".to_string(),
                status: CherryPickStatus::Pending,
            },
        ];

        let result = StepResult::CherryPicksPrepared { items: items.clone() };

        if let StepResult::CherryPicksPrepared { items: i } = result {
            assert_eq!(i.len(), 1);
            assert_eq!(i[0].pr_id, 42);
        }
    }

    /// # StepResult is Send + 'static
    ///
    /// Compile-time test for channel compatibility.
    #[test]
    fn test_step_result_is_send() {
        fn assert_send<T: Send + 'static>() {}
        assert_send::<StepResult>();
    }
}
```

---

## 2. Unit Tests - Context Extraction

### 2.1 SetupContext Tests

```rust
#[cfg(test)]
mod setup_context_tests {
    use super::*;

    /// # SetupContext Clone Mode Detection
    ///
    /// Tests that clone mode is correctly detected when no local repo.
    ///
    /// ## Test Scenario
    /// - Create MergeApp without local_repo
    /// - Extract SetupContext
    ///
    /// ## Expected Outcome
    /// - is_clone_mode is true
    /// - local_repo is None
    #[test]
    fn test_clone_mode_detection() {
        let config = create_test_config_without_local_repo();
        let mut harness = TuiTestHarness::with_config(config);
        harness.merge_app_mut().set_version(Some("v1.0.0".to_string()));

        let context = SetupContext::from_app(harness.merge_app());

        assert!(context.is_clone_mode);
        assert!(context.local_repo.is_none());
    }

    /// # SetupContext Worktree Mode Detection
    ///
    /// Tests that worktree mode is correctly detected when local repo exists.
    ///
    /// ## Test Scenario
    /// - Create MergeApp with local_repo set
    /// - Extract SetupContext
    ///
    /// ## Expected Outcome
    /// - is_clone_mode is false
    /// - local_repo is Some
    #[test]
    fn test_worktree_mode_detection() {
        let config = create_test_config_with_local_repo("/path/to/repo");
        let mut harness = TuiTestHarness::with_config(config);
        harness.merge_app_mut().set_version(Some("v1.0.0".to_string()));

        let context = SetupContext::from_app(harness.merge_app());

        assert!(!context.is_clone_mode);
        assert_eq!(context.local_repo, Some(PathBuf::from("/path/to/repo")));
    }

    /// # SetupContext Extracts Version
    ///
    /// Tests that version is correctly extracted.
    #[test]
    fn test_version_extraction() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.merge_app_mut().set_version(Some("v2.5.0".to_string()));

        let context = SetupContext::from_app(harness.merge_app());

        assert_eq!(context.version, "v2.5.0");
    }

    /// # SetupContext Extracts Target Branch
    ///
    /// Tests that target branch is correctly extracted.
    #[test]
    fn test_target_branch_extraction() {
        let config = create_test_config_with_target_branch("release/2.0");
        let mut harness = TuiTestHarness::with_config(config);
        harness.merge_app_mut().set_version(Some("v1.0.0".to_string()));

        let context = SetupContext::from_app(harness.merge_app());

        assert_eq!(context.target_branch, "release/2.0");
    }

    /// # SetupContext Extracts Selected PRs
    ///
    /// Tests that selected PRs are correctly extracted.
    ///
    /// ## Test Scenario
    /// - Create MergeApp with selected PRs
    /// - Extract SetupContext
    ///
    /// ## Expected Outcome
    /// - selected_prs contains correct PR data
    /// - commit_ids extracted from last_merge_commit
    #[test]
    fn test_selected_prs_extraction() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        // Add mock PRs and select them
        harness.add_mock_prs(vec![
            MockPr::new(101, "Fix login bug", Some("commit_abc")),
            MockPr::new(102, "Add feature X", Some("commit_def")),
            MockPr::new(103, "PR without commit", None),
        ]);
        harness.select_prs(&[101, 102, 103]);
        harness.merge_app_mut().set_version(Some("v1.0.0".to_string()));

        let context = SetupContext::from_app(harness.merge_app());

        assert_eq!(context.selected_prs.len(), 3);
        assert_eq!(context.selected_prs[0].pr_id, 101);
        assert_eq!(context.selected_prs[0].commit_id, Some("commit_abc".to_string()));
        assert_eq!(context.selected_prs[2].commit_id, None);
    }

    /// # SetupContext Extracts Run Hooks Setting
    ///
    /// Tests that run_hooks flag is correctly extracted.
    #[test]
    fn test_run_hooks_extraction() {
        let config = create_test_config_with_run_hooks(true);
        let mut harness = TuiTestHarness::with_config(config);
        harness.merge_app_mut().set_version(Some("v1.0.0".to_string()));

        let context = SetupContext::from_app(harness.merge_app());

        assert!(context.run_hooks);
    }
}
```

---

## 3. Unit Tests - Individual Step Execution

### 3.1 FetchDetails Step Tests

```rust
#[cfg(test)]
mod fetch_details_step_tests {
    use super::*;
    use mockito::Server;

    /// # FetchDetails Success
    ///
    /// Tests successful fetch of repository details from Azure DevOps.
    ///
    /// ## Test Scenario
    /// - Mock Azure DevOps API to return repo details
    /// - Execute FetchDetails step
    ///
    /// ## Expected Outcome
    /// - StepStarted message sent
    /// - StepCompleted message sent with SSH URL
    #[tokio::test]
    async fn test_fetch_details_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/org/proj/_apis/git/repositories/repo")
            .with_body(r#"{"sshUrl": "git@ssh.dev.azure.com:v3/org/proj/repo"}"#)
            .create_async()
            .await;

        let (tx, mut rx) = mpsc::channel(16);
        let client = create_mock_client(&server.url());
        let context = SetupContext {
            is_clone_mode: true,
            ..Default::default()
        };

        // Execute step
        execute_fetch_details_step(&tx, &context, &client).await;

        // Verify messages
        let msg1 = rx.recv().await.unwrap();
        assert!(matches!(msg1, ProgressMessage::StepStarted(WizardStep::FetchDetails)));

        let msg2 = rx.recv().await.unwrap();
        if let ProgressMessage::StepCompleted { step, result } = msg2 {
            assert_eq!(step, WizardStep::FetchDetails);
            if let StepResult::FetchDetails { ssh_url } = result {
                assert!(ssh_url.contains("azure"));
            }
        }

        mock.assert();
    }

    /// # FetchDetails Network Error
    ///
    /// Tests handling of network errors during fetch.
    ///
    /// ## Test Scenario
    /// - Mock API to return error
    /// - Execute FetchDetails step
    ///
    /// ## Expected Outcome
    /// - StepStarted message sent
    /// - Error message sent
    #[tokio::test]
    async fn test_fetch_details_network_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/org/proj/_apis/git/repositories/repo")
            .with_status(500)
            .create_async()
            .await;

        let (tx, mut rx) = mpsc::channel(16);
        let client = create_mock_client(&server.url());
        let context = SetupContext {
            is_clone_mode: true,
            ..Default::default()
        };

        execute_fetch_details_step(&tx, &context, &client).await;

        let msg1 = rx.recv().await.unwrap();
        assert!(matches!(msg1, ProgressMessage::StepStarted(_)));

        let msg2 = rx.recv().await.unwrap();
        assert!(matches!(msg2, ProgressMessage::Error { .. }));

        mock.assert();
    }

    /// # FetchDetails Skipped in Worktree Mode
    ///
    /// Tests that FetchDetails is skipped when not in clone mode.
    ///
    /// ## Test Scenario
    /// - Set is_clone_mode to false
    /// - Verify step is skipped
    ///
    /// ## Expected Outcome
    /// - No FetchDetails messages sent
    #[tokio::test]
    async fn test_fetch_details_skipped_worktree_mode() {
        let (tx, mut rx) = mpsc::channel(16);
        let context = SetupContext {
            is_clone_mode: false,
            local_repo: Some(PathBuf::from("/repo")),
            ..Default::default()
        };

        // In worktree mode, FetchDetails should not be called
        // This test verifies the task logic skips it
        run_setup_task_until_step(tx, context, WizardStep::CheckPrerequisites).await;

        // First message should be CheckPrerequisites, not FetchDetails
        let msg = rx.recv().await.unwrap();
        if let ProgressMessage::StepStarted(step) = msg {
            assert_eq!(step, WizardStep::CheckPrerequisites);
        }
    }
}
```

### 3.2 CheckPrerequisites Step Tests

```rust
#[cfg(test)]
mod check_prerequisites_step_tests {
    use super::*;

    /// # CheckPrerequisites Clone Mode - Has SSH URL
    ///
    /// Tests prerequisites pass when SSH URL is available.
    #[tokio::test]
    async fn test_check_prerequisites_clone_mode_success() {
        let (tx, mut rx) = mpsc::channel(16);
        let context = SetupContext {
            is_clone_mode: true,
            ..Default::default()
        };
        let ssh_url = Some("git@example.com:repo.git".to_string());

        execute_check_prerequisites_step(&tx, &context, ssh_url.as_deref()).await;

        // Should complete successfully
        let msg1 = rx.recv().await.unwrap();
        assert!(matches!(msg1, ProgressMessage::StepStarted(WizardStep::CheckPrerequisites)));

        let msg2 = rx.recv().await.unwrap();
        assert!(matches!(msg2, ProgressMessage::StepCompleted { step: WizardStep::CheckPrerequisites, .. }));
    }

    /// # CheckPrerequisites Clone Mode - Missing SSH URL
    ///
    /// Tests prerequisites fail when SSH URL is missing.
    #[tokio::test]
    async fn test_check_prerequisites_clone_mode_missing_url() {
        let (tx, mut rx) = mpsc::channel(16);
        let context = SetupContext {
            is_clone_mode: true,
            ..Default::default()
        };

        execute_check_prerequisites_step(&tx, &context, None).await;

        let msg1 = rx.recv().await.unwrap();
        assert!(matches!(msg1, ProgressMessage::StepStarted(_)));

        let msg2 = rx.recv().await.unwrap();
        assert!(matches!(msg2, ProgressMessage::Error { .. }));
    }

    /// # CheckPrerequisites Worktree Mode - Valid Repo
    ///
    /// Tests prerequisites pass when local repo exists.
    #[tokio::test]
    async fn test_check_prerequisites_worktree_mode_valid() {
        let temp = TempDir::new().unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let context = SetupContext {
            is_clone_mode: false,
            local_repo: Some(temp.path().to_path_buf()),
            ..Default::default()
        };

        execute_check_prerequisites_step(&tx, &context, None).await;

        let msg1 = rx.recv().await.unwrap();
        let msg2 = rx.recv().await.unwrap();
        assert!(matches!(msg2, ProgressMessage::StepCompleted { .. }));
    }

    /// # CheckPrerequisites Worktree Mode - Invalid Repo Path
    ///
    /// Tests prerequisites fail when local repo path doesn't exist.
    #[tokio::test]
    async fn test_check_prerequisites_worktree_mode_invalid_path() {
        let (tx, mut rx) = mpsc::channel(16);
        let context = SetupContext {
            is_clone_mode: false,
            local_repo: Some(PathBuf::from("/nonexistent/path")),
            ..Default::default()
        };

        execute_check_prerequisites_step(&tx, &context, None).await;

        let msg1 = rx.recv().await.unwrap();
        let msg2 = rx.recv().await.unwrap();
        assert!(matches!(msg2, ProgressMessage::Error { .. }));
    }
}
```

### 3.3 CloneOrWorktree Step Tests

```rust
#[cfg(test)]
mod clone_or_worktree_step_tests {
    use super::*;

    /// # Clone Repository Success
    ///
    /// Tests successful repository cloning.
    ///
    /// ## Test Scenario
    /// - Provide valid SSH URL
    /// - Execute CloneOrWorktree step in clone mode
    ///
    /// ## Expected Outcome
    /// - Repository cloned to temp directory
    /// - StepCompleted with CloneComplete result
    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_clone_repository_success() {
        let (tx, mut rx) = mpsc::channel(16);
        let context = SetupContext {
            is_clone_mode: true,
            target_branch: "main".to_string(),
            run_hooks: false,
            ..Default::default()
        };
        let ssh_url = "https://github.com/rust-lang/rust.git"; // Public repo for testing

        execute_clone_step(&tx, &context, ssh_url).await;

        let msg1 = rx.recv().await.unwrap();
        assert!(matches!(msg1, ProgressMessage::StepStarted(WizardStep::CloneOrWorktree)));

        let msg2 = rx.recv().await.unwrap();
        if let ProgressMessage::StepCompleted { result, .. } = msg2 {
            assert!(matches!(result, StepResult::CloneComplete { .. }));
        }
    }

    /// # Clone Repository Invalid URL
    ///
    /// Tests cloning with invalid SSH URL.
    #[tokio::test]
    async fn test_clone_repository_invalid_url() {
        let (tx, mut rx) = mpsc::channel(16);
        let context = SetupContext {
            is_clone_mode: true,
            target_branch: "main".to_string(),
            ..Default::default()
        };
        let ssh_url = "invalid://not-a-url";

        execute_clone_step(&tx, &context, ssh_url).await;

        let msg1 = rx.recv().await.unwrap();
        let msg2 = rx.recv().await.unwrap();
        assert!(matches!(msg2, ProgressMessage::Error { .. }));
    }

    /// # Create Worktree Success
    ///
    /// Tests successful worktree creation.
    ///
    /// ## Test Scenario
    /// - Create temp git repository
    /// - Execute CloneOrWorktree step in worktree mode
    ///
    /// ## Expected Outcome
    /// - Worktree created
    /// - StepCompleted with WorktreeComplete result
    #[tokio::test]
    async fn test_create_worktree_success() {
        let temp_repo = create_temp_git_repo();
        let (tx, mut rx) = mpsc::channel(16);
        let context = SetupContext {
            is_clone_mode: false,
            local_repo: Some(temp_repo.path().to_path_buf()),
            target_branch: "main".to_string(),
            version: "v1.0.0".to_string(),
            run_hooks: false,
            ..Default::default()
        };

        execute_worktree_step(&tx, &context).await;

        let msg1 = rx.recv().await.unwrap();
        let msg2 = rx.recv().await.unwrap();
        if let ProgressMessage::StepCompleted { result, .. } = msg2 {
            if let StepResult::WorktreeComplete { path, base_path } = result {
                assert!(path.ends_with("next-v1.0.0"));
                assert_eq!(base_path, temp_repo.path());
            }
        }
    }

    /// # Create Worktree Already Exists
    ///
    /// Tests error when worktree already exists.
    #[tokio::test]
    async fn test_create_worktree_already_exists() {
        let temp_repo = create_temp_git_repo();
        // Pre-create the worktree
        create_worktree_manually(&temp_repo, "next-v1.0.0");

        let (tx, mut rx) = mpsc::channel(16);
        let context = SetupContext {
            is_clone_mode: false,
            local_repo: Some(temp_repo.path().to_path_buf()),
            version: "v1.0.0".to_string(),
            ..Default::default()
        };

        execute_worktree_step(&tx, &context).await;

        let msg2 = rx.try_recv();
        // Should get WorktreeExists error
        if let Ok(ProgressMessage::Error { error, .. }) = msg2 {
            assert!(matches!(error, SetupError::Setup(git::RepositorySetupError::WorktreeExists(_))));
        }
    }
}
```

### 3.4 CreateBranch Step Tests

```rust
#[cfg(test)]
mod create_branch_step_tests {
    use super::*;

    /// # Create Branch Success
    ///
    /// Tests successful branch creation.
    #[tokio::test]
    async fn test_create_branch_success() {
        let temp_repo = create_temp_git_repo();
        let (tx, mut rx) = mpsc::channel(16);

        execute_create_branch_step(
            &tx,
            temp_repo.path(),
            "main",
            "v1.0.0",
        ).await;

        let msg1 = rx.recv().await.unwrap();
        assert!(matches!(msg1, ProgressMessage::StepStarted(WizardStep::CreateBranch)));

        let msg2 = rx.recv().await.unwrap();
        if let ProgressMessage::StepCompleted { result, .. } = msg2 {
            if let StepResult::BranchCreated { branch_name } = result {
                assert_eq!(branch_name, "patch/main-v1.0.0");
            }
        }

        // Verify branch actually exists
        assert!(branch_exists(temp_repo.path(), "patch/main-v1.0.0"));
    }

    /// # Create Branch Already Exists
    ///
    /// Tests error when branch already exists.
    #[tokio::test]
    async fn test_create_branch_already_exists() {
        let temp_repo = create_temp_git_repo();
        // Pre-create the branch
        create_branch_manually(&temp_repo, "patch/main-v1.0.0");

        let (tx, mut rx) = mpsc::channel(16);

        execute_create_branch_step(
            &tx,
            temp_repo.path(),
            "main",
            "v1.0.0",
        ).await;

        let msg2 = rx.recv().await.unwrap();
        let msg2 = rx.recv().await.unwrap();
        if let ProgressMessage::Error { error, .. } = msg2 {
            assert!(matches!(error, SetupError::Setup(git::RepositorySetupError::BranchExists(_))));
        }
    }
}
```

### 3.5 PrepareCherryPicks Step Tests

```rust
#[cfg(test)]
mod prepare_cherry_picks_step_tests {
    use super::*;

    /// # Prepare Cherry-Picks Success
    ///
    /// Tests successful preparation of cherry-pick items.
    #[tokio::test]
    async fn test_prepare_cherry_picks_success() {
        let (tx, mut rx) = mpsc::channel(16);
        let selected_prs = vec![
            SelectedPrData {
                pr_id: 101,
                pr_title: "Fix bug".to_string(),
                commit_id: Some("abc123".to_string()),
            },
            SelectedPrData {
                pr_id: 102,
                pr_title: "Add feature".to_string(),
                commit_id: Some("def456".to_string()),
            },
        ];

        execute_prepare_cherry_picks_step(&tx, &selected_prs).await;

        let msg1 = rx.recv().await.unwrap();
        assert!(matches!(msg1, ProgressMessage::StepStarted(WizardStep::PrepareCherryPicks)));

        let msg2 = rx.recv().await.unwrap();
        if let ProgressMessage::StepCompleted { result, .. } = msg2 {
            if let StepResult::CherryPicksPrepared { items } = result {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].pr_id, 101);
                assert_eq!(items[0].commit_id, "abc123");
                assert!(matches!(items[0].status, CherryPickStatus::Pending));
            }
        }
    }

    /// # Prepare Cherry-Picks Filters PRs Without Commits
    ///
    /// Tests that PRs without commit IDs are filtered out.
    #[tokio::test]
    async fn test_prepare_cherry_picks_filters_no_commits() {
        let (tx, mut rx) = mpsc::channel(16);
        let selected_prs = vec![
            SelectedPrData {
                pr_id: 101,
                pr_title: "Has commit".to_string(),
                commit_id: Some("abc123".to_string()),
            },
            SelectedPrData {
                pr_id: 102,
                pr_title: "No commit".to_string(),
                commit_id: None,
            },
        ];

        execute_prepare_cherry_picks_step(&tx, &selected_prs).await;

        let msg2 = rx.recv().await.unwrap();
        let msg2 = rx.recv().await.unwrap();
        if let ProgressMessage::StepCompleted { result, .. } = msg2 {
            if let StepResult::CherryPicksPrepared { items } = result {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].pr_id, 101);
            }
        }
    }

    /// # Prepare Cherry-Picks Empty List
    ///
    /// Tests error when no PRs have commits.
    #[tokio::test]
    async fn test_prepare_cherry_picks_empty() {
        let (tx, mut rx) = mpsc::channel(16);
        let selected_prs = vec![
            SelectedPrData {
                pr_id: 101,
                pr_title: "No commit".to_string(),
                commit_id: None,
            },
        ];

        execute_prepare_cherry_picks_step(&tx, &selected_prs).await;

        let msg2 = rx.recv().await.unwrap();
        let msg2 = rx.recv().await.unwrap();
        assert!(matches!(msg2, ProgressMessage::Error { .. }));
    }
}
```

---

## 4. Integration Tests - Full Task Flow

```rust
#[cfg(test)]
mod task_integration_tests {
    use super::*;

    /// # Full Clone Mode Flow
    ///
    /// Tests complete execution of all steps in clone mode.
    ///
    /// ## Test Scenario
    /// - Mock API for FetchDetails
    /// - Execute full task
    /// - Verify all messages in correct order
    ///
    /// ## Expected Outcome
    /// - All 7 steps complete
    /// - AllComplete message at end
    #[tokio::test]
    #[ignore] // Requires mock setup
    async fn test_full_clone_mode_flow() {
        let (tx, mut rx) = mpsc::channel(32);
        let context = SetupContext {
            is_clone_mode: true,
            target_branch: "main".to_string(),
            version: "v1.0.0".to_string(),
            selected_prs: vec![
                SelectedPrData {
                    pr_id: 1,
                    pr_title: "Test".to_string(),
                    commit_id: Some("abc".to_string()),
                },
            ],
            ..Default::default()
        };
        let client = create_mock_client_with_repo_details();

        tokio::spawn(run_setup_task(tx, context, client));

        // Collect all messages
        let mut messages = Vec::new();
        while let Some(msg) = rx.recv().await {
            let is_complete = matches!(msg, ProgressMessage::AllComplete);
            messages.push(msg);
            if is_complete {
                break;
            }
        }

        // Verify step sequence for clone mode
        let expected_steps = [
            WizardStep::FetchDetails,
            WizardStep::CheckPrerequisites,
            WizardStep::CloneOrWorktree,
            WizardStep::CreateBranch,
            WizardStep::PrepareCherryPicks,
            WizardStep::InitializeState,
        ];

        let mut step_index = 0;
        for msg in &messages {
            if let ProgressMessage::StepStarted(step) = msg {
                assert_eq!(*step, expected_steps[step_index]);
                step_index += 1;
            }
        }

        assert!(matches!(messages.last(), Some(ProgressMessage::AllComplete)));
    }

    /// # Full Worktree Mode Flow
    ///
    /// Tests complete execution of all steps in worktree mode.
    #[tokio::test]
    async fn test_full_worktree_mode_flow() {
        let temp_repo = create_temp_git_repo_with_remote();
        let (tx, mut rx) = mpsc::channel(32);
        let context = SetupContext {
            is_clone_mode: false,
            local_repo: Some(temp_repo.path().to_path_buf()),
            target_branch: "main".to_string(),
            version: "v1.0.0".to_string(),
            selected_prs: vec![
                SelectedPrData {
                    pr_id: 1,
                    pr_title: "Test".to_string(),
                    commit_id: Some("abc".to_string()),
                },
            ],
            ..Default::default()
        };

        tokio::spawn(run_setup_task(tx, context, create_dummy_client()));

        // Verify step sequence for worktree mode (no FetchDetails)
        let expected_steps = [
            WizardStep::CheckPrerequisites,
            WizardStep::FetchTargetBranch,
            WizardStep::CloneOrWorktree,
            WizardStep::CreateBranch,
            WizardStep::PrepareCherryPicks,
            WizardStep::InitializeState,
        ];

        let mut messages = Vec::new();
        while let Some(msg) = rx.recv().await {
            let is_complete = matches!(msg, ProgressMessage::AllComplete);
            messages.push(msg);
            if is_complete {
                break;
            }
        }

        // Verify FetchDetails was skipped
        for msg in &messages {
            if let ProgressMessage::StepStarted(step) = msg {
                assert_ne!(*step, WizardStep::FetchDetails);
            }
        }
    }

    /// # Task Cancellation
    ///
    /// Tests that task can be cancelled mid-execution.
    #[tokio::test]
    async fn test_task_cancellation() {
        let (tx, mut rx) = mpsc::channel(16);
        let context = SetupContext {
            is_clone_mode: true,
            ..Default::default()
        };

        let handle = tokio::spawn(run_setup_task(tx, context, create_slow_mock_client()));

        // Wait for first message
        let _ = rx.recv().await;

        // Abort the task
        handle.abort();

        // Task should be aborted
        assert!(handle.await.is_err());
    }

    /// # Channel Disconnect Handling
    ///
    /// Tests that task exits gracefully when receiver is dropped.
    #[tokio::test]
    async fn test_channel_disconnect() {
        let (tx, rx) = mpsc::channel(16);
        let context = SetupContext {
            is_clone_mode: true,
            ..Default::default()
        };

        let handle = tokio::spawn(run_setup_task(tx, context, create_mock_client_success()));

        // Drop receiver
        drop(rx);

        // Task should exit without panic
        let result = handle.await;
        assert!(result.is_ok());
    }
}
```

---

## 5. UI State Tests

### 5.1 State Transition Tests

```rust
#[cfg(test)]
mod state_transition_tests {
    use super::*;

    /// # Idle to Running Transition
    ///
    /// Tests that first process_key transitions from Idle to Running.
    #[tokio::test]
    async fn test_idle_to_running() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        harness.merge_app_mut().set_version(Some("v1.0.0".to_string()));

        let mut state = SetupRepoState::new();
        assert!(matches!(state.state, SetupState::Idle));

        let result = state.process_key(KeyCode::Null, harness.merge_app_mut()).await;

        assert!(matches!(state.state, SetupState::Running { .. }));
        assert!(matches!(result, StateChange::Keep));
    }

    /// # Running Receives Progress Updates
    ///
    /// Tests that Running state processes channel messages.
    #[tokio::test]
    async fn test_running_processes_updates() {
        let mut state = SetupRepoState::new();
        // Manually set up Running state with a test channel
        let (tx, rx) = mpsc::channel(16);
        state.state = SetupState::Running {
            progress: WizardProgress::new(true),
            progress_rx: rx,
            task_handle: tokio::spawn(async {}),
            pending_results: VecDeque::new(),
        };

        // Send a progress message
        tx.send(ProgressMessage::StepStarted(WizardStep::FetchDetails)).await.unwrap();

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let _ = state.process_key(KeyCode::Null, harness.merge_app_mut()).await;

        // Verify progress updated
        if let SetupState::Running { progress, .. } = &state.state {
            assert!(matches!(progress.current_step, Some(WizardStep::FetchDetails)));
        }
    }

    /// # Running to Complete Transition
    ///
    /// Tests transition to CherryPick state on AllComplete.
    #[tokio::test]
    async fn test_running_to_complete() {
        let mut state = SetupRepoState::new();
        let (tx, rx) = mpsc::channel(16);
        state.state = SetupState::Running {
            progress: WizardProgress::new(true),
            progress_rx: rx,
            task_handle: tokio::spawn(async {}),
            pending_results: VecDeque::new(),
        };

        tx.send(ProgressMessage::AllComplete).await.unwrap();

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let result = state.process_key(KeyCode::Null, harness.merge_app_mut()).await;

        assert!(matches!(result, StateChange::Change(MergeState::CherryPick(_))));
    }

    /// # Running to Error Transition
    ///
    /// Tests transition to Error state on error message.
    #[tokio::test]
    async fn test_running_to_error() {
        let mut state = SetupRepoState::new();
        let (tx, rx) = mpsc::channel(16);
        state.state = SetupState::Running {
            progress: WizardProgress::new(true),
            progress_rx: rx,
            task_handle: tokio::spawn(async {}),
            pending_results: VecDeque::new(),
        };

        tx.send(ProgressMessage::Error {
            step: WizardStep::FetchDetails,
            error: SetupError::General("Network error".to_string()),
        }).await.unwrap();

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let _ = state.process_key(KeyCode::Null, harness.merge_app_mut()).await;

        assert!(matches!(state.state, SetupState::Error { .. }));
    }

    /// # Error Retry Resets to Idle
    ///
    /// Tests that 'r' key in Error state resets to Idle.
    #[tokio::test]
    async fn test_error_retry() {
        let mut state = SetupRepoState::new();
        state.state = SetupState::Error {
            error: SetupError::General("Test error".to_string()),
            message: "Error occurred".to_string(),
            progress: None,
        };

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let result = state.process_key(KeyCode::Char('r'), harness.merge_app_mut()).await;

        assert!(matches!(state.state, SetupState::Idle));
        assert!(matches!(result, StateChange::Keep));
    }

    /// # Error Escape Exits
    ///
    /// Tests that Esc in Error state transitions to ErrorState.
    #[tokio::test]
    async fn test_error_escape() {
        let mut state = SetupRepoState::new();
        state.state = SetupState::Error {
            error: SetupError::General("Test error".to_string()),
            message: "Error occurred".to_string(),
            progress: None,
        };

        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);

        let result = state.process_key(KeyCode::Esc, harness.merge_app_mut()).await;

        assert!(matches!(result, StateChange::Change(MergeState::Error(_))));
    }
}
```

### 5.2 Result Application Tests

```rust
#[cfg(test)]
mod result_application_tests {
    use super::*;

    /// # Apply CloneComplete Result
    ///
    /// Tests that CloneComplete result updates MergeApp correctly.
    #[test]
    fn test_apply_clone_complete() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        let mut state = SetupRepoState::new();

        let temp = TempDir::new().unwrap();
        let path = temp.path().to_path_buf();
        let result = StepResult::CloneComplete {
            path: path.clone(),
            temp_dir: temp,
        };

        state.apply_result(result, harness.merge_app_mut());

        assert_eq!(harness.merge_app().repo_path(), Some(path.as_path()));
    }

    /// # Apply WorktreeComplete Result
    ///
    /// Tests that WorktreeComplete result updates MergeApp correctly.
    #[test]
    fn test_apply_worktree_complete() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        let mut state = SetupRepoState::new();

        let result = StepResult::WorktreeComplete {
            path: PathBuf::from("/repo/.worktrees/next-v1.0.0"),
            base_path: PathBuf::from("/repo"),
        };

        state.apply_result(result, harness.merge_app_mut());

        assert_eq!(
            harness.merge_app().repo_path(),
            Some(Path::new("/repo/.worktrees/next-v1.0.0"))
        );
        assert_eq!(
            harness.merge_app().worktree.base_repo_path,
            Some(PathBuf::from("/repo"))
        );
    }

    /// # Apply CherryPicksPrepared Result
    ///
    /// Tests that CherryPicksPrepared result updates cherry_pick_items.
    #[test]
    fn test_apply_cherry_picks_prepared() {
        let config = create_test_config_default();
        let mut harness = TuiTestHarness::with_config(config);
        let mut state = SetupRepoState::new();

        let items = vec![
            CherryPickItem {
                commit_id: "abc".to_string(),
                pr_id: 1,
                pr_title: "Test".to_string(),
                status: CherryPickStatus::Pending,
            },
        ];
        let result = StepResult::CherryPicksPrepared { items };

        state.apply_result(result, harness.merge_app_mut());

        assert_eq!(harness.merge_app().cherry_pick_items().len(), 1);
    }
}
```

---

## 6. UI Snapshot Tests

```rust
#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use crate::ui::{
        snapshot_testing::with_settings_and_module_path,
        testing::{TuiTestHarness, create_test_config_default},
    };
    use insta::assert_snapshot;

    /// Helper to create Running state with given progress
    fn make_running_state(progress: WizardProgress) -> SetupState {
        let (_, rx) = mpsc::channel(1);
        SetupState::Running {
            progress,
            progress_rx: rx,
            task_handle: tokio::spawn(async {}),
            pending_results: VecDeque::new(),
        }
    }

    /// # Snapshot: Idle State
    ///
    /// Tests UI rendering of initial Idle state.
    #[test]
    fn test_snapshot_idle_state() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut state = MergeState::SetupRepo(SetupRepoState::new());
            harness.render_merge_state(&mut state);

            assert_snapshot!("idle", harness.backend());
        });
    }

    /// # Snapshot: Clone Mode - FetchDetails In Progress
    #[test]
    fn test_snapshot_clone_fetch_details() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner = SetupRepoState::new();
            let mut progress = WizardProgress::new(true);
            progress.start_step(WizardStep::FetchDetails);
            inner.state = make_running_state(progress);
            inner.is_clone_mode = Some(true);

            let mut state = MergeState::SetupRepo(inner);
            harness.render_merge_state(&mut state);

            assert_snapshot!("clone_fetch_details", harness.backend());
        });
    }

    /// # Snapshot: Clone Mode - CheckPrerequisites In Progress
    #[test]
    fn test_snapshot_clone_check_prerequisites() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner = SetupRepoState::new();
            let mut progress = WizardProgress::new(true);
            progress.complete_step(WizardStep::FetchDetails);
            progress.start_step(WizardStep::CheckPrerequisites);
            inner.state = make_running_state(progress);
            inner.is_clone_mode = Some(true);

            let mut state = MergeState::SetupRepo(inner);
            harness.render_merge_state(&mut state);

            assert_snapshot!("clone_check_prerequisites", harness.backend());
        });
    }

    /// # Snapshot: Clone Mode - Cloning In Progress
    #[test]
    fn test_snapshot_clone_cloning() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner = SetupRepoState::new();
            let mut progress = WizardProgress::new(true);
            progress.complete_step(WizardStep::FetchDetails);
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.start_step(WizardStep::CloneOrWorktree);
            inner.state = make_running_state(progress);
            inner.is_clone_mode = Some(true);

            let mut state = MergeState::SetupRepo(inner);
            harness.render_merge_state(&mut state);

            assert_snapshot!("clone_cloning", harness.backend());
        });
    }

    /// # Snapshot: Worktree Mode - FetchTargetBranch In Progress
    #[test]
    fn test_snapshot_worktree_fetch_branch() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner = SetupRepoState::new();
            let mut progress = WizardProgress::new(false);
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.start_step(WizardStep::FetchTargetBranch);
            inner.state = make_running_state(progress);
            inner.is_clone_mode = Some(false);

            let mut state = MergeState::SetupRepo(inner);
            harness.render_merge_state(&mut state);

            assert_snapshot!("worktree_fetch_branch", harness.backend());
        });
    }

    /// # Snapshot: Worktree Mode - Creating Worktree
    #[test]
    fn test_snapshot_worktree_creating() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner = SetupRepoState::new();
            let mut progress = WizardProgress::new(false);
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.complete_step(WizardStep::FetchTargetBranch);
            progress.start_step(WizardStep::CloneOrWorktree);
            inner.state = make_running_state(progress);
            inner.is_clone_mode = Some(false);

            let mut state = MergeState::SetupRepo(inner);
            harness.render_merge_state(&mut state);

            assert_snapshot!("worktree_creating", harness.backend());
        });
    }

    /// # Snapshot: CreateBranch In Progress
    #[test]
    fn test_snapshot_create_branch() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner = SetupRepoState::new();
            let mut progress = WizardProgress::new(false);
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.complete_step(WizardStep::FetchTargetBranch);
            progress.complete_step(WizardStep::CloneOrWorktree);
            progress.start_step(WizardStep::CreateBranch);
            inner.state = make_running_state(progress);
            inner.is_clone_mode = Some(false);

            let mut state = MergeState::SetupRepo(inner);
            harness.render_merge_state(&mut state);

            assert_snapshot!("create_branch", harness.backend());
        });
    }

    /// # Snapshot: PrepareCherryPicks In Progress
    #[test]
    fn test_snapshot_prepare_cherry_picks() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner = SetupRepoState::new();
            let mut progress = WizardProgress::new(false);
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.complete_step(WizardStep::FetchTargetBranch);
            progress.complete_step(WizardStep::CloneOrWorktree);
            progress.complete_step(WizardStep::CreateBranch);
            progress.start_step(WizardStep::PrepareCherryPicks);
            inner.state = make_running_state(progress);
            inner.is_clone_mode = Some(false);

            let mut state = MergeState::SetupRepo(inner);
            harness.render_merge_state(&mut state);

            assert_snapshot!("prepare_cherry_picks", harness.backend());
        });
    }

    /// # Snapshot: InitializeState In Progress
    #[test]
    fn test_snapshot_initialize_state() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner = SetupRepoState::new();
            let mut progress = WizardProgress::new(false);
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.complete_step(WizardStep::FetchTargetBranch);
            progress.complete_step(WizardStep::CloneOrWorktree);
            progress.complete_step(WizardStep::CreateBranch);
            progress.complete_step(WizardStep::PrepareCherryPicks);
            progress.start_step(WizardStep::InitializeState);
            inner.state = make_running_state(progress);
            inner.is_clone_mode = Some(false);

            let mut state = MergeState::SetupRepo(inner);
            harness.render_merge_state(&mut state);

            assert_snapshot!("initialize_state", harness.backend());
        });
    }

    /// # Snapshot: All Steps Complete (Clone Mode)
    #[test]
    fn test_snapshot_clone_all_complete() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner = SetupRepoState::new();
            let mut progress = WizardProgress::new(true);
            progress.complete_step(WizardStep::FetchDetails);
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.complete_step(WizardStep::CloneOrWorktree);
            progress.complete_step(WizardStep::CreateBranch);
            progress.complete_step(WizardStep::PrepareCherryPicks);
            progress.complete_step(WizardStep::InitializeState);
            inner.state = make_running_state(progress);
            inner.is_clone_mode = Some(true);

            let mut state = MergeState::SetupRepo(inner);
            harness.render_merge_state(&mut state);

            assert_snapshot!("clone_all_complete", harness.backend());
        });
    }

    /// # Snapshot: Error - Branch Exists
    #[test]
    fn test_snapshot_error_branch_exists() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner = SetupRepoState::new();
            let mut progress = WizardProgress::new(true);
            progress.complete_step(WizardStep::FetchDetails);
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.complete_step(WizardStep::CloneOrWorktree);
            progress.start_step(WizardStep::CreateBranch);

            inner.state = SetupState::Error {
                error: SetupError::Setup(git::RepositorySetupError::BranchExists(
                    "patch/main-v1.0.0".to_string(),
                )),
                message: "Branch 'patch/main-v1.0.0' already exists.\n\nOptions:\n  • Press 'r' to retry\n  • Press 'f' to force delete\n  • Press 'Esc' to go back".to_string(),
                progress: Some(progress),
            };
            inner.is_clone_mode = Some(true);

            let mut state = MergeState::SetupRepo(inner);
            harness.render_merge_state(&mut state);

            assert_snapshot!("error_branch_exists", harness.backend());
        });
    }

    /// # Snapshot: Error - Worktree Exists
    #[test]
    fn test_snapshot_error_worktree_exists() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner = SetupRepoState::new();
            let mut progress = WizardProgress::new(false);
            progress.complete_step(WizardStep::CheckPrerequisites);
            progress.complete_step(WizardStep::FetchTargetBranch);
            progress.start_step(WizardStep::CloneOrWorktree);

            inner.state = SetupState::Error {
                error: SetupError::Setup(git::RepositorySetupError::WorktreeExists(
                    "/repo/.worktrees/next-v1.0.0".to_string(),
                )),
                message: "Worktree already exists.\n\nOptions:\n  • Press 'r' to retry\n  • Press 'f' to force remove\n  • Press 'Esc' to go back".to_string(),
                progress: Some(progress),
            };
            inner.is_clone_mode = Some(false);

            let mut state = MergeState::SetupRepo(inner);
            harness.render_merge_state(&mut state);

            assert_snapshot!("error_worktree_exists", harness.backend());
        });
    }

    /// # Snapshot: Error - Network Error (No Progress)
    #[test]
    fn test_snapshot_error_network() {
        with_settings_and_module_path(module_path!(), || {
            let config = create_test_config_default();
            let mut harness = TuiTestHarness::with_config(config);

            let mut inner = SetupRepoState::new();
            let mut progress = WizardProgress::new(true);
            progress.start_step(WizardStep::FetchDetails);

            inner.state = SetupState::Error {
                error: SetupError::General("Network timeout".to_string()),
                message: "Failed to fetch repository details: Network timeout\n\nOptions:\n  • Press 'r' to retry\n  • Press 'Esc' to go back".to_string(),
                progress: Some(progress),
            };
            inner.is_clone_mode = Some(true);

            let mut state = MergeState::SetupRepo(inner);
            harness.render_merge_state(&mut state);

            assert_snapshot!("error_network", harness.backend());
        });
    }
}
```

---

## 7. Test Helpers

```rust
#[cfg(test)]
mod test_helpers {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    /// Creates a temporary git repository for testing.
    pub fn create_temp_git_repo() -> TempDir {
        let temp = TempDir::new().unwrap();
        Command::new("git")
            .current_dir(temp.path())
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(temp.path())
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(temp.path())
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        // Create initial commit
        std::fs::write(temp.path().join("README.md"), "# Test").unwrap();
        Command::new("git")
            .current_dir(temp.path())
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(temp.path())
            .args(["commit", "-m", "Initial"])
            .output()
            .unwrap();
        temp
    }

    /// Creates a temp git repo with a fake remote.
    pub fn create_temp_git_repo_with_remote() -> TempDir {
        let temp = create_temp_git_repo();
        // Add a fake remote
        Command::new("git")
            .current_dir(temp.path())
            .args(["remote", "add", "origin", "https://example.com/repo.git"])
            .output()
            .unwrap();
        temp
    }

    /// Creates a branch manually in the repo.
    pub fn create_branch_manually(repo: &TempDir, branch_name: &str) {
        Command::new("git")
            .current_dir(repo.path())
            .args(["branch", branch_name])
            .output()
            .unwrap();
    }

    /// Checks if a branch exists in the repo.
    pub fn branch_exists(repo_path: &Path, branch_name: &str) -> bool {
        let output = Command::new("git")
            .current_dir(repo_path)
            .args(["branch", "--list", branch_name])
            .output()
            .unwrap();
        !output.stdout.is_empty()
    }

    /// Creates a worktree manually in the repo.
    pub fn create_worktree_manually(repo: &TempDir, name: &str) {
        let worktree_path = repo.path().join(name);
        Command::new("git")
            .current_dir(repo.path())
            .args(["worktree", "add", worktree_path.to_str().unwrap(), "HEAD"])
            .output()
            .unwrap();
    }

    /// Creates a mock Azure DevOps client.
    pub fn create_mock_client(base_url: &str) -> AzureDevOpsClient {
        AzureDevOpsClient::new_with_base_url(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
            base_url.to_string(),
        ).unwrap()
    }

    /// Creates a dummy client (for tests that don't need network).
    pub fn create_dummy_client() -> AzureDevOpsClient {
        AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        ).unwrap()
    }
}
```

---

## 8. Test Matrix Summary

| Test Category | Count | Coverage |
|---------------|-------|----------|
| Message Types | 6 | ProgressMessage, StepResult |
| Context Extraction | 6 | SetupContext fields |
| FetchDetails Step | 3 | Success, Error, Skip |
| CheckPrerequisites Step | 4 | Clone/Worktree × Valid/Invalid |
| CloneOrWorktree Step | 4 | Clone/Worktree × Success/Error |
| CreateBranch Step | 2 | Success, Already Exists |
| PrepareCherryPicks Step | 3 | Success, Filter, Empty |
| Task Integration | 4 | Full flows, Cancellation |
| State Transitions | 6 | All transitions |
| Result Application | 3 | Clone, Worktree, CherryPicks |
| UI Snapshots | 12 | All visual states |
| **Total** | **53** | |

---

## 9. Running Tests

```bash
# Run all channel wizard tests
cargo test wizard_channel --lib

# Run specific category
cargo test progress_message_tests --lib
cargo test setup_context_tests --lib
cargo test fetch_details_step_tests --lib
cargo test snapshot_tests --lib

# Run with coverage
cargo llvm-cov nextest --test wizard_channel

# Update snapshots
cargo insta review

# Run integration tests (may require network)
cargo test task_integration_tests --lib -- --ignored
```
