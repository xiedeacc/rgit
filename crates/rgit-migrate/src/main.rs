//! rgit-migrate — GitLab (omnibus, PostgreSQL 17.11.x) → rgit (SQLite).
//!
//! Reads only the tables/columns listed in DESIGN.md §13.2, writes the rgit
//! SQLite database, copies repositories + LFS objects, normalizes
//! alternates/pool repos, and emits a verification report. The GitLab
//! installation is never modified. See docs/MIGRATION.md for the runbook.

use anyhow::{bail, Context};
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row, SqlitePool};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "rgit-migrate", about = "Migrate GitLab (PostgreSQL) to rgit (SQLite)")]
struct Args {
    /// GitLab PostgreSQL URL, e.g.
    /// postgres://gitlab-psql@/gitlabhq_production?host=/var/opt/gitlab/postgresql
    #[arg(long)]
    pg: String,

    /// GitLab repositories root (omnibus default shown).
    #[arg(long, default_value = "/var/opt/gitlab/git-data/repositories")]
    gitlab_repos: PathBuf,

    /// GitLab LFS objects root (omnibus default shown).
    #[arg(long, default_value = "/var/opt/gitlab/gitlab-rails/shared/lfs-objects")]
    gitlab_lfs: PathBuf,

    /// rgit data directory to create (…/data with rgit.db, repositories/, lfs-objects/).
    #[arg(long)]
    out: PathBuf,

    /// Read + report only; write nothing.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Default, serde::Serialize)]
struct Report {
    users: u64,
    namespaces: u64,
    projects: u64,
    project_members: u64,
    group_members: u64,
    ssh_keys: u64,
    lfs_objects: u64,
    lfs_bytes: u64,
    repos_copied: u64,
    repos_repacked: u64,
    errors: Vec<String>,
    warnings: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let args = Args::parse();

    let pg = PgPoolOptions::new()
        .max_connections(4)
        .connect(&args.pg)
        .await
        .context("connecting to GitLab PostgreSQL")?;

    let mut report = Report::default();

    // ---- Phase 1: read GitLab tables (read-only) --------------------------
    let users = read_users(&pg).await?;
    let namespaces = read_namespaces(&pg, &mut report).await?;
    let projects = read_projects(&pg, &mut report).await?;
    let members = read_members(&pg).await?;
    let keys = read_keys(&pg).await?;
    let lfs = read_lfs(&pg, &mut report).await?;
    let forks: Vec<(i64, i64)> = sqlx::query(
        "SELECT project_id, forked_from_project_id FROM fork_network_members
         WHERE forked_from_project_id IS NOT NULL",
    )
    .fetch_all(&pg)
    .await?
    .into_iter()
    .map(|r| (r.get::<i64, _>(0), r.get::<i64, _>(1)))
    .collect();

    tracing::info!(
        users = users.len(),
        namespaces = namespaces.len(),
        projects = projects.len(),
        "GitLab data loaded"
    );

    if args.dry_run {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    // ---- Phase 2: write SQLite -------------------------------------------
    std::fs::create_dir_all(&args.out)?;
    let db_path = args.out.join("rgit.db");
    if db_path.exists() {
        bail!("{} already exists — refusing to overwrite", db_path.display());
    }
    let sqlite = rgit_core::db::connect(&rgit_core::config::DbConfig { path: db_path.clone() }).await?;
    rgit_core::db::migrate(&sqlite).await?;

    write_sqlite(&sqlite, &users, &namespaces, &projects, &members, &keys, &lfs, &forks, &mut report)
        .await?;

    // ---- Phase 3: copy repositories + LFS objects -------------------------
    copy_repositories(&args, &projects, &mut report).await?;
    copy_lfs_objects(&args, &lfs.objects, &mut report).await?;

    // ---- Phase 4: verify ---------------------------------------------------
    verify(&args, &sqlite, &mut report).await?;

    let report_path = args.out.join("migration-report.json");
    std::fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;
    println!("report written to {}", report_path.display());

    if !report.errors.is_empty() {
        bail!("{} error(s) — see report; data may be incomplete", report.errors.len());
    }
    println!("migration completed with no errors");
    Ok(())
}

// ---------------------------------------------------------------------------

struct GlUser {
    id: i64,
    username: String,
    email: String,
    name: String,
    encrypted_password: String,
    admin: bool,
    state: String,
}

async fn read_users(pg: &PgPool) -> anyhow::Result<Vec<GlUser>> {
    // user_type = 0: humans only (no bots/service accounts).
    let rows = sqlx::query(
        r#"SELECT id, username, email, COALESCE(name,''), encrypted_password,
                  admin, COALESCE(state,'active')
           FROM users WHERE user_type = 0 AND username IS NOT NULL ORDER BY id"#,
    )
    .fetch_all(pg)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| GlUser {
            id: r.get(0),
            username: r.get(1),
            email: r.get(2),
            name: r.get(3),
            encrypted_password: r.get(4),
            admin: r.get(5),
            state: r.get(6),
        })
        .collect())
}

