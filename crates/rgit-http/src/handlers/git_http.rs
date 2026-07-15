//! git smart HTTP (DESIGN.md §8.1).
//!
//! GET  /{ns}/{proj}.git/info/refs?service=git-{upload,receive}-pack
//! POST /{ns}/{proj}.git/git-upload-pack
//! POST /{ns}/{proj}.git/git-receive-pack
//!
//! Bodies are streamed both directions; nothing is buffered in memory.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use rgit_core::auth::authorize_repo;
use rgit_core::auth::token::Scope;
use rgit_core::models::Project;
use rgit_core::perm::RepoAction;
use rgit_core::state::AppState;
use rgit_core::Error;
use rgit_git::protocol::{self, Service};
use serde::Deserialize;
use tokio_util::io::{ReaderStream, StreamReader};

use super::helpers::{find_project, repo_disk_path};
use crate::middleware::auth::Identity;

#[derive(Deserialize)]
pub struct InfoRefsQuery {
    service: Option<String>,
}

/// Strip the mandatory ".git" suffix from the URL project segment.
fn project_path_from_url(segment: &str) -> Result<&str, Error> {
    segment.strip_suffix(".git").ok_or(Error::NotFound)
}

/// 401 with a Basic challenge so git CLIs prompt for credentials.
fn basic_challenge() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"rgit\"")],
        "authentication required\n",
    )
        .into_response()
}

/// Resolve project + authorize the wire service. On failure, translate to
/// the git-client-friendly status codes (401 challenge / 404 opacity).
async fn resolve_and_authorize(
    state: &AppState,
    identity: &Identity,
    ns: &str,
    proj_segment: &str,
    service: Service,
) -> Result<Project, Response> {
    let proj_path = project_path_from_url(proj_segment)
        .map_err(|_| (StatusCode::NOT_FOUND, "not found\n").into_response())?;

    let (_, project) = find_project(state, ns, proj_path)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "not found\n").into_response())?;

    let (action, scope) = match service {
        Service::UploadPack | Service::UploadArchive => (RepoAction::Read, Scope::ReadRepository),
        Service::ReceivePack => (RepoAction::Write, Scope::WriteRepository),
    };

    // PAT scope check (only constrains token-authenticated callers).
    if identity.user.is_some() && !identity.allows(scope) {
        return Err((StatusCode::FORBIDDEN, "token scope insufficient\n").into_response());
    }

    match authorize_repo(&state.db, identity.user.as_ref(), &project, action).await {
        Ok(()) => Ok(project),
        Err(Error::Unauthorized) => Err(basic_challenge()),
        Err(Error::Forbidden) => {
            // Private repos stay invisible to authenticated-but-unauthorized
            // users; archived-write gets an explicit message.
            if action == RepoAction::Write && project.archived {
                Err((StatusCode::FORBIDDEN, "project is archived (read-only)\n").into_response())
            } else {
                Err((StatusCode::NOT_FOUND, "not found\n").into_response())
            }
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    }
}

