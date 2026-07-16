//! Read-only repository inspection for the browsing API (DESIGN.md §10).
//! Parses porcelain-stable plumbing output (`ls-tree -z`, `log -z`,
//! `for-each-ref`, `cat-file`).

use rgit_core::config::GitConfig;
use rgit_core::{Error, Result};
use serde::Serialize;
use std::path::Path;

use crate::repo::{run_git, run_git_limited};

const TREE_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const LOG_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const COMMIT_OUTPUT_LIMIT: usize = 2 * 1024 * 1024;
const DIFF_OUTPUT_LIMIT: usize = 32 * 1024 * 1024;

/// Refuse refs/paths that could be mistaken for options.
fn check_arg(s: &str) -> Result<&str> {
    if s.starts_with('-') || s.contains('\0') {
        return Err(Error::invalid("invalid ref or path"));
    }
    Ok(s)
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    pub kind: String, // "blob" | "tree" | "commit" (submodule)
    pub mode: String,
    pub sha: String,
    pub size: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitInfo {
    pub sha: String,
    pub author_name: String,
    pub author_email: String,
    pub authored_at: String, // ISO-8601
    pub message: String,
    pub parents: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefInfo {
    pub name: String,
    pub sha: String,
    /// Peeled commit for annotated tags.
    pub target_sha: Option<String>,
}

/// `git ls-tree -z --long <ref> -- <path>/` — one directory level.
pub async fn list_tree(
    cfg: &GitConfig,
    repo: &Path,
    reference: &str,
    path: &str,
) -> Result<Vec<TreeEntry>> {
    check_arg(reference)?;
    check_arg(path)?;
    let treeish = if path.is_empty() {
        reference.to_string()
    } else {
        format!("{reference}:{path}")
    };
    let out = run_git_limited(
        cfg,
        &["ls-tree", "-z", "--long", &treeish],
        Some(repo),
        TREE_OUTPUT_LIMIT,
    )
    .await?;

    let mut entries = Vec::new();
    for record in out.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        // "<mode> <type> <sha> <size>\t<name>"
        let record = String::from_utf8_lossy(record);
        let Some((meta, name)) = record.split_once('\t') else {
            continue;
        };
        let fields: Vec<&str> = meta.split_whitespace().collect();
        if fields.len() != 4 {
            continue;
        }
        entries.push(TreeEntry {
            name: name.to_string(),
            path: if path.is_empty() {
                name.to_string()
            } else {
                format!("{path}/{name}")
            },
            mode: fields[0].to_string(),
            kind: fields[1].to_string(),
            sha: fields[2].to_string(),
            size: fields[3].parse().ok(),
        });
    }
    // Directories first, then files, both alphabetical (GitHub-style).
    entries.sort_by(|a, b| {
        (b.kind == "tree")
            .cmp(&(a.kind == "tree"))
            .then(a.name.cmp(&b.name))
    });
    Ok(entries)
}

/// Raw blob bytes at `<ref>:<path>`. `limit` guards memory (0 = unlimited).
pub async fn read_blob(
    cfg: &GitConfig,
    repo: &Path,
    reference: &str,
    path: &str,
    limit: usize,
) -> Result<Vec<u8>> {
    check_arg(reference)?;
    check_arg(path)?;
    let spec = format!("{reference}:{path}");
    let size = run_git_limited(cfg, &["cat-file", "-s", &spec], Some(repo), 64)
        .await?
        .split(|byte| byte.is_ascii_whitespace())
        .next()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| Error::Git("git cat-file returned an invalid size".into()))?;
    if limit > 0 && size > limit {
        return Err(Error::invalid("blob exceeds size limit"));
    }
    let output_limit = if limit > 0 { limit } else { size };
    run_git_limited(cfg, &["cat-file", "blob", &spec], Some(repo), output_limit).await
}

const LOG_FORMAT: &str = "%H%x00%an%x00%ae%x00%aI%x00%P%x00%B";

pub async fn count_commits(
    cfg: &GitConfig,
    repo: &Path,
    reference: &str,
    path: Option<&str>,
) -> Result<i64> {
    check_arg(reference)?;
    let mut args = vec!["rev-list", "--count", reference];
    if let Some(path) = path {
        check_arg(path)?;
        args.push("--");
        args.push(path);
    }
    let output = run_git_limited(cfg, &args, Some(repo), 64).await?;
    String::from_utf8_lossy(&output)
        .trim()
        .parse::<i64>()
        .map_err(|_| Error::Git("git rev-list returned an invalid count".into()))
}

/// Commit history of a ref (optionally limited to a path), newest first.
pub async fn log(
    cfg: &GitConfig,
    repo: &Path,
    reference: &str,
    path: Option<&str>,
    skip: u32,
    limit: u32,
) -> Result<Vec<CommitInfo>> {
    check_arg(reference)?;
    let skip = format!("--skip={skip}");
    let max = format!("--max-count={limit}");
    let format = format!("--format={LOG_FORMAT}");
    let mut args = vec!["log", "-z", &format, &skip, &max, reference];
    if let Some(p) = path {
        check_arg(p)?;
        args.push("--");
        args.push(p);
    }
    let out = run_git_limited(cfg, &args, Some(repo), LOG_OUTPUT_LIMIT).await?;
    parse_commits(&out)
}

