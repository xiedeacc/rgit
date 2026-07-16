//! Repository browsing API (DESIGN.md §10): tree/blob/raw/commits/refs/archive.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use rgit_core::auth::authorize_repo;
use rgit_core::perm::RepoAction;
use rgit_core::state::AppState;
use rgit_core::Error;
use serde::Deserialize;
use tokio_util::io::ReaderStream;

use super::projects::locate;
use super::{paginated_json, Pagination};
use crate::error::ApiResult;
use crate::handlers::helpers::repo_disk_path;
use crate::middleware::auth::Identity;

const BLOB_JSON_LIMIT: usize = 1024 * 1024; // 1 MiB for base64 JSON blobs
const RAW_LIMIT: usize = 64 * 1024 * 1024; // 64 MiB raw fetch guard

async fn readable_repo(
    state: &AppState,
    identity: &Identity,
    id: &str,
) -> Result<(rgit_core::models::Project, std::path::PathBuf), Error> {
    let project = locate(state, id).await?;
    authorize_repo(
        &state.db,
        identity.user.as_ref(),
        &project,
        RepoAction::Read,
    )
    .await?;
    let path = repo_disk_path(state, &project);
    Ok((project, path))
}

fn default_ref(project: &rgit_core::models::Project, r: Option<String>) -> String {
    r.filter(|s| !s.is_empty())
        .or_else(|| project.default_branch.clone())
        .unwrap_or_else(|| "HEAD".to_string())
}