/// GET info/refs — capability advertisement.
pub async fn info_refs(
    State(state): State<AppState>,
    Path((ns, proj)): Path<(String, String)>,
    Query(query): Query<InfoRefsQuery>,
    identity: Identity,
    headers: HeaderMap,
) -> Response {
    let Some(service) = query
        .service
        .as_deref()
        .and_then(Service::from_wire)
        .filter(|s| *s != Service::UploadArchive)
    else {
        // Dumb protocol is not served.
        return (StatusCode::FORBIDDEN, "smart HTTP only\n").into_response();
    };

    let project = match resolve_and_authorize(&state, &identity, &ns, &proj, service).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    let repo = repo_disk_path(&state, &project);
    let git_protocol = header_str(&headers, "Git-Protocol");
    let mut child = match protocol::spawn_advertise_refs(
        &state.config.git,
        service,
        &repo,
        git_protocol.as_deref(),
    ) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "advertise-refs spawn failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let stdout = child.stdout.take().expect("stdout piped");
    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    // pkt-line service header + flush + refs advertisement.
    let mut prelude = protocol::pkt_line(&format!("# service={}\n", service.name()));
    prelude.extend_from_slice(protocol::FLUSH_PKT);
    let body = Body::from_stream(
        futures::stream::once(async move { Ok::<_, std::io::Error>(bytes::Bytes::from(prelude)) })
            .chain(ReaderStream::new(stdout)),
    );

    (
        [
            (
                header::CONTENT_TYPE,
                format!("application/x-{}-advertisement", service.name()),
            ),
            (header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        body,
    )
        .into_response()
}

/// POST git-upload-pack / git-receive-pack — the actual data transfer.
pub async fn service_rpc(
    State(state): State<AppState>,
    Path((ns, proj, service_name)): Path<(String, String, String)>,
    identity: Identity,
    headers: HeaderMap,
    request: axum::extract::Request,
) -> Response {
    let Some(service) = Service::from_wire(&service_name)
        .filter(|s| matches!(s, Service::UploadPack | Service::ReceivePack))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let expected_type = format!("application/x-{}-request", service.name());
    if header_str(&headers, "content-type").as_deref() != Some(expected_type.as_str()) {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "bad content-type\n").into_response();
    }

    let project = match resolve_and_authorize(&state, &identity, &ns, &proj, service).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    let repo = repo_disk_path(&state, &project);
    let git_protocol = header_str(&headers, "Git-Protocol");
    let gzipped = header_str(&headers, "content-encoding").as_deref() == Some("gzip");

    let mut child = match protocol::spawn_stateless_rpc(
        &state.config.git,
        service,
        &repo,
        git_protocol.as_deref(),
    ) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "stateless-rpc spawn failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut stdin = child.stdin.take().expect("stdin piped");
    let stdout = child.stdout.take().expect("stdout piped");

    // Request body → (gunzip) → child stdin, streamed.
    let body_stream = request
        .into_body()
        .into_data_stream()
        .map(|r| r.map_err(std::io::Error::other));
    let is_receive = service == Service::ReceivePack;
    let state2 = state.clone();
    let project_id = project.id;
    tokio::spawn(async move {
        let mut reader = StreamReader::new(body_stream);
        let copy_result = if gzipped {
            let mut gunzip = async_compression::tokio::bufread::GzipDecoder::new(
                tokio::io::BufReader::new(&mut reader),
            );
            tokio::io::copy(&mut gunzip, &mut stdin).await
        } else {
            tokio::io::copy(&mut reader, &mut stdin).await
        };
        if let Err(e) = copy_result {
            tracing::debug!(error = %e, "git rpc body copy ended early");
        }
        drop(stdin); // EOF to git
        let status = child.wait().await;

        // Post-receive bookkeeping (DESIGN.md §8.1 step 4).
        if is_receive && matches!(&status, Ok(s) if s.success()) {
            if let Err(e) = post_receive(&state2, project_id).await {
                tracing::warn!(error = %e, project_id, "post-receive update failed");
            }
        }
    });

    (
        [
            (
                header::CONTENT_TYPE,
                format!("application/x-{}-result", service.name()),
            ),
            (header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        Body::from_stream(ReaderStream::new(stdout)),
    )
        .into_response()
}

/// After a successful push: refresh default_branch (first push) and updated_at.
async fn post_receive(state: &AppState, project_id: i64) -> anyhow::Result<()> {
    let project = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = ?1")
        .bind(project_id)
        .fetch_one(&state.db)
        .await?;
    let repo = repo_disk_path(state, &project);
    let head = rgit_git::repo::head_branch(&state.config.git, &repo).await?;
    sqlx::query("UPDATE projects SET default_branch = ?1, updated_at = datetime('now') WHERE id = ?2")
        .bind(head)
        .bind(project_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
}