/// Single commit metadata.
pub async fn commit(cfg: &GitConfig, repo: &Path, sha: &str) -> Result<CommitInfo> {
    check_arg(sha)?;
    let format = format!("--format={LOG_FORMAT}");
    let out = run_git_limited(
        cfg,
        &["log", "-z", &format, "--max-count=1", sha],
        Some(repo),
        COMMIT_OUTPUT_LIMIT,
    )
    .await?;
    parse_commits(&out)?
        .into_iter()
        .next()
        .ok_or(Error::NotFound)
}

/// Unified diff of one commit against its first parent.
pub async fn commit_diff(cfg: &GitConfig, repo: &Path, sha: &str) -> Result<Vec<u8>> {
    check_arg(sha)?;
    run_git_limited(
        cfg,
        &["diff-tree", "-p", "--root", "--no-commit-id", sha],
        Some(repo),
        DIFF_OUTPUT_LIMIT,
    )
    .await
}

fn parse_commits(out: &[u8]) -> Result<Vec<CommitInfo>> {
    let text = String::from_utf8_lossy(out);
    let mut commits = Vec::new();
    for record in text.split('\0').collect::<Vec<_>>().chunks(6) {
        if record.len() != 6 {
            break;
        }
        // Trailing record separator from -z leaves the next sha glued to the
        // previous body; log -z separates records with NUL so chunks(6) holds.
        commits.push(CommitInfo {
            sha: record[0].trim().to_string(),
            author_name: record[1].to_string(),
            author_email: record[2].to_string(),
            authored_at: record[3].to_string(),
            parents: record[4].split_whitespace().map(String::from).collect(),
            message: record[5].trim_end().to_string(),
        });
    }
    Ok(commits)
}

/// Branches (refs/heads) with tip shas.
pub async fn branches(cfg: &GitConfig, repo: &Path) -> Result<Vec<RefInfo>> {
    for_each_ref(cfg, repo, "refs/heads/").await
}

/// Tags (refs/tags) with peeled targets.
pub async fn tags(cfg: &GitConfig, repo: &Path) -> Result<Vec<RefInfo>> {
    for_each_ref(cfg, repo, "refs/tags/").await
}

async fn for_each_ref(cfg: &GitConfig, repo: &Path, prefix: &str) -> Result<Vec<RefInfo>> {
    let out = run_git_limited(
        cfg,
        &[
            "for-each-ref",
            "--format=%(refname:short)%00%(objectname)%00%(*objectname)",
            prefix,
        ],
        Some(repo),
        TREE_OUTPUT_LIMIT,
    )
    .await?;
    let text = String::from_utf8_lossy(&out);
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\0');
            Some(RefInfo {
                name: parts.next()?.to_string(),
                sha: parts.next()?.to_string(),
                target_sha: parts.next().filter(|s| !s.is_empty()).map(String::from),
            })
        })
        .collect())
}

/// Resolve any ref expression to a full sha (also validates existence).
pub async fn rev_parse(cfg: &GitConfig, repo: &Path, reference: &str) -> Result<String> {
    check_arg(reference)?;
    let out = run_git(
        cfg,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
        Some(repo),
    )
    .await
    .map_err(|_| Error::NotFound)?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn blob_limit_is_checked_before_content_is_returned() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let repo = std::env::temp_dir().join(format!("rgit-read-test-{nonce}"));
        std::fs::create_dir_all(&repo).expect("create repo dir");
        let config = GitConfig {
            bin: "git".into(),
            timeout_secs: 10,
            ..GitConfig::default()
        };
        crate::repo::run_git(&config, &["init", "--initial-branch=main"], Some(&repo))
            .await
            .expect("init repository");
        crate::repo::run_git(&config, &["config", "user.name", "Rgit Test"], Some(&repo))
            .await
            .expect("configure user name");
        crate::repo::run_git(
            &config,
            &["config", "user.email", "rgit@example.test"],
            Some(&repo),
        )
        .await
        .expect("configure user email");
        std::fs::write(repo.join("large.txt"), vec![b'x'; 4096]).expect("write blob");
        crate::repo::run_git(&config, &["add", "large.txt"], Some(&repo))
            .await
            .expect("git add");
        crate::repo::run_git(&config, &["commit", "-m", "large blob"], Some(&repo))
            .await
            .expect("git commit");

        assert!(matches!(
            read_blob(&config, &repo, "HEAD", "large.txt", 1024).await,
            Err(Error::Invalid(_))
        ));
        assert_eq!(
            read_blob(&config, &repo, "HEAD", "large.txt", 4096)
                .await
                .expect("read bounded blob")
                .len(),
            4096
        );

        std::fs::remove_dir_all(repo).expect("remove test repo");
    }
}
