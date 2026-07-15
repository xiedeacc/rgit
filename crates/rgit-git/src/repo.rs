//! Bare repository lifecycle: create, fork, delete, HEAD management.
//!
//! Every operation spawns the system `git` binary with explicit args
//! (never a shell) and a hard timeout.

use rgit_core::config::GitConfig;
use rgit_core::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

/// Run a git command to completion, capturing stdout. Fails on non-zero exit.
pub async fn run_git(cfg: &GitConfig, args: &[&str], cwd: Option<&Path>) -> Result<Vec<u8>> {
    let mut cmd = Command::new(&cfg.bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_PROTOCOL", "version=2")
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let timeout = std::time::Duration::from_secs(cfg.timeout_secs);
    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| Error::Git(format!("git {} timed out", args.join(" "))))?
        .map_err(|e| Error::Git(format!("failed to spawn git: {e}")))?;

    if !output.status.success() {
        return Err(Error::Git(format!(
            "git {} failed ({}): {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

/// Create an empty bare repository with the given initial HEAD branch.
pub async fn init_bare(cfg: &GitConfig, path: &Path, default_branch: &str) -> Result<()> {
    if path.exists() {
        return Err(Error::conflict("repository directory already exists"));
    }
    std::fs::create_dir_all(path)?;
    run_git(
        cfg,
        &[
            "init",
            "--bare",
            &format!("--initial-branch={default_branch}"),
            path.to_str().ok_or_else(|| Error::invalid("non-utf8 path"))?,
        ],
        None,
    )
    .await?;
    Ok(())
}

/// Fork: bare local clone. `--local` hardlinks objects on the same
/// filesystem, so forks are near-free on disk.
pub async fn fork_local(cfg: &GitConfig, src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        return Err(Error::conflict("fork target directory already exists"));
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    run_git(
        cfg,
        &[
            "clone",
            "--bare",
            "--local",
            src.to_str().ok_or_else(|| Error::invalid("non-utf8 path"))?,
            dst.to_str().ok_or_else(|| Error::invalid("non-utf8 path"))?,
        ],
        None,
    )
    .await?;
    Ok(())
}

/// Soft-delete: rename the repo dir out of the addressable path.
/// Returns the trash path.
pub fn soft_delete(path: &Path) -> Result<PathBuf> {
    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let trash = path.with_extension(format!("deleted.{ts}"));
    std::fs::rename(path, &trash)?;
    Ok(trash)
}

/// Read the symbolic HEAD branch name (e.g. "main"), if any.
pub async fn head_branch(cfg: &GitConfig, repo: &Path) -> Result<Option<String>> {
    let out = run_git(cfg, &["symbolic-ref", "--short", "HEAD"], Some(repo)).await;
    match out {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).trim().to_string())),
        Err(_) => Ok(None), // unborn/detached HEAD
    }
}

/// Point HEAD at another branch (project default branch setting).
pub async fn set_head_branch(cfg: &GitConfig, repo: &Path, branch: &str) -> Result<()> {
    run_git(
        cfg,
        &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
        Some(repo),
    )
    .await?;
    Ok(())
}
