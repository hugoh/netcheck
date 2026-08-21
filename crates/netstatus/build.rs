use std::process::Command;

/// Computes the version string every netcheck binary reports:
/// - `NETCHECK_VERSION` env var (set by CI to the release tag, e.g. "1.2.3")
///   if present and non-empty.
/// - Otherwise `dev-<short-sha>`, from `git rev-parse --short=8 HEAD`.
/// - Otherwise (no git, e.g. a source tarball) plain `dev`.
fn main() {
    println!("cargo:rerun-if-env-changed=NETCHECK_VERSION");
    println!("cargo:rerun-if-changed=../../.git/HEAD");

    let version = std::env::var("NETCHECK_VERSION")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(git_dev_version)
        .unwrap_or_else(|| "dev".to_string());

    println!("cargo:rustc-env=NETCHECK_VERSION={version}");
}

fn git_dev_version() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?;
    Some(format!("dev-{}", sha.trim()))
}