#[derive(Deserialize)]
pub struct TreeQuery {
    pub r#ref: Option<String>,
    #[serde(default)]
    pub path: String,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

/// GET .../repository/tree
pub async fn tree(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
    Query(q): Query<TreeQuery>,
) -> ApiResult<Response> {
    let (project, repo) = readable_repo(&state, &identity, &id).await?;
    let _operation = state.begin_git_operation().await?;
    let reference = default_ref(&project, q.r#ref);
    let entries = rgit_git::read::list_tree(&state.config.git, &repo, &reference, &q.path).await?;
    let total = entries.len() as i64;
    let page = Pagination::from_options(q.page, q.per_page);
    let entries = entries
        .into_iter()
        .skip(page.offset() as usize)
        .take(page.limit() as usize)
        .collect::<Vec<_>>();
    Ok(paginated_json(entries, total))
}

/// GET .../repository/blob — metadata + base64 content (small files).
pub async fn blob(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
    Query(q): Query<TreeQuery>,
) -> ApiResult<Response> {
    let (project, repo) = readable_repo(&state, &identity, &id).await?;
    let _operation = state.begin_git_operation().await?;
    let reference = default_ref(&project, q.r#ref);
    let bytes = rgit_git::read::read_blob(
        &state.config.git,
        &repo,
        &reference,
        &q.path,
        BLOB_JSON_LIMIT,
    )
    .await?;
    let binary = bytes.contains(&0);
    Ok(Json(serde_json::json!({
        "path": q.path,
        "ref": reference,
        "size": bytes.len(),
        "binary": binary,
        "content_base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
    }))
    .into_response())
}

/// GET .../repository/raw — raw bytes, safe content type (DESIGN.md §7.5).
pub async fn raw(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
    Query(q): Query<TreeQuery>,
) -> ApiResult<Response> {
    let (project, repo) = readable_repo(&state, &identity, &id).await?;
    let _operation = state.begin_git_operation().await?;
    let reference = default_ref(&project, q.r#ref);
    let bytes =
        rgit_git::read::read_blob(&state.config.git, &repo, &reference, &q.path, RAW_LIMIT).await?;

    // Anti-XSS: never serve HTML-ish types from repo content.
    let guessed = mime_guess::from_path(&q.path).first_or_octet_stream();
    let content_type = if guessed.type_() == "text" || guessed.subtype() == "html" {
        "text/plain; charset=utf-8".to_string()
    } else {
        guessed.essence_str().to_string()
    };

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct CommitsQuery {
    pub r#ref: Option<String>,
    pub path: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

/// GET .../repository/commits
pub async fn commits(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
    Query(q): Query<CommitsQuery>,
) -> ApiResult<Response> {
    let (project, repo) = readable_repo(&state, &identity, &id).await?;
    let _operation = state.begin_git_operation().await?;
    let reference = default_ref(&project, q.r#ref);
    let page = Pagination::from_options(q.page, q.per_page);
    let total =
        rgit_git::read::count_commits(&state.config.git, &repo, &reference, q.path.as_deref())
            .await?;
    let commits = rgit_git::read::log(
        &state.config.git,
        &repo,
        &reference,
        q.path.as_deref(),
        page.offset() as u32,
        page.limit() as u32,
    )
    .await?;
    Ok(paginated_json(commits, total))
}

/// GET .../repository/commits/{sha}
pub async fn commit_detail(
    State(state): State<AppState>,
    identity: Identity,
    Path((id, sha)): Path<(String, String)>,
) -> ApiResult<Response> {
    let (_, repo) = readable_repo(&state, &identity, &id).await?;
    let _operation = state.begin_git_operation().await?;
    let commit = rgit_git::read::commit(&state.config.git, &repo, &sha).await?;
    Ok(Json(commit).into_response())
}

/// GET .../repository/diff/{sha}
pub async fn commit_diff(
    State(state): State<AppState>,
    identity: Identity,
    Path((id, sha)): Path<(String, String)>,
) -> ApiResult<Response> {
    let (_, repo) = readable_repo(&state, &identity, &id).await?;
    let _operation = state.begin_git_operation().await?;
    let diff = rgit_git::read::commit_diff(&state.config.git, &repo, &sha).await?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], diff).into_response())
}

/// GET .../repository/branches
pub async fn branches(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let (_, repo) = readable_repo(&state, &identity, &id).await?;
    let _operation = state.begin_git_operation().await?;
    Ok(Json(rgit_git::read::branches(&state.config.git, &repo).await?).into_response())
}

/// GET .../repository/tags
pub async fn tags(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let (_, repo) = readable_repo(&state, &identity, &id).await?;
    let _operation = state.begin_git_operation().await?;
    Ok(Json(rgit_git::read::tags(&state.config.git, &repo).await?).into_response())
}

#[derive(Deserialize)]
pub struct ArchiveQuery {
    pub r#ref: Option<String>,
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_format() -> String {
    "tar.gz".into()
}

/// GET .../repository/archive — streamed `git archive` (DESIGN.md §8.3).
pub async fn archive(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
    Query(q): Query<ArchiveQuery>,
) -> ApiResult<Response> {
    let (project, repo) = readable_repo(&state, &identity, &id).await?;
    let operation = state.begin_git_operation().await?;
    let (format, content_type) = match q.format.as_str() {
        "tar.gz" | "tgz" => ("tar.gz", "application/gzip"),
        "zip" => ("zip", "application/zip"),
        "tar" => ("tar", "application/x-tar"),
        _ => return Err(Error::invalid("format must be tar.gz, zip or tar").into()),
    };
    let reference = default_ref(&project, q.r#ref);
    // Validate the ref resolves before spawning the streaming child.
    let sha = rgit_git::read::rev_parse(&state.config.git, &repo, &reference).await?;

    let mut child = tokio::process::Command::new(&state.config.git.bin)
        .args([
            "-c",
            "filter.lfs.process=",
            "-c",
            "filter.lfs.smudge=",
            "-c",
            "filter.lfs.required=false",
        ])
        .arg(format!("--git-dir={}", repo.display()))
        .arg("archive")
        .arg(format!("--format={format}"))
        .arg(format!("--prefix={}-{}/", project.path, &sha[..8]))
        .arg(&sha)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Error::Git(format!("git archive spawn failed: {e}")))?;

    let stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");
    let git_config = state.config.git.clone();
    tokio::spawn(async move {
        let _operation = operation;
        let stderr_task = tokio::spawn(async move {
            let mut output = Vec::new();
            let _ = tokio::io::AsyncReadExt::read_to_end(&mut stderr, &mut output).await;
            output
        });
        let status = rgit_git::protocol::wait_with_timeout(&git_config, &mut child).await;
        let stderr = stderr_task.await.unwrap_or_default();
        if !matches!(&status, Ok(code) if code.success()) {
            tracing::warn!(
                ?status,
                stderr = %String::from_utf8_lossy(&stderr).trim(),
                "git archive failed"
            );
        }
    });

    let filename = format!("{}-{}.{format}", project.path, &sha[..8]);
    Ok((
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        Body::from_stream(ReaderStream::new(stdout)),
    )
        .into_response())
}

/// GET .../repository/readme — locate README at ref root.
pub async fn readme(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
    Query(q): Query<TreeQuery>,
) -> ApiResult<Response> {
    let (project, repo) = readable_repo(&state, &identity, &id).await?;
    let _operation = state.begin_git_operation().await?;
    let reference = default_ref(&project, q.r#ref);
    let entries = rgit_git::read::list_tree(&state.config.git, &repo, &reference, "").await?;
    let candidates = [
        "README.md",
        "README",
        "readme.md",
        "README.markdown",
        "README.txt",
    ];
    let Some(entry) = candidates
        .iter()
        .find_map(|c| entries.iter().find(|e| e.kind == "blob" && e.name == *c))
    else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    let bytes = rgit_git::read::read_blob(
        &state.config.git,
        &repo,
        &reference,
        &entry.path,
        BLOB_JSON_LIMIT,
    )
    .await?;
    Ok(Json(serde_json::json!({
        "path": entry.path,
        "content": String::from_utf8_lossy(&bytes),
    }))
    .into_response())
}
