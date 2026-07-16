//! Git LFS (DESIGN.md §9): standard batch API + basic transfer.
//!
//! POST /{ns}/{proj}.git/info/lfs/objects/batch
//! GET  /{ns}/{proj}.git/info/lfs/objects/{oid}
//! PUT  /{ns}/{proj}.git/info/lfs/objects/{oid}
//! POST /{ns}/{proj}.git/info/lfs/verify

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use rgit_core::auth::authorize_repo;
use rgit_core::auth::token::Scope;
use rgit_core::models::Project;
use rgit_core::perm::RepoAction;
use rgit_core::state::AppState;
use rgit_core::storage;
use rgit_core::Error;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

use super::helpers::find_project;
use crate::middleware::auth::Identity;

pub const LFS_CONTENT_TYPE: &str = "application/vnd.git-lfs+json";
static UPLOAD_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize)]
pub struct BatchRequest {
    pub operation: String, // "download" | "upload"
    #[serde(default)]
    pub objects: Vec<ObjectSpec>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ObjectSpec {
    pub oid: String,
    pub size: i64,
}

#[derive(Serialize)]
struct BatchResponse {
    transfer: &'static str,
    objects: Vec<BatchObject>,
}

#[derive(Serialize)]
struct BatchObject {
    oid: String,
    size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    authenticated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actions: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<serde_json::Value>,
}

fn lfs_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, LFS_CONTENT_TYPE)],
        serde_json::json!({ "message": message }).to_string(),
    )
        .into_response()
}

async fn resolve_lfs(
    state: &AppState,
    identity: &Identity,
    ns: &str,
    proj_segment: &str,
    action: RepoAction,
) -> Result<Project, Response> {
    if !state.config.lfs.enabled {
        return Err(lfs_error(StatusCode::NOT_FOUND, "LFS disabled"));
    }
    let proj_path = proj_segment
        .strip_suffix(".git")
        .ok_or_else(|| lfs_error(StatusCode::NOT_FOUND, "not found"))?;
    let (_, project) = find_project(state, ns, proj_path)
        .await
        .map_err(|_| lfs_error(StatusCode::NOT_FOUND, "not found"))?;
    if !project.lfs_enabled {
        return Err(lfs_error(StatusCode::NOT_FOUND, "LFS disabled for project"));
    }

    if let Some((project_id, can_write)) = identity.lfs_project() {
        if project_id != project.id || (action == RepoAction::Write && !can_write) {
            return Err(lfs_error(
                StatusCode::FORBIDDEN,
                "LFS token scope insufficient",
            ));
        }
    }

    let scope = match action {
        RepoAction::Write => Scope::WriteRepository,
        _ => Scope::ReadRepository,
    };
    if identity.user.is_some() && !identity.allows(scope) {
        return Err(lfs_error(StatusCode::FORBIDDEN, "token scope insufficient"));
    }

    match authorize_repo(&state.db, identity.user.as_ref(), &project, action).await {
        Ok(()) => Ok(project),
        Err(Error::Unauthorized) => Err((
            StatusCode::UNAUTHORIZED,
            [
                (header::WWW_AUTHENTICATE, "Basic realm=\"rgit\""),
                (header::CONTENT_TYPE, LFS_CONTENT_TYPE),
            ],
            serde_json::json!({"message": "authentication required"}).to_string(),
        )
            .into_response()),
        Err(_) => Err(lfs_error(StatusCode::FORBIDDEN, "access denied")),
    }
}

