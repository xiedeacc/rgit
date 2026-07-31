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

/// Permit receive-pack to delete the branch currently pointed to by HEAD.
///
/// Git refuses this by default for bare repositories because clones would have
/// no checkout target. rgit refreshes HEAD after successful pushes, so allowing
/// the deletion lets users remove their old default branch and automatically
/// land on another branch.
pub async fn allow_current_branch_deletion(cfg: &GitConfig, repo: &Path) -> Result<()> {
    run_git(
        cfg,
        &["config", "receive.denyDeleteCurrent", "ignore"],
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
    let head = head_branch(cfg, repo).await?;
    if let Some(head) = head.as_deref() {
        if branch_exists(cfg, repo, head).await? {
            return Ok(Some(head.to_string()));
        }
    }

    let Some(branch) = fallback_branch(cfg, repo, head.as_deref()).await? else {
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

async fn fallback_branch(
    cfg: &GitConfig,
    repo: &Path,
    current: Option<&str>,
) -> Result<Option<String>> {
    for branch in ["master", "main"] {
        if current != Some(branch) && branch_exists(cfg, repo, branch).await? {
            return Ok(Some(branch.to_string()));
        }
    }
    Ok(branches_by_recent_commit(cfg, repo)
        .await?
        .into_iter()
        .find(|branch| Some(branch.as_str()) != current))
}

async fn branches_by_recent_commit(cfg: &GitConfig, repo: &Path) -> Result<Vec<String>> {
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
        .filter_map(|line| {
            let branch = line.trim();
            (!branch.is_empty()).then(|| branch.to_string())
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    fn temp_root(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("rgit-repo-{name}-{nonce}"))
    }

    fn git(cwd: Option<&Path>, args: &[&str]) {
        let mut command = StdCommand::new("git");
        command
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Rgit Test")
            .env("GIT_AUTHOR_EMAIL", "rgit@example.test")
            .env("GIT_COMMITTER_NAME", "Rgit Test")
            .env("GIT_COMMITTER_EMAIL", "rgit@example.test");
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let output = command.output().expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn commit_file(work: &Path, name: &str, contents: &str, message: &str, date: &str) {
        std::fs::write(work.join(name), contents).expect("write test file");
        git(Some(work), &["add", name]);
        let output = StdCommand::new("git")
            .args(["commit", "-m", message])
            .current_dir(work)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Rgit Test")
            .env("GIT_AUTHOR_EMAIL", "rgit@example.test")
            .env("GIT_COMMITTER_NAME", "Rgit Test")
            .env("GIT_COMMITTER_EMAIL", "rgit@example.test")
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .output()
            .expect("run git commit");
        assert!(
            output.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn setup_repo(name: &str, branches: &[(&str, &str)]) -> (PathBuf, PathBuf, PathBuf) {
        let root = temp_root(name);
        let bare = root.join("repo.git");
        let work = root.join("work");
        std::fs::create_dir_all(&root).expect("create temp root");
        git(
            None,
            &[
                "init",
                "--bare",
                "--initial-branch=feature",
                bare.to_str().unwrap(),
            ],
        );
        git(
            None,
            &["init", "--initial-branch=feature", work.to_str().unwrap()],
        );
        commit_file(
            &work,
            "feature.txt",
            "feature\n",
            "feature",
            "2024-01-01T00:00:00Z",
        );
        for (branch, date) in branches {
            git(Some(&work), &["checkout", "-B", branch, "feature"]);
            commit_file(
                &work,
                &format!("{branch}.txt"),
                &format!("{branch}\n"),
                branch,
                date,
            );
        }
        git(Some(&work), &["checkout", "feature"]);
        git(
            Some(&work),
            &["remote", "add", "origin", bare.to_str().unwrap()],
        );
        let mut push_args = vec!["push", "origin", "feature"];
        push_args.extend(branches.iter().map(|(branch, _)| *branch));
        git(Some(&work), &push_args);
        (root, bare, work)
    }

    #[tokio::test]
    async fn deleting_default_branch_refreshes_head_to_master() {
        let cfg = GitConfig::default();
        let (root, repo, work) = setup_repo(
            "prefer-master",
            &[
                ("main", "2024-01-02T00:00:00Z"),
                ("master", "2024-01-03T00:00:00Z"),
                ("newer", "2024-01-04T00:00:00Z"),
            ],
        );

        set_head_branch(&cfg, &repo, "feature")
            .await
            .expect("set head");
        allow_current_branch_deletion(&cfg, &repo)
            .await
            .expect("allow current deletion");
        git(Some(&work), &["push", "origin", "--delete", "feature"]);
        refresh_head_branch_after_push(&cfg, &repo)
            .await
            .expect("refresh head");

        assert_eq!(
            head_branch(&cfg, &repo)
                .await
                .expect("read head")
                .as_deref(),
            Some("master")
        );
        std::fs::remove_dir_all(root).expect("remove temp root");
    }

    #[tokio::test]
    async fn deleting_default_branch_refreshes_head_to_main_without_master() {
        let cfg = GitConfig::default();
        let (root, repo, work) = setup_repo(
            "prefer-main",
            &[
                ("main", "2024-01-02T00:00:00Z"),
                ("newer", "2024-01-04T00:00:00Z"),
            ],
        );

        set_head_branch(&cfg, &repo, "feature")
            .await
            .expect("set head");
        allow_current_branch_deletion(&cfg, &repo)
            .await
            .expect("allow current deletion");
        git(Some(&work), &["push", "origin", "--delete", "feature"]);
        refresh_head_branch_after_push(&cfg, &repo)
            .await
            .expect("refresh head");

        assert_eq!(
            head_branch(&cfg, &repo)
                .await
                .expect("read head")
                .as_deref(),
            Some("main")
        );
        std::fs::remove_dir_all(root).expect("remove temp root");
    }

    #[tokio::test]
    async fn deleting_default_branch_refreshes_head_to_recent_branch() {
        let cfg = GitConfig::default();
        let (root, repo, work) = setup_repo(
            "prefer-recent",
            &[
                ("older", "2024-01-02T00:00:00Z"),
                ("newer", "2024-01-04T00:00:00Z"),
            ],
        );

        set_head_branch(&cfg, &repo, "feature")
            .await
            .expect("set head");
        allow_current_branch_deletion(&cfg, &repo)
            .await
            .expect("allow current deletion");
        git(Some(&work), &["push", "origin", "--delete", "feature"]);
        refresh_head_branch_after_push(&cfg, &repo)
            .await
            .expect("refresh head");

        assert_eq!(
            head_branch(&cfg, &repo)
                .await
                .expect("read head")
                .as_deref(),
            Some("newer")
        );
        std::fs::remove_dir_all(root).expect("remove temp root");
    }

    #[tokio::test]
    async fn allow_current_branch_deletion_allows_git_to_delete_head_branch() {
        let cfg = GitConfig::default();
        let (root, repo, work) = setup_repo(
            "delete-previous-head",
            &[("master", "2024-01-02T00:00:00Z")],
        );

        set_head_branch(&cfg, &repo, "feature")
            .await
            .expect("set head");
        allow_current_branch_deletion(&cfg, &repo)
            .await
            .expect("allow current deletion");
        git(Some(&work), &["push", "origin", "--delete", "feature"]);
        refresh_head_branch_after_push(&cfg, &repo)
            .await
            .expect("refresh head");

        assert_eq!(
            head_branch(&cfg, &repo)
                .await
                .expect("read head")
                .as_deref(),
            Some("master")
        );
        assert!(!branch_exists(&cfg, &repo, "feature")
            .await
            .expect("check feature"));
        std::fs::remove_dir_all(root).expect("remove temp root");
    }
}
