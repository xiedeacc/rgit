//! Route tree (DESIGN.md §8-10).

use axum::extract::DefaultBodyLimit;
use axum::middleware as axum_mw;
use axum::routing::{delete, get, patch, post};
use axum::Router;
use rgit_core::state::AppState;

use crate::handlers::{api, git_http, lfs, web};
use crate::middleware::{auth, security};

pub fn build(state: AppState) -> Router {
    let json_limit = state.config.http.max_json_body;

    let api_v1 = Router::new()
        // public instance status
        .route("/status", get(api::status))
        // session / current user
        .route(
            "/session",
            post(api::session::login).delete(api::session::logout),
        )
        .route(
            "/user",
            get(api::session::current_user).patch(api::user::update_profile),
        )
        .route("/user/password", post(api::user::change_password))
        // ssh keys / tokens
        .route("/user/keys", get(api::keys::list).post(api::keys::create))
        .route("/user/keys/{id}", delete(api::keys::delete))
        .route(
            "/user/tokens",
            get(api::tokens::list).post(api::tokens::create),
        )
        .route("/user/tokens/{id}", delete(api::tokens::revoke))
        // projects
        .route(
            "/projects",
            get(api::projects::list).post(api::projects::create),
        )
        .route(
            "/projects/{id}",
            get(api::projects::get)
                .patch(api::projects::update)
                .delete(api::projects::delete),
        )
        .route("/projects/{id}/{flag}", post(api::projects::set_archived)) // archive|unarchive
        .route("/projects/{id}/fork", post(api::projects::fork))
        .route("/projects/{id}/transfer", post(api::projects::transfer))
        // members
        .route(
            "/projects/{id}/members",
            get(api::members::list).post(api::members::add),
        )
        .route(
            "/projects/{id}/members/{user_id}",
            delete(api::members::remove),
        )
        // repository browsing
        .route(
            "/projects/{id}/repository/tree",
            get(api::repo_browse::tree),
        )
        .route(
            "/projects/{id}/repository/blob",
            get(api::repo_browse::blob),
        )
        .route("/projects/{id}/repository/raw", get(api::repo_browse::raw))
        .route(
            "/projects/{id}/repository/commits",
            get(api::repo_browse::commits),
        )
        .route(
            "/projects/{id}/repository/commits/{sha}",
            get(api::repo_browse::commit_detail),
        )
        .route(
            "/projects/{id}/repository/diff/{sha}",
            get(api::repo_browse::commit_diff),
        )
        .route(
            "/projects/{id}/repository/branches",
            get(api::repo_browse::branches),
        )
        .route(
            "/projects/{id}/repository/tags",
            get(api::repo_browse::tags),
        )
        .route(
            "/projects/{id}/repository/archive",
            get(api::repo_browse::archive),
        )
        .route(
            "/projects/{id}/repository/readme",
            get(api::repo_browse::readme),
        )
        // groups
        .route("/groups", get(api::groups::list).post(api::groups::create))
        .route(
            "/groups/{id}",
            get(api::groups::get)
                .patch(api::groups::update)
                .delete(api::groups::delete),
        )
        .route(
            "/groups/{id}/members",
            get(api::groups::list_members).post(api::groups::add_member),
        )
        .route(
            "/groups/{id}/members/{user_id}",
            delete(api::groups::remove_member),
        )
        // admin
        .route(
            "/admin/users",
            get(api::admin::list_users).post(api::admin::create_user),
        )
        .route(
            "/admin/users/{id}",
            patch(api::admin::update_user).delete(api::admin::delete_user),
        )
        .route("/admin/projects", get(api::admin::list_projects))
        .route("/admin/stats", get(api::admin::stats))
        .fallback(api::not_found)
        .layer(axum_mw::from_fn(auth::api_guard))
        .layer(DefaultBodyLimit::max(json_limit));

    // git smart HTTP + LFS live under /{ns}/{proj}.git/...
    // Body limits are disabled: pushes and LFS uploads stream to disk.
    let git = Router::new()
        .route("/{ns}/{proj}/info/refs", get(git_http::info_refs))
        .route(
            "/{ns}/{proj}/git-upload-pack",
            post(git_http::upload_pack).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/{ns}/{proj}/git-receive-pack",
            post(git_http::receive_pack).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/{ns}/{proj}/info/lfs/objects/batch",
            post(lfs::batch).layer(DefaultBodyLimit::max(json_limit)),
        )
        .route(
            "/{ns}/{proj}/info/lfs/objects/{oid}",
            get(lfs::download)
                .put(lfs::upload)
                .layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/{ns}/{proj}/info/lfs/verify",
            post(lfs::verify).layer(DefaultBodyLimit::max(json_limit)),
        );

    Router::new()
        .nest("/api/v1", api_v1)
        .merge(git)
        .fallback(get(web::spa))
        .layer(axum_mw::from_fn_with_state(
            state.clone(),
            auth::resolve_identity,
        ))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(axum_mw::from_fn_with_state(
            state.clone(),
            security::add_security_headers,
        ))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::body::Body;
    use axum::http::{header, Method, Request, StatusCode};
    use rgit_core::auth::{session, token};
    use rgit_core::config::AppConfig;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::ServiceExt;

    async fn test_state() -> (AppState, PathBuf) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rgit-http-test-{nonce}"));
        let mut config = AppConfig::default();
        config.db.path = root.join("rgit.db");
        config.storage.repositories = root.join("repositories");
        config.storage.lfs_objects = root.join("lfs-objects");
        config.web.static_dir = root.join("web");
        config.http.external_url = "https://git.example.test".into();
        std::fs::create_dir_all(&config.web.static_dir).expect("create web dir");

        let db = rgit_core::db::connect(&config.db)
            .await
            .expect("connect sqlite");
        rgit_core::db::migrate(&db).await.expect("migrate sqlite");
        sqlx::query(
            r#"
            INSERT INTO users (id, username, email, name, password_hash, is_admin)
            VALUES (1, 'admin', 'admin@example.test', 'Admin', 'unused', 1)
            "#,
        )
        .execute(&db)
        .await
        .expect("insert admin");

        (AppState::from_parts(config, db), root)
    }

    async fn insert_token(state: &AppState, raw: &str, scopes: &str) {
        insert_token_for(state, 1, raw, scopes).await;
    }

    async fn insert_token_for(state: &AppState, user_id: i64, raw: &str, scopes: &str) {
        sqlx::query(
            r#"
            INSERT INTO personal_access_tokens (user_id, name, token_hash, scopes)
            VALUES (?1, 'test', ?2, ?3)
            "#,
        )
        .bind(user_id)
        .bind(token::hash_token(raw))
        .bind(scopes)
        .execute(&state.db)
        .await
        .expect("insert token");
    }

    fn bearer_request(method: Method, uri: &str, token: &str, body: Body) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .expect("request")
    }

    #[tokio::test]
    async fn repository_only_pat_cannot_access_rest_api() {
        let (state, root) = test_state().await;
        insert_token(&state, "rgit_repo_only", r#"["read_repository"]"#).await;

        let response = build(state.clone())
            .oneshot(bearer_request(
                Method::GET,
                "/api/v1/admin/stats",
                "rgit_repo_only",
                Body::empty(),
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        state.db.close().await;
        std::fs::remove_dir_all(root).expect("remove test dir");
    }

    #[tokio::test]
    async fn unknown_api_route_returns_json_404_instead_of_spa() {
        let (state, root) = test_state().await;
        insert_token(&state, "rgit_api_404", r#"["read_api"]"#).await;

        let response = build(state.clone())
            .oneshot(bearer_request(
                Method::GET,
                "/api/v1/does-not-exist",
                "rgit_api_404",
                Body::empty(),
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert_eq!(response.headers().get("x-frame-options").unwrap(), "DENY");
        assert_eq!(
            response.headers().get("strict-transport-security").unwrap(),
            "max-age=63072000"
        );
        assert!(response.headers().contains_key("content-security-policy"));

        state.db.close().await;
        std::fs::remove_dir_all(root).expect("remove test dir");
    }

    #[tokio::test]
    async fn status_reports_uptime_without_auth() {
        let (state, root) = test_state().await;

        let response = build(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/status")
                    .body(Body::empty())
                    .expect("status request"),
            )
            .await
            .expect("status response");
        assert_eq!(response.status(), StatusCode::OK);

        let body: serde_json::Value = serde_json::from_slice(
            &to_bytes(response.into_body(), 1024 * 1024)
                .await
                .expect("read status response"),
        )
        .expect("parse status response");
        assert!(body["uptime_seconds"].as_u64().is_some());

        state.db.close().await;
        std::fs::remove_dir_all(root).expect("remove test dir");
    }

    #[tokio::test]
    async fn pagination_totals_are_stable_and_match_filters() {
        let (state, root) = test_state().await;
        insert_token(&state, "rgit_page", r#"["read_api"]"#).await;
        sqlx::query(
            r#"
            INSERT INTO namespaces (id, path, name, kind, owner_user_id)
            VALUES (1, 'admin', 'Admin', 'user', 1);
            INSERT INTO projects
                (id, namespace_id, path, name, visibility, disk_id, disk_hash, updated_at)
            VALUES
                (1, 1, 'p1', 'Project 1', 20, 1, printf('%064d', 1), '2024-01-01 00:00:00'),
                (2, 1, 'p2', 'Project 2', 10, 2, printf('%064d', 2), '2024-01-03 00:00:00'),
                (3, 1, 'p3', 'Project 3', 20, 3, printf('%064d', 3), '2024-01-02 00:00:00'),
                (4, 1, 'p4', 'Project 4', 10, 4, printf('%064d', 4), '2024-01-03 00:00:00'),
                (5, 1, 'p5', 'Project 5', 0,  5, printf('%064d', 5), '2023-12-31 00:00:00');
            "#,
        )
        .execute(&state.db)
        .await
        .expect("insert paginated projects");

        for uri in [
            "/api/v1/projects?page=1&per_page=2",
            "/api/v1/projects?page=2&per_page=2",
            "/api/v1/admin/projects?page=2&per_page=2",
        ] {
            let response = build(state.clone())
                .oneshot(bearer_request(Method::GET, uri, "rgit_page", Body::empty()))
                .await
                .expect("paginated response");
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers().get("x-total").unwrap(), "5");
        }

        let ordered = build(state.clone())
            .oneshot(bearer_request(
                Method::GET,
                "/api/v1/projects?page=1&per_page=5",
                "rgit_page",
                Body::empty(),
            ))
            .await
            .expect("ordered project response");
        let body: serde_json::Value = serde_json::from_slice(
            &to_bytes(ordered.into_body(), 1024 * 1024)
                .await
                .expect("read ordered project response"),
        )
        .expect("parse ordered project response");
        let ids: Vec<i64> = body
            .as_array()
            .expect("project array")
            .iter()
            .map(|project| project["id"].as_i64().expect("project id"))
            .collect();
        assert_eq!(ids, vec![4, 2, 3, 1, 5]);

        let filtered = build(state.clone())
            .oneshot(bearer_request(
                Method::GET,
                "/api/v1/projects?visibility=10&page=1&per_page=1",
                "rgit_page",
                Body::empty(),
            ))
            .await
            .expect("filtered response");
        assert_eq!(filtered.status(), StatusCode::OK);
        assert_eq!(filtered.headers().get("x-total").unwrap(), "2");

        let users = build(state.clone())
            .oneshot(bearer_request(
                Method::GET,
                "/api/v1/admin/users?page=1&per_page=1",
                "rgit_page",
                Body::empty(),
            ))
            .await
            .expect("admin users response");
        assert_eq!(users.status(), StatusCode::OK);
        assert_eq!(users.headers().get("x-total").unwrap(), "1");

        state.db.close().await;
        std::fs::remove_dir_all(root).expect("remove test dir");
    }

    #[tokio::test]
    async fn project_create_and_fork_publish_repositories() {
        let (state, root) = test_state().await;
        sqlx::query(
            r#"
            INSERT INTO namespaces (id, path, name, kind, owner_user_id)
            VALUES (1, 'admin', 'Admin', 'user', 1)
            "#,
        )
        .execute(&state.db)
        .await
        .expect("insert namespace");
        insert_token(&state, "rgit_project_write", r#"["api"]"#).await;

        let created = build(state.clone())
            .oneshot(bearer_request(
                Method::POST,
                "/api/v1/projects",
                "rgit_project_write",
                Body::from(r#"{"name":"Demo","path":"demo"}"#),
            ))
            .await
            .expect("create response");
        assert_eq!(created.status(), StatusCode::CREATED);
        let created_json: serde_json::Value = serde_json::from_slice(
            &to_bytes(created.into_body(), 1024 * 1024)
                .await
                .expect("create body"),
        )
        .expect("create json");
        let project_id = created_json["id"].as_i64().expect("project id");
        let disk_hash = created_json["disk_hash"].as_str().expect("disk hash");
        assert!(rgit_core::storage::repo_path(&state.config.storage, disk_hash).is_dir());

        let forked = build(state.clone())
            .oneshot(bearer_request(
                Method::POST,
                &format!("/api/v1/projects/{project_id}/fork"),
                "rgit_project_write",
                Body::from(r#"{"path":"demo-fork","name":"Demo Fork"}"#),
            ))
            .await
            .expect("fork response");
        assert_eq!(forked.status(), StatusCode::CREATED);
        let forked_json: serde_json::Value = serde_json::from_slice(
            &to_bytes(forked.into_body(), 1024 * 1024)
                .await
                .expect("fork body"),
        )
        .expect("fork json");
        assert_eq!(forked_json["forked_from_project_id"], project_id);
        let fork_hash = forked_json["disk_hash"].as_str().expect("fork disk hash");
        assert!(rgit_core::storage::repo_path(&state.config.storage, fork_hash).is_dir());

        state.db.close().await;
        std::fs::remove_dir_all(root).expect("remove test dir");
    }

    #[tokio::test]
    async fn read_api_pat_is_read_only_and_api_pat_can_mutate() {
        let (state, root) = test_state().await;
        insert_token(&state, "rgit_read_api", r#"["read_api"]"#).await;
        insert_token(&state, "rgit_full_api", r#"["api"]"#).await;

        let read = build(state.clone())
            .oneshot(bearer_request(
                Method::GET,
                "/api/v1/admin/stats",
                "rgit_read_api",
                Body::empty(),
            ))
            .await
            .expect("read response");
        assert_eq!(read.status(), StatusCode::OK);

        let denied = build(state.clone())
            .oneshot(bearer_request(
                Method::PATCH,
                "/api/v1/user",
                "rgit_read_api",
                Body::from(r#"{"name":"Nope"}"#),
            ))
            .await
            .expect("denied response");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);

        let allowed = build(state.clone())
            .oneshot(bearer_request(
                Method::PATCH,
                "/api/v1/user",
                "rgit_full_api",
                Body::from(r#"{"name":"Updated"}"#),
            ))
            .await
            .expect("allowed response");
        assert_eq!(allowed.status(), StatusCode::OK);

        state.db.close().await;
        std::fs::remove_dir_all(root).expect("remove test dir");
    }

    #[tokio::test]
    async fn session_mutation_requires_exact_csrf_header() {
        let (state, root) = test_state().await;
        let raw_session = session::create_session(&state.db, 1, 1, None, None)
            .await
            .expect("create session");

        let request = |csrf: &str| {
            Request::builder()
                .method(Method::PATCH)
                .uri("/api/v1/user")
                .header(header::COOKIE, format!("rgit_session={raw_session}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header("X-Rgit-Csrf", csrf)
                .body(Body::from(r#"{"name":"Updated"}"#))
                .expect("request")
        };

        let denied = build(state.clone())
            .oneshot(request("yes"))
            .await
            .expect("denied response");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);

        let allowed = build(state.clone())
            .oneshot(request("1"))
            .await
            .expect("allowed response");
        assert_eq!(allowed.status(), StatusCode::OK);

        state.db.close().await;
        std::fs::remove_dir_all(root).expect("remove test dir");
    }

    #[tokio::test]
    async fn role_escalation_and_last_owner_removal_are_rejected() {
        let (state, root) = test_state().await;
        for (id, username) in [
            (2_i64, "owner"),
            (3, "maintainer"),
            (4, "project-owner"),
            (5, "group-owner"),
        ] {
            sqlx::query(
                r#"
                INSERT INTO users (id, username, email, name, password_hash)
                VALUES (?1, ?2, ?3, ?2, 'unused')
                "#,
            )
            .bind(id)
            .bind(username)
            .bind(format!("{username}@example.test"))
            .execute(&state.db)
            .await
            .expect("insert user");
        }
        sqlx::query(
            r#"
            INSERT INTO namespaces (id, path, name, kind, owner_user_id)
            VALUES (1, 'owner', 'Owner', 'user', 2),
                   (2, 'team', 'Team', 'group', NULL),
                   (3, 'owned-team', 'Owned Team', 'group', NULL)
            "#,
        )
        .execute(&state.db)
        .await
        .expect("insert namespaces");
        sqlx::query(
            r#"
            INSERT INTO projects
                (id, namespace_id, path, name, disk_id, disk_hash)
            VALUES (1, 1, 'demo', 'Demo', 1, ?1)
            "#,
        )
        .bind(rgit_core::storage::disk_hash(1))
        .execute(&state.db)
        .await
        .expect("insert project");
        sqlx::query(
            r#"
            INSERT INTO project_members (project_id, user_id, access_level)
            VALUES (1, 3, 40), (1, 4, 50)
            "#,
        )
        .execute(&state.db)
        .await
        .expect("insert project members");
        sqlx::query(
            "INSERT INTO group_members (namespace_id, user_id, access_level) VALUES (2, 2, 50), (3, 5, 50)",
        )
        .execute(&state.db)
        .await
        .expect("insert group owner");
        insert_token_for(&state, 2, "rgit_owner", r#"["api"]"#).await;
        insert_token_for(&state, 3, "rgit_maintainer", r#"["api"]"#).await;
        insert_token(&state, "rgit_admin", r#"["api"]"#).await;

        let group_members = build(state.clone())
            .oneshot(bearer_request(
                Method::GET,
                "/api/v1/groups/2/members",
                "rgit_owner",
                Body::empty(),
            ))
            .await
            .expect("group members response");
        assert_eq!(group_members.status(), StatusCode::OK);

        let hidden_group = build(state.clone())
            .oneshot(bearer_request(
                Method::GET,
                "/api/v1/groups/2",
                "rgit_maintainer",
                Body::empty(),
            ))
            .await
            .expect("hidden group response");
        assert_eq!(hidden_group.status(), StatusCode::NOT_FOUND);

        let update_group = build(state.clone())
            .oneshot(bearer_request(
                Method::PATCH,
                "/api/v1/groups/2",
                "rgit_owner",
                Body::from(r#"{"name":"Renamed Team"}"#),
            ))
            .await
            .expect("group update response");
        assert_eq!(update_group.status(), StatusCode::OK);

        let escalate = build(state.clone())
            .oneshot(bearer_request(
                Method::POST,
                "/api/v1/projects/1/members",
                "rgit_maintainer",
                Body::from(r#"{"user_id":3,"access_level":50}"#),
            ))
            .await
            .expect("escalation response");
        assert_eq!(escalate.status(), StatusCode::FORBIDDEN);

        let remove_higher = build(state.clone())
            .oneshot(bearer_request(
                Method::DELETE,
                "/api/v1/projects/1/members/4",
                "rgit_maintainer",
                Body::empty(),
            ))
            .await
            .expect("member removal response");
        assert_eq!(remove_higher.status(), StatusCode::FORBIDDEN);

        let remove_last_owner = build(state.clone())
            .oneshot(bearer_request(
                Method::DELETE,
                "/api/v1/groups/2/members/2",
                "rgit_owner",
                Body::empty(),
            ))
            .await
            .expect("group owner removal response");
        assert_eq!(remove_last_owner.status(), StatusCode::CONFLICT);

        let add_member_to_user_namespace = build(state.clone())
            .oneshot(bearer_request(
                Method::POST,
                "/api/v1/groups/1/members",
                "rgit_admin",
                Body::from(r#"{"user_id":3,"access_level":30}"#),
            ))
            .await
            .expect("user namespace membership response");
        assert_eq!(add_member_to_user_namespace.status(), StatusCode::NOT_FOUND);

        let demote_last_admin = build(state.clone())
            .oneshot(bearer_request(
                Method::PATCH,
                "/api/v1/admin/users/1",
                "rgit_admin",
                Body::from(r#"{"is_admin":false}"#),
            ))
            .await
            .expect("admin demotion response");
        assert_eq!(demote_last_admin.status(), StatusCode::CONFLICT);

        let delete_sole_group_owner = build(state.clone())
            .oneshot(bearer_request(
                Method::DELETE,
                "/api/v1/admin/users/5",
                "rgit_admin",
                Body::empty(),
            ))
            .await
            .expect("sole group owner deletion response");
        assert_eq!(delete_sole_group_owner.status(), StatusCode::CONFLICT);

        state.db.close().await;
        std::fs::remove_dir_all(root).expect("remove test dir");
    }
}
