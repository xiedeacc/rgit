//! Route tree (DESIGN.md §8-10).

use axum::extract::DefaultBodyLimit;
use axum::middleware as axum_mw;
use axum::routing::{delete, get, patch, post};
use axum::Router;
use rgit_core::state::AppState;

use crate::handlers::{api, git_http, lfs, web};
use crate::middleware::auth;

pub fn build(state: AppState) -> Router {
    let json_limit = state.config.http.max_json_body;

    let api_v1 = Router::new()
        // session / current user
        .route("/session", post(api::session::login).delete(api::session::logout))
        .route("/user", get(api::session::current_user).patch(api::user::update_profile))
        .route("/user/password", post(api::user::change_password))
        // ssh keys / tokens
        .route("/user/keys", get(api::keys::list).post(api::keys::create))
        .route("/user/keys/{id}", delete(api::keys::delete))
        .route("/user/tokens", get(api::tokens::list).post(api::tokens::create))
        .route("/user/tokens/{id}", delete(api::tokens::revoke))
        // projects
        .route("/projects", get(api::projects::list).post(api::projects::create))
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
        .route("/projects/{id}/members", get(api::members::list).post(api::members::add))
        .route("/projects/{id}/members/{user_id}", delete(api::members::remove))
        // repository browsing
        .route("/projects/{id}/repository/tree", get(api::repo_browse::tree))
        .route("/projects/{id}/repository/blob", get(api::repo_browse::blob))
        .route("/projects/{id}/repository/raw", get(api::repo_browse::raw))
        .route("/projects/{id}/repository/commits", get(api::repo_browse::commits))
        .route("/projects/{id}/repository/commits/{sha}", get(api::repo_browse::commit_detail))
        .route("/projects/{id}/repository/diff/{sha}", get(api::repo_browse::commit_diff))
        .route("/projects/{id}/repository/branches", get(api::repo_browse::branches))
        .route("/projects/{id}/repository/tags", get(api::repo_browse::tags))
        .route("/projects/{id}/repository/archive", get(api::repo_browse::archive))
        .route("/projects/{id}/repository/readme", get(api::repo_browse::readme))
        // groups
        .route("/groups", get(api::groups::list).post(api::groups::create))
        .route("/groups/{id}", delete(api::groups::delete))
        .route("/groups/{id}/members", post(api::groups::add_member))
        .route("/groups/{id}/members/{user_id}", delete(api::groups::remove_member))
        // admin
        .route("/admin/users", get(api::admin::list_users).post(api::admin::create_user))
        .route(
            "/admin/users/{id}",
            patch(api::admin::update_user).delete(api::admin::delete_user),
        )
        .route("/admin/projects", get(api::admin::list_projects))
        .route("/admin/stats", get(api::admin::stats))
        .layer(axum_mw::from_fn(auth::csrf_guard))
        .layer(DefaultBodyLimit::max(json_limit));

    // git smart HTTP + LFS live under /{ns}/{proj}.git/...
    // Body limits are disabled: pushes and LFS uploads stream to disk.
    let git = Router::new()
        .route("/{ns}/{proj}/info/refs", get(git_http::info_refs))
        .route("/{ns}/{proj}/{service}", post(git_http::service_rpc))
        .route("/{ns}/{proj}/info/lfs/objects/batch", post(lfs::batch))
        .route(
            "/{ns}/{proj}/info/lfs/objects/{oid}",
            get(lfs::download).put(lfs::upload),
        )
        .route("/{ns}/{proj}/info/lfs/verify", post(lfs::verify))
        .layer(DefaultBodyLimit::disable());

    Router::new()
        .nest("/api/v1", api_v1)
        .merge(git)
        .fallback(get(web::spa))
        .layer(axum_mw::from_fn_with_state(state.clone(), auth::resolve_identity))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state)
}