struct GlNamespace {
    id: i64,
    name: String,
    path: String,
    kind: String, // "user" | "group"
    owner_id: Option<i64>,
}

async fn read_namespaces(pg: &PgPool, report: &mut Report) -> anyhow::Result<Vec<GlNamespace>> {
    // Only root-level User/Group namespaces; rgit is flat (DESIGN.md §13.2).
    let nested: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM namespaces WHERE type = 'Group' AND parent_id IS NOT NULL",
    )
    .fetch_one(pg)
    .await?;
    if nested > 0 {
        report.errors.push(format!(
            "{nested} nested subgroup(s) found — flatten them in GitLab first (rgit groups are flat)"
        ));
    }

    let rows = sqlx::query(
        r#"SELECT id, name, path, type, owner_id FROM namespaces
           WHERE type IN ('User','Group') AND parent_id IS NULL ORDER BY id"#,
    )
    .fetch_all(pg)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| GlNamespace {
            id: r.get(0),
            name: r.get(1),
            path: r.get(2),
            kind: if r.get::<String, _>(3) == "User" { "user".into() } else { "group".into() },
            owner_id: r.get(4),
        })
        .collect())
}

struct GlProject {
    id: i64,
    name: String,
    path: String,
    description: String,
    namespace_id: i64,
    visibility_level: i32,
    archived: bool,
    lfs_enabled: bool,
}

async fn read_projects(pg: &PgPool, report: &mut Report) -> anyhow::Result<Vec<GlProject>> {
    let legacy: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM projects WHERE COALESCE(storage_version, 0) < 2",
    )
    .fetch_one(pg)
    .await?;
    if legacy > 0 {
        report.errors.push(format!(
            "{legacy} project(s) on legacy storage — run GitLab's hashed-storage migration first"
        ));
    }

    let rows = sqlx::query(
        r#"SELECT id, COALESCE(name,path), path, COALESCE(description,''),
                  namespace_id, visibility_level, archived, COALESCE(lfs_enabled, true)
           FROM projects WHERE pending_delete = false OR pending_delete IS NULL
           ORDER BY id"#,
    )
    .fetch_all(pg)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| GlProject {
            id: r.get(0),
            name: r.get(1),
            path: r.get(2),
            description: r.get(3),
            namespace_id: r.get(4),
            visibility_level: r.get(5),
            archived: r.get(6),
            lfs_enabled: r.get(7),
        })
        .collect())
}

struct GlMember {
    source_type: String, // "Project" | "Namespace"
    source_id: i64,
    user_id: i64,
    access_level: i32,
}

async fn read_members(pg: &PgPool) -> anyhow::Result<Vec<GlMember>> {
    // Real memberships only: user present, not an unaccepted invite/request.
    let rows = sqlx::query(
        r#"SELECT source_type, source_id, user_id, access_level FROM members
           WHERE user_id IS NOT NULL AND requested_at IS NULL AND invite_token IS NULL"#,
    )
    .fetch_all(pg)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| GlMember {
            source_type: r.get(0),
            source_id: r.get(1),
            user_id: r.get(2),
            access_level: r.get(3),
        })
        .collect())
}

