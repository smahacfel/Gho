use std::process::Command;

fn git_output(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn git_worktree_is_clean() -> Option<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout.is_empty())
}

fn main() {
    // Re-run the provenance probe whenever any workspace source that can
    // affect the shipped launcher changes. Without these directory watches a
    // previously cached build-script result could incorrectly retain
    // `GIT_WORKTREE_CLEAN=true` after an unstaged source edit.
    for path in [
        "src",
        "../ghost-core/src",
        "../ghost-launcher/src",
        "../off-chain/components",
        "../Cargo.toml",
        "../Cargo.lock",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    if let Some(head_path) = git_output(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head_path}");
    }
    if let Some(index_path) = git_output(&["rev-parse", "--git-path", "index"]) {
        println!("cargo:rerun-if-changed={index_path}");
    }
    // Prospective PR2C provenance is derived from the checkout being built.
    // An arbitrary environment override must not be able to impersonate a
    // different source revision.
    let commit =
        git_output(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown_build_commit".to_string());
    // Failure to inspect the repository is not evidence of a clean build.
    let worktree_clean = git_worktree_is_clean().unwrap_or(false);
    println!("cargo:rustc-env=GIT_COMMIT={commit}");
    println!("cargo:rustc-env=GIT_WORKTREE_CLEAN={worktree_clean}");
}