/// POST info/lfs/objects/batch
pub async fn batch(
    State(state): State<AppState>,
    Path((ns, proj)): Path<(String, String)>,
    identity: Identity,
    headers: axum::http::HeaderMap,
    Json(req): Json<BatchRequest>,
) -> Response {
    // git-lfs does not carry the repo credentials over to transfer hrefs;
    // echo the caller's Authorization into each action header (GitLab-style).
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let action = match req.operation.as_str() {
        "download" => RepoAction::Read,
        "upload" => RepoAction::Write,
        _ => return lfs_error(StatusCode::UNPROCESSABLE_ENTITY, "unknown operation"),
    };
    let project = match resolve_lfs(&state, &identity, &ns, &proj, action).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    let base = format!(
        "{}/{}/{}.git/info/lfs/objects",
        state.config.http.external_url.trim_end_matches('/'),
        ns,
        project.path
    );

    let mut objects = Vec::with_capacity(req.objects.len());
    for spec in &req.objects {
        if !storage::is_valid_lfs_oid(&spec.oid) || spec.size < 0 {
            objects.push(BatchObject {
                oid: spec.oid.clone(),
                size: spec.size,
                authenticated: None,
                actions: None,
                error: Some(serde_json::json!({"code": 422, "message": "invalid oid"})),
            });
            continue;
        }
        let obj = match action {
            RepoAction::Read => {
                batch_download_object(&state, &project, spec, &base, auth_header.as_deref()).await
            }
            _ => batch_upload_object(&state, &project, spec, &base, auth_header.as_deref()).await,
        };
        match obj {
            Ok(o) => objects.push(o),
            Err(e) => {
                tracing::error!(error = %e, oid = %spec.oid, "lfs batch item failed");
                return lfs_error(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
            }
        }
    }

    (
        [(header::CONTENT_TYPE, LFS_CONTENT_TYPE)],
        Json(BatchResponse {
            transfer: "basic",
            objects,
        }),
    )
        .into_response()
}

fn action_json(href: String, auth: Option<&str>) -> serde_json::Value {
    match auth {
        Some(a) => serde_json::json!({ "href": href, "header": { "Authorization": a } }),
        None => serde_json::json!({ "href": href }),
    }
}

async fn batch_download_object(
    state: &AppState,
    project: &Project,
    spec: &ObjectSpec,
    base: &str,
    auth: Option<&str>,
) -> anyhow::Result<BatchObject> {
    let linked: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT o.size FROM lfs_objects o
        JOIN project_lfs_objects pl ON pl.lfs_object_id = o.id
        WHERE pl.project_id = ?1 AND o.oid = ?2
        "#,
    )
    .bind(project.id)
    .bind(&spec.oid)
    .fetch_optional(&state.db)
    .await?;

    Ok(match linked {
        Some(size) => BatchObject {
            oid: spec.oid.clone(),
            size,
            authenticated: Some(true),
            actions: Some(serde_json::json!({
                "download": action_json(format!("{base}/{}", spec.oid), auth)
            })),
            error: None,
        },
        None => BatchObject {
            oid: spec.oid.clone(),
            size: spec.size,
            authenticated: None,
            actions: None,
            error: Some(serde_json::json!({"code": 404, "message": "object not found"})),
        },
    })
}

async fn batch_upload_object(
    state: &AppState,
    project: &Project,
    spec: &ObjectSpec,
    base: &str,
    auth: Option<&str>,
) -> anyhow::Result<BatchObject> {
    let max = state.config.lfs.max_file_size;
    if max > 0 && spec.size as u64 > max {
        return Ok(BatchObject {
            oid: spec.oid.clone(),
            size: spec.size,
            authenticated: None,
            actions: None,
            error: Some(serde_json::json!({"code": 422, "message": "object too large"})),
        });
    }

    let existing: Option<(i64, i64)> =
        sqlx::query_as("SELECT id, size FROM lfs_objects WHERE oid = ?1")
            .bind(&spec.oid)
            .fetch_optional(&state.db)
            .await?;

    if let Some((lfs_id, stored_size)) = existing {
        if stored_size != spec.size {
            return Ok(BatchObject {
                oid: spec.oid.clone(),
                size: spec.size,
                authenticated: None,
                actions: None,
                error: Some(serde_json::json!({"code": 422, "message": "size mismatch"})),
            });
        }
        // Global dedup: object already stored — just link it to the project.
        sqlx::query(
            "INSERT OR IGNORE INTO project_lfs_objects (project_id, lfs_object_id) VALUES (?1, ?2)",
        )
        .bind(project.id)
        .bind(lfs_id)
        .execute(&state.db)
        .await?;
        return Ok(BatchObject {
            oid: spec.oid.clone(),
            size: spec.size,
            authenticated: Some(true),
            actions: None, // no upload needed
            error: None,
        });
    }

    Ok(BatchObject {
        oid: spec.oid.clone(),
        size: spec.size,
        authenticated: Some(true),
        actions: Some(serde_json::json!({
            "upload": action_json(format!("{base}/{}", spec.oid), auth),
            "verify": action_json(format!("{}/verify", base.trim_end_matches("/objects")), auth),
        })),
        error: None,
    })
}

/// GET info/lfs/objects/{oid} — stream the object.
pub async fn download(
    State(state): State<AppState>,
    Path((ns, proj, oid)): Path<(String, String, String)>,
    identity: Identity,
) -> Response {
    let project = match resolve_lfs(&state, &identity, &ns, &proj, RepoAction::Read).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    if !storage::is_valid_lfs_oid(&oid) {
        return lfs_error(StatusCode::NOT_FOUND, "not found");
    }

    let size: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT o.size FROM lfs_objects o
        JOIN project_lfs_objects pl ON pl.lfs_object_id = o.id
        WHERE pl.project_id = ?1 AND o.oid = ?2
        "#,
    )
    .bind(project.id)
    .bind(&oid)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);
    let Some(size) = size else {
        return lfs_error(StatusCode::NOT_FOUND, "object not linked to project");
    };

    let path = storage::lfs_path(&state.config.storage, &oid);
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => {
            tracing::error!(oid, "lfs object in DB but missing on disk");
            return lfs_error(StatusCode::NOT_FOUND, "object missing");
        }
    };
    let operation = state.begin_operation();
    let stream = ReaderStream::new(file).map(move |item| {
        let _keep_alive = &operation;
        item
    });

    (
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CONTENT_LENGTH, size.to_string()),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