struct GlKey {
    user_id: i64,
    title: String,
    key: String,
    /// bytea in PG → unpadded base64 here (GitLab Ruby-level format).
    fingerprint_sha256: String,
}

async fn read_keys(pg: &PgPool) -> anyhow::Result<Vec<GlKey>> {
    use base64::Engine;
    let rows = sqlx::query(
        r#"SELECT user_id, COALESCE(title,''), key, fingerprint_sha256
           FROM keys WHERE type = 'Key' AND user_id IS NOT NULL
             AND key IS NOT NULL AND fingerprint_sha256 IS NOT NULL"#,
    )
    .fetch_all(pg)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| GlKey {
            user_id: r.get(0),
            title: r.get(1),
            key: r.get(2),
            fingerprint_sha256: base64::engine::general_purpose::STANDARD_NO_PAD
                .encode(r.get::<Vec<u8>, _>(3)),
        })
        .collect())
}

#[derive(Default)]
struct GlLfs {
    /// oid → size
    objects: Vec<(String, i64)>,
    /// (project_id, oid)
    links: Vec<(i64, String)>,
}

async fn read_lfs(pg: &PgPool, report: &mut Report) -> anyhow::Result<GlLfs> {
    let remote: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM lfs_objects WHERE file_store <> 1")
            .fetch_one(pg)
            .await?;
    if remote > 0 {
        report.errors.push(format!(
            "{remote} LFS object(s) in object storage — run `gitlab-rake gitlab:lfs:migrate_to_local` first"
        ));
    }

    let objects = sqlx::query("SELECT oid, size FROM lfs_objects ORDER BY id")
        .fetch_all(pg)
        .await?
        .into_iter()
        .map(|r| (r.get::<String, _>(0), r.get::<i64, _>(1)))
        .collect();
    let links = sqlx::query(
        r#"SELECT DISTINCT lp.project_id, o.oid
           FROM lfs_objects_projects lp JOIN lfs_objects o ON o.id = lp.lfs_object_id"#,
    )
    .fetch_all(pg)
    .await?
    .into_iter()
    .map(|r| (r.get::<i64, _>(0), r.get::<String, _>(1)))
    .collect();
    Ok(GlLfs { objects, links })
}

// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn write_sqlite(
    db: &SqlitePool,
    users: &[GlUser],
    namespaces: &[GlNamespace],
    projects: &[GlProject],
    members: &[GlMember],
    keys: &[GlKey],
    lfs: &GlLfs,
    forks: &[(i64, i64)],
    report: &mut Report,
) -> anyhow::Result<()> {
    let mut tx = db.begin().await?;

    for u in users {
        // Non-active states collapse to 'blocked' (DESIGN.md §13.2).
        let state = if u.state == "active" { "active" } else { "blocked" };
        sqlx::query(
            r#"INSERT INTO users (id, username, email, name, password_hash, is_admin, state)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
        )
        .bind(u.id)
        .bind(&u.username)
        .bind(&u.email)
        .bind(&u.name)
        .bind(&u.encrypted_password) // bcrypt, verified as-is by rgit
        .bind(u.admin)
        .bind(state)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("user {}", u.username))?;
        report.users += 1;
    }

    let user_ids: HashMap<i64, ()> = users.iter().map(|u| (u.id, ())).collect();
    for n in namespaces {
        if n.kind == "user" && n.owner_id.map(|o| !user_ids.contains_key(&o)).unwrap_or(true) {
            // Namespace of a bot/ghost user we skipped.
            continue;
        }
        sqlx::query(
            r#"INSERT INTO namespaces (id, path, name, kind, owner_user_id)
               VALUES (?1, ?2, ?3, ?4, ?5)"#,
        )
        .bind(n.id)
        .bind(&n.path)
        .bind(&n.name)
        .bind(&n.kind)
        .bind(if n.kind == "user" { n.owner_id } else { None })
        .execute(&mut *tx)
        .await
        .with_context(|| format!("namespace {}", n.path))?;
        report.namespaces += 1;
    }

    let fork_map: HashMap<i64, i64> = forks.iter().copied().collect();
    for p in projects {
        // disk_id = GitLab project id keeps @hashed paths valid (DESIGN.md §5).
        sqlx::query(
            r#"INSERT INTO projects (id, namespace_id, path, name, description, visibility,
                                     archived, lfs_enabled, disk_id, disk_hash, forked_from_project_id)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"#,
        )
        .bind(p.id)
        .bind(p.namespace_id)
        .bind(&p.path)
        .bind(&p.name)
        .bind(&p.description)
        .bind(p.visibility_level)
        .bind(p.archived)
        .bind(p.lfs_enabled)
        .bind(p.id)
        .bind(rgit_core::storage::disk_hash(p.id))
        .bind(fork_map.get(&p.id))
        .execute(&mut *tx)
        .await
        .with_context(|| format!("project {}", p.path))?;
        report.projects += 1;
    }

    for m in members {
        if !user_ids.contains_key(&m.user_id) {
            continue;
        }
        let res = match m.source_type.as_str() {
            "Project" => {
                sqlx::query(
                    r#"INSERT OR IGNORE INTO project_members (project_id, user_id, access_level)
                       SELECT ?1, ?2, ?3 WHERE EXISTS (SELECT 1 FROM projects WHERE id = ?1)"#,
                )
                .bind(m.source_id)
                .bind(m.user_id)
                .bind(m.access_level)
                .execute(&mut *tx)
                .await
            }
            "Namespace" => {
                sqlx::query(
                    r#"INSERT OR IGNORE INTO group_members (namespace_id, user_id, access_level)
                       SELECT ?1, ?2, ?3 WHERE EXISTS
                           (SELECT 1 FROM namespaces WHERE id = ?1 AND kind = 'group')"#,
                )
                .bind(m.source_id)
                .bind(m.user_id)
                .bind(m.access_level)
                .execute(&mut *tx)
                .await
            }
            _ => continue,
        }?;
        if res.rows_affected() > 0 {
            if m.source_type == "Project" {
                report.project_members += 1;
            } else {
                report.group_members += 1;
            }
        }
    }

    for k in keys {
        if !user_ids.contains_key(&k.user_id) {
            continue;
        }
        let res = sqlx::query(
            r#"INSERT OR IGNORE INTO ssh_keys (user_id, title, key, fingerprint_sha256)
               VALUES (?1, ?2, ?3, ?4)"#,
        )
        .bind(k.user_id)
        .bind(&k.title)
        .bind(&k.key)
        .bind(&k.fingerprint_sha256)
        .execute(&mut *tx)
        .await?;
        report.ssh_keys += res.rows_affected();
    }

    for (oid, size) in &lfs.objects {
        sqlx::query("INSERT OR IGNORE INTO lfs_objects (oid, size) VALUES (?1, ?2)")
            .bind(oid)
            .bind(size)
            .execute(&mut *tx)
            .await?;
        report.lfs_objects += 1;
        report.lfs_bytes += *size as u64;
    }
    for (project_id, oid) in &lfs.links {
        sqlx::query(
            r#"INSERT OR IGNORE INTO project_lfs_objects (project_id, lfs_object_id)
               SELECT ?1, id FROM lfs_objects WHERE oid = ?2
                 AND EXISTS (SELECT 1 FROM projects WHERE id = ?1)"#,
        )
        .bind(project_id)
        .bind(oid)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------

async fn copy_repositories(
    args: &Args,
    projects: &[GlProject],
    report: &mut Report,
) -> anyhow::Result<()> {
    let dst_root = args.out.join("repositories");
    for p in projects {
        let hash = rgit_core::storage::disk_hash(p.id);
        let rel = rgit_core::storage::repo_rel_path(&hash);
        let src = args.gitlab_repos.join(&rel);
        let dst = dst_root.join(&rel);

        if !src.is_dir() {
            report.errors.push(format!(
                "project {} (id {}): repository missing at {}",
                p.path, p.id, src.display()
            ));
            continue;
        }
        copy_dir(&src, &dst).await?;
        report.repos_copied += 1;

        // Preserve the wiki repo if present (not served, not lost).
        let wiki_rel = rel.replace(".git", ".wiki.git");
        let wiki_src = args.gitlab_repos.join(&wiki_rel);
        if wiki_src.is_dir() {
            copy_dir(&wiki_src, &dst_root.join(&wiki_rel)).await?;
        }

        // Dissolve @pools alternates: make the copy self-contained (§13.3-3).
        let alternates = dst.join("objects/info/alternates");
        if alternates.exists() {
            run(
                "git",
                &["-C", dst.to_str().unwrap(), "repack", "-a", "-d"],
            )
            .await
            .with_context(|| format!("repack {}", dst.display()))?;
            std::fs::remove_file(&alternates)?;
            run(
                "git",
                &["-C", dst.to_str().unwrap(), "fsck", "--connectivity-only", "--no-progress"],
            )
            .await
            .with_context(|| format!("fsck after repack {}", dst.display()))?;
            report.repos_repacked += 1;
        }
    }
    Ok(())
}

async fn copy_lfs_objects(
    args: &Args,
    objects: &[(String, i64)],
    report: &mut Report,
) -> anyhow::Result<()> {
    let cfg = rgit_core::config::StorageConfig {
        repositories: args.out.join("repositories"),
        lfs_objects: args.out.join("lfs-objects"),
    };
    for (oid, size) in objects {
        let src = args
            .gitlab_lfs
            .join(&oid[0..2])
            .join(&oid[2..4])
            .join(&oid[4..]);
        let dst = rgit_core::storage::lfs_path(&cfg, oid);
        match std::fs::metadata(&src) {
            Ok(meta) if meta.len() == *size as u64 => {
                std::fs::create_dir_all(dst.parent().unwrap())?;
                std::fs::copy(&src, &dst)?;
            }
            Ok(meta) => report.errors.push(format!(
                "lfs {oid}: size mismatch (db {size}, disk {})",
                meta.len()
            )),
            Err(_) => report.errors.push(format!("lfs {oid}: missing at {}", src.display())),
        }
    }
    Ok(())
}

async fn verify(args: &Args, db: &SqlitePool, report: &mut Report) -> anyhow::Result<()> {
    // Every project must have a working repo HEAD lookup.
    let rows: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id, path, disk_hash FROM projects")
            .fetch_all(db)
            .await?;
    for (id, path, hash) in rows {
        let repo = args
            .out
            .join("repositories")
            .join(rgit_core::storage::repo_rel_path(&hash));
        if run("git", &["-C", repo.to_str().unwrap(), "rev-parse", "--git-dir"])
            .await
            .is_err()
        {
            report
                .errors
                .push(format!("verify: project {path} (id {id}) unreadable at {}", repo.display()));
        }
    }
    // Populate default_branch from disk.
    let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, disk_hash FROM projects")
        .fetch_all(db)
        .await?;
    for (id, hash) in rows {
        let repo = args
            .out
            .join("repositories")
            .join(rgit_core::storage::repo_rel_path(&hash));
        if let Ok(out) = run("git", &["-C", repo.to_str().unwrap(), "symbolic-ref", "--short", "HEAD"]).await
        {
            let branch = String::from_utf8_lossy(&out).trim().to_string();
            sqlx::query("UPDATE projects SET default_branch = ?1 WHERE id = ?2")
                .bind(branch)
                .bind(id)
                .execute(db)
                .await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------

async fn copy_dir(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst.parent().unwrap())?;
    run(
        "cp",
        &["-a", src.to_str().unwrap(), dst.to_str().unwrap()],
    )
    .await
    .with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
    Ok(())
}

async fn run(bin: &str, argv: &[&str]) -> anyhow::Result<Vec<u8>> {
    let out = tokio::process::Command::new(bin)
        .args(argv)
        .output()
        .await
        .with_context(|| format!("spawning {bin}"))?;
    if !out.status.success() {
        bail!(
            "{bin} {argv:?} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}
