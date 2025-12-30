use std::process::Command;

fn main() {
    // Tell cargo to rerun build.rs if git HEAD changes
    println!("cargo::rerun-if-changed=.git/HEAD");
    println!("cargo::rerun-if-changed=.git/refs/heads");

    // Get the short git hash (8 characters)
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Check if the working directory is dirty
    let is_dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(false);

    let dirty_suffix = if is_dirty { "-dirty" } else { "" };

    // Export the git hash as an environment variable for use in the code
    println!("cargo::rustc-env=GIT_HASH={}{}", git_hash, dirty_suffix);
}
