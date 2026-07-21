//! Bare repository lifecycle: create, fork, delete, HEAD management.
//!
//! Every operation spawns the system `git` binary with explicit args
//! (never a shell) and a hard timeout.

use rgit_core::config::GitConfig;
use rgit_core::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Run a git command to completion, capturing stdout. Fails on non-zero exit.
pub async fn run_git(cfg: &GitConfig, args: &[&str], cwd: Option<&Path>) -> Result<Vec<u8>> {
    run_git_limited(cfg, args, cwd, 64 * 1024 * 1024).await
}

/// Run git while bounding captured stdout. stderr is always drained to avoid
/// pipe deadlocks, but only its first 64 KiB is retained for diagnostics.
pub async fn run_git_limited(
    cfg: &GitConfig,
    args: &[&str],
    cwd: Option<&Path>,
    max_stdout: usize,
) -> Result<Vec<u8>> {
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

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Git(format!("failed to spawn git: {e}")))?;
    let mut stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let stderr_task = tokio::spawn(drain_stderr(stderr));

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(cfg.timeout_secs);
    let read_stdout = async {
        let mut output = Vec::new();
        let mut buffer = [0_u8; 32 * 1024];
        loop {
            let read = stdout.read(&mut buffer).await?;
            if read == 0 {
                return Ok::<_, std::io::Error>(output);
            }
            if output.len().saturating_add(read) > max_stdout {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::FileTooLarge,
                    "git stdout exceeded limit",
                ));
            }
            output.extend_from_slice(&buffer[..read]);
        }
    };
    let stdout = match tokio::time::timeout_at(deadline, read_stdout).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::FileTooLarge => {
            let _ = child.kill().await;
            return Err(Error::invalid(format!(
                "git output exceeds {max_stdout} bytes"
            )));
        }
        Ok(Err(error)) => {
            let _ = child.kill().await;
            return Err(Error::Git(format!("reading git stdout failed: {error}")));
        }
        Err(_) => {
            let _ = child.kill().await;
            return Err(Error::Git(format!("git {} timed out", args.join(" "))));
        }
    };
    let status = match tokio::time::timeout_at(deadline, child.wait()).await {
        Ok(result) => result.map_err(|e| Error::Git(format!("waiting for git failed: {e}")))?,
        Err(_) => {
            let _ = child.kill().await;
            return Err(Error::Git(format!("git {} timed out", args.join(" "))));
        }
    };
    let stderr = stderr_task.await.unwrap_or_default();

    if !status.success() {
        return Err(Error::Git(format!(
            "git {} failed ({}): {}",
            args.join(" "),
            status,
            String::from_utf8_lossy(&stderr).trim()
        )));
    }
    Ok(stdout)
}

async fn drain_stderr(mut stderr: tokio::process::ChildStderr) -> Vec<u8> {
    const LOG_LIMIT: usize = 64 * 1024;
    let mut logged = Vec::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) | Err(_) => return logged,
            Ok(read) => {
                let keep = read.min(LOG_LIMIT.saturating_sub(logged.len()));
                logged.extend_from_slice(&buffer[..keep]);
            }
        }
    }
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
            path.to_str()
                .ok_or_else(|| Error::invalid("non-utf8 path"))?,
        ],
        None,
    )
    .await?;
    allow_shallow_updates(cfg, path).await?;
    Ok(())
}

/// Permit shallow clients to push shallow history into this repository.
///
/// This keeps imported third-party mirrors small when callers intentionally
/// push a single shallow branch instead of a complete upstream history.
pub async fn allow_shallow_updates(cfg: &GitConfig, repo: &Path) -> Result<()> {
    run_git(
        cfg,
        &["config", "receive.shallowUpdate", "true"],
        Some(repo),
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
            src.to_str()
                .ok_or_else(|| Error::invalid("non-utf8 path"))?,
            dst.to_str()
                .ok_or_else(|| Error::invalid("non-utf8 path"))?,
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

/// Return a usable default branch after a push.
///
/// Newly auto-created repositories start with HEAD pointing at `main`, but the
/// first push may create `master`, `develop`, or another branch. In that case,
/// move HEAD to an actual branch so repository browsing and clone defaults do
/// not point at an unborn ref.
pub async fn refresh_head_branch_after_push(
    cfg: &GitConfig,
    repo: &Path,
) -> Result<Option<String>> {
    if let Some(head) = head_branch(cfg, repo).await? {
        if branch_exists(cfg, repo, &head).await? {
            return Ok(Some(head));
        }
    }

    let Some(branch) = first_branch(cfg, repo).await? else {
        return Ok(None);
    };
    set_head_branch(cfg, repo, &branch).await?;
    Ok(Some(branch))
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

async fn branch_exists(cfg: &GitConfig, repo: &Path, branch: &str) -> Result<bool> {
    let reference = format!("refs/heads/{branch}");
    let out = run_git(
        cfg,
        &["show-ref", "--verify", "--quiet", &reference],
        Some(repo),
    )
    .await;
    match out {
        Ok(_) => Ok(true),
        Err(Error::Git(_)) => Ok(false),
        Err(error) => Err(error),
    }
}

async fn first_branch(cfg: &GitConfig, repo: &Path) -> Result<Option<String>> {
    let out = run_git(
        cfg,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/heads",
        ],
        Some(repo),
    )
    .await?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string()))
}
