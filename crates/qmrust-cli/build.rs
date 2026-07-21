//! Captures build-time provenance (git commit, rustc version, target triple,
//! build profile) as `cargo:rustc-env` vars, read back via `env!(...)` in the
//! CLI's provenance sidecars. Never panics: a missing git checkout or rustc
//! probe yields an empty string rather than failing the build.

use std::process::Command;

fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=QMRUST_GIT_COMMIT={commit}");

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let rustc_version = Command::new(&rustc)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=QMRUST_RUSTC_VERSION={rustc_version}");

    println!(
        "cargo:rustc-env=QMRUST_TARGET={}",
        std::env::var("TARGET").unwrap_or_default()
    );
    println!(
        "cargo:rustc-env=QMRUST_PROFILE={}",
        std::env::var("PROFILE").unwrap_or_default()
    );

    // Rebuild when the checked-out commit changes. `HEAD` moves on
    // checkout/branch-switch; `logs/HEAD` (the reflog) moves on every commit,
    // so watching both keeps the embedded commit hash current even when a new
    // commit lands on the same branch. `git rev-parse --git-path` resolves the
    // real locations (correct under worktrees and custom git dirs).
    for path in ["HEAD", "logs/HEAD"] {
        if let Some(resolved) = Command::new("git")
            .args(["rev-parse", "--git-path", path])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
        {
            println!("cargo:rerun-if-changed={resolved}");
        }
    }
    println!("cargo:rerun-if-env-changed=TARGET");
}
