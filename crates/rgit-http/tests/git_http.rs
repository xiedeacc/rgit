use rgit_core::auth::token;
use rgit_core::config::AppConfig;
use rgit_core::state::AppState;
use rgit_core::storage;
use std::future::IntoFuture;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;

fn temp_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!("rgit-git-http-test-{nonce}"))
}

async fn git(cwd: Option<&Path>, args: &[&str]) -> Output {
    let mut command = Command::new("git");
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
    let output = command.output().await.expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

async fn insert_project(state: &AppState, id: i64, path: &str) {
    let disk_hash = storage::disk_hash(id);
    sqlx::query(
        r#"
        INSERT INTO projects
            (id, namespace_id, path, name, visibility, disk_id, disk_hash)
        VALUES (?1, 1, ?2, ?2, 20, ?1, ?3)
        "#,
    )
    .bind(id)
    .bind(path)
    .bind(&disk_hash)
    .execute(&state.db)
    .await
    .expect("insert project");
    let repo = storage::repo_path(&state.config.storage, &disk_hash);
    rgit_git::repo::init_bare(&state.config.git, &repo, "main")
        .await
        .expect("init bare repo");
}

#[tokio::test]
async fn smart_http_supports_push_fetch_shallow_and_submodules() {
    let root = temp_root();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let addr = listener.local_addr().expect("listener address");

    let mut config = AppConfig::default();
    config.http.external_url = format!("http://{addr}");
    config.db.path = root.join("rgit.db");
    config.storage.repositories = root.join("repositories");
    config.storage.lfs_objects = root.join("lfs-objects");
    config.web.static_dir = root.join("web");
    config.git.timeout_secs = 30;
    std::fs::create_dir_all(&config.web.static_dir).expect("create web dir");

    let db = rgit_core::db::connect(&config.db)
        .await
        .expect("connect sqlite");
    rgit_core::db::migrate(&db).await.expect("migrate sqlite");
    let state = AppState::from_parts(config, db);
    let password_hash = rgit_core::auth::password::hash_password("correct-password", 4)
        .expect("hash test password");
    sqlx::query(
        r#"
        INSERT INTO users (id, username, email, name, password_hash)
        VALUES (1, 'alice', 'alice@example.test', 'Alice', ?1)
        "#,
    )
    .bind(password_hash)
    .execute(&state.db)
    .await
    .expect("insert user");
    sqlx::query(
        r#"
        INSERT INTO namespaces (id, path, name, kind, owner_user_id)
        VALUES (1, 'alice', 'Alice', 'user', 1)
        "#,
    )
    .execute(&state.db)
    .await
    .expect("insert namespace");
    let raw_token = "rgit_http_integration_token";
    sqlx::query(
        r#"
        INSERT INTO personal_access_tokens (user_id, name, token_hash, scopes)
        VALUES (1, 'git test', ?1, '["write_repository"]')
        "#,
    )
    .bind(token::hash_token(raw_token))
    .execute(&state.db)
    .await
    .expect("insert token");
    insert_project(&state, 1, "demo").await;
    insert_project(&state, 2, "library").await;

    let server = tokio::spawn(
        axum::serve(
            listener,
            rgit_http::router::build(state.clone())
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );

    let public_demo = format!("http://{addr}/alice/demo.git");
    let public_library = format!("http://{addr}/alice/library.git");
    let write_demo = format!("http://alice:{raw_token}@{addr}/alice/demo.git");
    let write_library = format!("http://alice:{raw_token}@{addr}/alice/library.git");
    let auto_created = format!("http://alice:{raw_token}@{addr}/auto-team/auto-demo.git");

    let auto_work = root.join("auto-work");
    std::fs::create_dir_all(&auto_work).expect("create auto worktree");
    git(Some(&auto_work), &["init", "--initial-branch=main"]).await;
    tokio::fs::write(auto_work.join("README.md"), b"auto-created\n")
        .await
        .expect("write auto readme");
    git(Some(&auto_work), &["add", "README.md"]).await;
    git(Some(&auto_work), &["commit", "-m", "initial push"]).await;
    git(
        Some(&auto_work),
        &["remote", "add", "origin", &auto_created],
    )
    .await;
    git(Some(&auto_work), &["push", "origin", "main"]).await;
    let auto_project: (i64, String) = sqlx::query_as(
        r#"
        SELECT p.id, p.disk_hash FROM projects p
        JOIN namespaces n ON n.id = p.namespace_id
        WHERE n.path = 'auto-team' AND n.kind = 'group' AND p.path = 'auto-demo'
        "#,
    )
    .fetch_one(&state.db)
    .await
    .expect("auto-created project");
    assert!(storage::repo_path(&state.config.storage, &auto_project.1).is_dir());
    let auto_owner: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM group_members gm
        JOIN namespaces n ON n.id = gm.namespace_id
        WHERE n.path = 'auto-team' AND gm.user_id = 1 AND gm.access_level = 50
        "#,
    )
    .fetch_one(&state.db)
    .await
    .expect("auto-created group owner");
    assert_eq!(auto_owner, 1);

    let library_work = root.join("library-work");
    git(
        None,
        &["clone", &write_library, library_work.to_str().unwrap()],
    )
    .await;
    tokio::fs::write(library_work.join("lib.txt"), b"library v1\n")
        .await
        .expect("write library file");
    git(Some(&library_work), &["add", "lib.txt"]).await;
    git(Some(&library_work), &["commit", "-m", "library v1"]).await;
    git(Some(&library_work), &["push", "origin", "main"]).await;

    let demo_work = root.join("demo-work");
    git(None, &["clone", &write_demo, demo_work.to_str().unwrap()]).await;
    tokio::fs::write(demo_work.join("README.md"), b"demo v1\n")
        .await
        .expect("write readme");
    git(Some(&demo_work), &["add", "README.md"]).await;
    git(Some(&demo_work), &["commit", "-m", "demo v1"]).await;
    git(
        Some(&demo_work),
        &["submodule", "add", &public_library, "deps/library"],
    )
    .await;
    git(Some(&demo_work), &["commit", "-am", "add submodule"]).await;
    git(Some(&demo_work), &["push", "origin", "main"]).await;
    let password_url = format!("http://alice:correct-password@{addr}/alice/demo.git");
    git(None, &["ls-remote", &password_url, "refs/heads/main"]).await;

    git(Some(&demo_work), &["lfs", "install", "--local"]).await;
    git(Some(&demo_work), &["lfs", "track", "*.bin"]).await;
    let lfs_payload: Vec<u8> = (0..(2 * 1024 * 1024))
        .map(|index| (index % 251) as u8)
        .collect();
    tokio::fs::write(demo_work.join("payload.bin"), &lfs_payload)
        .await
        .expect("write lfs payload");
    git(Some(&demo_work), &["add", ".gitattributes", "payload.bin"]).await;
    git(Some(&demo_work), &["commit", "-m", "add lfs payload"]).await;
    git(Some(&demo_work), &["push", "origin", "main"]).await;

    let recursive = root.join("recursive");
    git(
        None,
        &[
            "clone",
            "--recurse-submodules",
            &public_demo,
            recursive.to_str().unwrap(),
        ],
    )
    .await;
    assert_eq!(
        tokio::fs::read_to_string(recursive.join("deps/library/lib.txt"))
            .await
            .expect("read submodule file"),
        "library v1\n"
    );
    assert_eq!(
        tokio::fs::read(recursive.join("payload.bin"))
            .await
            .expect("read downloaded lfs payload"),
        lfs_payload
    );
    let lfs_rows: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM project_lfs_objects pl
        JOIN lfs_objects o ON o.id = pl.lfs_object_id
        WHERE pl.project_id = 1 AND o.size = ?1
        "#,
    )
    .bind(lfs_payload.len() as i64)
    .fetch_one(&state.db)
    .await
    .expect("query lfs link");
    assert_eq!(lfs_rows, 1);

    tokio::fs::write(demo_work.join("README.md"), b"demo v2\n")
        .await
        .expect("update readme");
    git(Some(&demo_work), &["commit", "-am", "demo v2"]).await;
    git(Some(&demo_work), &["push", "origin", "main"]).await;
    git(Some(&recursive), &["fetch", "origin"]).await;

    let shallow = root.join("shallow");
    git(
        None,
        &[
            "clone",
            "--depth",
            "1",
            &public_demo,
            shallow.to_str().unwrap(),
        ],
    )
    .await;
    let count = git(Some(&shallow), &["rev-list", "--count", "HEAD"]).await;
    assert_eq!(String::from_utf8_lossy(&count.stdout).trim(), "1");

    server.abort();
    let _ = server.await;
    state.db.close().await;
    std::fs::remove_dir_all(root).expect("remove test dir");
}
