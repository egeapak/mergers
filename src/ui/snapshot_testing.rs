use insta::Settings;
use std::path::Path;

/// Automatically configure snapshot path based on the calling module
/// This version uses module_path!() from the calling context
pub fn with_settings<F>(f: F)
where
    F: FnOnce(),
{
    with_settings_and_module_path(module_path!(), f);
}

/// Configure snapshot path with explicit module path
/// Used when you want to override the automatic detection
pub fn with_settings_explicit<F>(module_path: &str, f: F)
where
    F: FnOnce(),
{
    with_settings_and_module_path(module_path, f);
}

/// Internal function that handles the actual configuration
pub fn with_settings_and_module_path<F>(module_path: &str, f: F)
where
    F: FnOnce(),
{
    let mut settings = Settings::clone_current();

    // Convert module path to directory structure
    // e.g., "mergers::ui::state::shared::settings_confirmation::tests"
    // becomes "state/shared/settings_confirmation"
    let path = module_path_to_snapshot_dir(module_path);

    let snapshot_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/ui/snapshots")
        .join(path);

    settings.set_snapshot_path(snapshot_path);

    // Disable module prefix since directory structure provides context
    settings.set_prepend_module_to_snapshot(false);

    settings.bind(f);
}

fn module_path_to_snapshot_dir(module_path: &str) -> String {
    // Remove crate name and "tests" suffix, keep only the UI module structure
    let without_crate = module_path
        .strip_prefix("mergers::ui::")
        .unwrap_or(module_path);

    let without_tests = without_crate
        .strip_suffix("::tests")
        .unwrap_or(without_crate);

    without_tests.replace("::", "/")
}

/// Macro to automatically use the calling module's path
#[macro_export]
macro_rules! with_snapshot_settings {
    ($test_fn:expr) => {{
        use $crate::ui::snapshot_testing::with_settings_and_module_path;
        with_settings_and_module_path(module_path!(), $test_fn)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_path_conversion() {
        let test_cases = vec![
            (
                "mergers::ui::state::shared::settings_confirmation::tests",
                "state/shared/settings_confirmation",
            ),
            (
                "mergers::ui::state::default::pr_selection::tests",
                "state/default/pr_selection",
            ),
            (
                "mergers::ui::state::migration::data_loading::tests",
                "state/migration/data_loading",
            ),
            // Edge case: no tests suffix
            (
                "mergers::ui::state::shared::settings_confirmation",
                "state/shared/settings_confirmation",
            ),
            // Edge case: no mergers prefix
            (
                "other::ui::state::shared::settings_confirmation::tests",
                "other/ui/state/shared/settings_confirmation",
            ),
        ];

        for (input, expected) in test_cases {
            let result = module_path_to_snapshot_dir(input);
            assert_eq!(result, expected, "Failed for input: {}", input);
        }
    }
}