/// PUT info/lfs/objects/{oid} — streaming upload with digest verification.
pub async fn upload(
    State(state): State<AppState>,
    Path((ns, proj, oid)): Path<(String, String, String)>,
    identity: Identity,
    request: axum::extract::Request,
) -> Response {
    let project = match resolve_lfs(&state, &identity, &ns, &proj, RepoAction::Write).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    if !storage::is_valid_lfs_oid(&oid) {
        return lfs_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid oid");
    }

    let _operation = state.begin_operation();
    match store_streaming(&state, project.id, &oid, request).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(StoreError::DigestMismatch) => {
            lfs_error(StatusCode::UNPROCESSABLE_ENTITY, "oid/size mismatch")
        }
        Err(StoreError::TooLarge) => {
            lfs_error(StatusCode::UNPROCESSABLE_ENTITY, "object too large")
        }
        Err(StoreError::Other(e)) => {
            tracing::error!(error = %e, oid, "lfs upload failed");
            lfs_error(StatusCode::INTERNAL_SERVER_ERROR, "storage failure")
        }
    }
}

enum StoreError {
    DigestMismatch,
    TooLarge,
    Other(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for StoreError {
    fn from(e: E) -> Self {
        StoreError::Other(e.into())
    }
}

/// tmp file → hash while writing → fsync → verify → atomic rename → DB rows.
async fn store_streaming(
    state: &AppState,
    project_id: i64,
    oid: &str,
    request: axum::extract::Request,
) -> Result<(), StoreError> {
    let final_path = storage::lfs_path(&state.config.storage, oid);
    let tmp_dir = state.config.storage.lfs_objects.join("tmp");
    tokio::fs::create_dir_all(&tmp_dir).await?;
    let sequence = UPLOAD_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let tmp_path = tmp_dir.join(format!("{oid}.{}.{sequence}.part", std::process::id()));

    let mut file = tokio::fs::File::create(&tmp_path).await?;
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let max = state.config.lfs.max_file_size;

    let mut stream = request.into_body().into_data_stream();
    let result: Result<(), StoreError> = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(anyhow::Error::from)?;
            written += chunk.len() as u64;
            if max > 0 && written > max {
                return Err(StoreError::TooLarge);
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.sync_all().await?;
        if hex::encode(hasher.finalize_reset()) != oid {
            return Err(StoreError::DigestMismatch);
        }
        Ok(())
    }
    .await;

    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(e);
    }

    tokio::fs::create_dir_all(final_path.parent().expect("sharded path")).await?;
    tokio::fs::rename(&tmp_path, &final_path).await?;

    let mut tx = state.db.begin().await.map_err(anyhow::Error::from)?;
    sqlx::query("INSERT OR IGNORE INTO lfs_objects (oid, size) VALUES (?1, ?2)")
        .bind(oid)
        .bind(written as i64)
        .execute(&mut *tx)
        .await
        .map_err(anyhow::Error::from)?;
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO project_lfs_objects (project_id, lfs_object_id)
        SELECT ?1, id FROM lfs_objects WHERE oid = ?2
        "#,
    )
    .bind(project_id)
    .bind(oid)
    .execute(&mut *tx)
    .await
    .map_err(anyhow::Error::from)?;
    tx.commit().await.map_err(anyhow::Error::from)?;
    Ok(())
}

/// POST info/lfs/verify — confirm an uploaded object.
pub async fn verify(
    State(state): State<AppState>,
    Path((ns, proj)): Path<(String, String)>,
    identity: Identity,
    Json(spec): Json<ObjectSpec>,
) -> Response {
    let project = match resolve_lfs(&state, &identity, &ns, &proj, RepoAction::Write).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let size: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT o.size FROM lfs_objects o
        JOIN project_lfs_objects pl ON pl.lfs_object_id = o.id
        WHERE pl.project_id = ?1 AND o.oid = ?2
        "#,
    )
    .bind(project.id)
    .bind(&spec.oid)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    match size {
        Some(s) if s == spec.size => StatusCode::OK.into_response(),
        _ => lfs_error(StatusCode::NOT_FOUND, "object not found or size mismatch"),
    }
}
