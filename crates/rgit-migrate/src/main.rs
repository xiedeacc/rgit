//! rgit-migrate — GitLab (omnibus, PostgreSQL 17.11.x) → rgit (SQLite).
//!
//! Reads only the tables/columns listed in DESIGN.md §13.2, writes the rgit
//! SQLite database, and either copies repositories/LFS or reuses GitLab's
//! existing storage roots. The GitLab installation is never modified. See
//! docs/MIGRATION.md for the runbook.

use anyhow::{bail, Context};
use chrono::NaiveDateTime;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser, Debug)]
#[command(
    name = "rgit-migrate",
    about = "Migrate GitLab (PostgreSQL) to rgit (SQLite)"
)]
struct Args {
    /// GitLab PostgreSQL URL, e.g.
    /// postgres://postgres@localhost/gitlabhq_production?host=/var/run/postgresql
    #[arg(long)]
    pg: String,

    /// GitLab repositories root (omnibus default shown).
    #[arg(long, default_value = "/var/opt/gitlab/git-data/repositories")]
    gitlab_repos: PathBuf,

    /// Additional GitLab repository storage, repeatable as NAME=PATH.
    #[arg(long = "gitlab-storage", value_name = "NAME=PATH", value_parser = parse_storage_mapping)]
    gitlab_storages: Vec<StorageMapping>,

    /// GitLab LFS objects root (omnibus default shown).
    #[arg(
        long,
        default_value = "/var/opt/gitlab/gitlab-rails/shared/lfs-objects"
    )]
    gitlab_lfs: PathBuf,

    /// rgit data directory to create.
    #[arg(long)]
    out: PathBuf,

    /// Keep repositories and LFS in their existing GitLab paths. Only SQLite
    /// and the report are written; no repository copy/repack/fsck is run.
    #[arg(long)]
    reuse_storage: bool,

    /// Read + report only; write nothing.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Clone, Debug)]
struct StorageMapping {
    name: String,
    path: PathBuf,
}

fn parse_storage_mapping(value: &str) -> Result<StorageMapping, String> {
    let (name, path) = value
        .split_once('=')
        .ok_or_else(|| "expected NAME=PATH".to_string())?;
    if name.is_empty() || path.is_empty() {
        return Err("storage name and path must not be empty".into());
    }
    Ok(StorageMapping {
        name: name.into(),
        path: PathBuf::from(path),
    })
}

#[derive(Default, serde::Serialize)]
struct Report {
    storage_mode: String,
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
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();

    let pg = PgPoolOptions::new()
        .max_connections(4)
        .connect(&args.pg)
        .await
        .context("connecting to GitLab PostgreSQL")?;

    let mut report = Report {
        storage_mode: if args.reuse_storage { "reuse" } else { "copy" }.into(),
        ..Report::default()
    };

    // ---- Phase 1: read GitLab tables (read-only) --------------------------
    let users = read_users(&pg).await?;
    let namespaces = read_namespaces(&pg, &mut report).await?;
    let projects = read_projects(&pg, &mut report).await?;
    let members = read_members(&pg).await?;
    let keys = read_keys(&pg).await?;
    let lfs = read_lfs(&pg, &mut report).await?;
    let forks: Vec<(i64, i64)> = sqlx::query(
        "SELECT project_id::bigint, forked_from_project_id::bigint FROM fork_network_members
         WHERE forked_from_project_id IS NOT NULL",
    )
    .fetch_all(&pg)
    .await?
    .into_iter()
    .map(|r| Ok((r.try_get::<i64, _>(0)?, r.try_get::<i64, _>(1)?)))
    .collect::<Result<Vec<_>, sqlx::Error>>()?;

    report.users = users.len() as u64;
    report.namespaces = namespaces.len() as u64;
    report.projects = projects.len() as u64;
    report.ssh_keys = keys.len() as u64;
    report.lfs_objects = lfs.objects.len() as u64;
    report.lfs_bytes = lfs.objects.iter().map(|(_, size)| *size as u64).sum();
    let human_ids: std::collections::HashSet<i64> = users.iter().map(|user| user.id).collect();
    report.project_members = members
        .iter()
        .filter(|member| member.source_type == "Project" && human_ids.contains(&member.user_id))
        .count() as u64;
    report.group_members = members
        .iter()
        .filter(|member| member.source_type == "Namespace" && human_ids.contains(&member.user_id))
        .count() as u64;
    preflight_sources(&args, &projects, &lfs.objects, &mut report)?;

    tracing::info!(
        users = users.len(),
        namespaces = namespaces.len(),
        projects = projects.len(),
        "GitLab data loaded"
    );

    if args.dry_run || !report.errors.is_empty() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        if !report.errors.is_empty() {
            bail!(
                "{} preflight error(s); destination was not modified",
                report.errors.len()
            );
        }
        return Ok(());
    }

    // Build everything next to the destination, then publish with one rename.
    // A failed copy or verification can never leave a half-migrated data root.
    let final_out = args.out.clone();
    validate_destination(&final_out)?;
    let mut staging = StagingDir::create(&final_out)?;
    let mut work_args = args;
    work_args.out = staging.path().to_path_buf();

    // ---- Phase 2: write SQLite -------------------------------------------
    let db_path = work_args.out.join("rgit.db");
    let sqlite = rgit_core::db::connect(&rgit_core::config::DbConfig {
        path: db_path.clone(),
    })
    .await?;
    rgit_core::db::migrate(&sqlite).await?;

    write_sqlite(
        &sqlite,
        &users,
        &namespaces,
        &projects,
        &members,
        &keys,
        &lfs,
        &forks,
        &mut report,
    )
    .await?;

    if work_args.reuse_storage {
        // Shared mode deliberately performs no Git subprocess work. GitLab
        // must remain stopped whenever rgit is serving these same paths.
        populate_shared_default_branches(&work_args, &projects, &sqlite, &mut report).await?;
    } else {
        // ---- Phase 3: copy repositories + LFS objects ---------------------
        copy_repositories(&work_args, &projects, &mut report).await?;
        copy_lfs_objects(&work_args, &lfs.objects, &mut report).await?;

        // ---- Phase 4: verify ----------------------------------------------
        verify(&work_args, &sqlite, &mut report).await?;
    }

    let report_path = work_args.out.join("migration-report.json");
    std::fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;

    if !report.errors.is_empty() {
        let failed_report = failed_report_path(&final_out);
        std::fs::write(&failed_report, serde_json::to_string_pretty(&report)?)?;
        bail!(
            "{} error(s); destination was not published; see {}",
            report.errors.len(),
            failed_report.display()
        );
    }
    sqlite.close().await;
    staging.publish(&final_out)?;
    println!(
        "report written to {}",
        final_out.join("migration-report.json").display()
    );
    println!("migration completed with no errors");
    Ok(())
}

// ---------------------------------------------------------------------------

fn validate_destination(out: &Path) -> anyhow::Result<()> {
    if !out.exists() {
        return Ok(());
    }
    anyhow::ensure!(
        out.is_dir(),
        "destination {} is not a directory",
        out.display()
    );
    anyhow::ensure!(
        std::fs::read_dir(out)?.next().is_none(),
        "destination {} is not empty; refusing to overwrite it",
        out.display()
    );
    Ok(())
}

fn failed_report_path(out: &Path) -> PathBuf {
    let name = out
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("rgit-data");
    out.with_file_name(format!("{name}-migration-report.json"))
}

struct StagingDir {
    path: PathBuf,
    published: bool,
}

impl StagingDir {
    fn create(out: &Path) -> anyhow::Result<Self> {
        let parent = out.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let name = out
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("rgit-data");
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!(
            ".{name}.rgit-migrate-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path)?;
        Ok(Self {
            path,
            published: false,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn publish(&mut self, out: &Path) -> anyhow::Result<()> {
        if out.exists() {
            std::fs::remove_dir(out)
                .with_context(|| format!("remove empty destination {}", out.display()))?;
        }
        std::fs::rename(&self.path, out)
            .with_context(|| format!("publish migration to {}", out.display()))?;
        self.published = true;
        Ok(())
    }
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        if !self.published {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
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
        r#"SELECT id::bigint, username, email, COALESCE(name,''),
                  COALESCE(encrypted_password, '!'),
                  admin, COALESCE(state,'active')
           FROM users WHERE user_type = 0 AND username IS NOT NULL ORDER BY id"#,
    )
    .fetch_all(pg)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            Ok(GlUser {
                id: r.try_get(0)?,
                username: r.try_get(1)?,
                email: r.try_get(2)?,
                name: r.try_get(3)?,
                encrypted_password: r.try_get(4)?,
                admin: r.try_get(5)?,
                state: r.try_get(6)?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?)
}

struct GlNamespace {
    id: i64,
    name: String,
    path: String,
    kind: String, // "user" | "group"
    owner_id: Option<i64>,
    /// GitLab group ids from the root group through this namespace.
    ancestry: Vec<i64>,
}

struct RawNamespace {
    id: i64,
    name: String,
    path: String,
    kind: String,
    owner_id: Option<i64>,
    parent_id: Option<i64>,
}

async fn read_namespaces(pg: &PgPool, report: &mut Report) -> anyhow::Result<Vec<GlNamespace>> {
    let rows = sqlx::query(
        r#"SELECT id::bigint, name, path, type, owner_id::bigint, parent_id::bigint
           FROM namespaces
           WHERE type IN ('User','Group') ORDER BY id"#,
    )
    .fetch_all(pg)
    .await?;

    let raw: Vec<RawNamespace> = rows
        .into_iter()
        .map(|r| {
            Ok(RawNamespace {
                id: r.try_get(0)?,
                name: r.try_get(1)?,
                path: r.try_get(2)?,
                kind: if r.try_get::<String, _>(3)? == "User" {
                    "user".into()
                } else {
                    "group".into()
                },
                owner_id: r.try_get(4)?,
                parent_id: r.try_get(5)?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    flatten_namespaces(&raw, report)
}

fn flatten_namespaces(
    raw: &[RawNamespace],
    report: &mut Report,
) -> anyhow::Result<Vec<GlNamespace>> {
    let by_id: HashMap<i64, &RawNamespace> = raw
        .iter()
        .map(|namespace| (namespace.id, namespace))
        .collect();

    let mut resolved = Vec::with_capacity(raw.len());
    for namespace in raw {
        if namespace.kind == "user" {
            resolved.push((namespace, namespace.path.clone(), Vec::new(), 0_usize));
            continue;
        }

        let mut chain = Vec::new();
        let mut current = Some(namespace.id);
        let mut seen = HashSet::new();
        while let Some(id) = current {
            if !seen.insert(id) {
                anyhow::bail!("namespace parent cycle detected at id {id}");
            }
            let item = by_id.get(&id).ok_or_else(|| {
                anyhow::anyhow!("namespace {} has missing parent {id}", namespace.id)
            })?;
            if item.kind != "group" {
                anyhow::bail!(
                    "group namespace {} has non-group ancestor {id}",
                    namespace.id
                );
            }
            chain.push(*item);
            current = item.parent_id;
        }
        chain.reverse();
        let ancestry = chain.iter().map(|item| item.id).collect::<Vec<_>>();
        let path = chain
            .iter()
            .map(|item| item.path.as_str())
            .collect::<Vec<_>>()
            .join("--");
        resolved.push((namespace, path, ancestry, chain.len()));
    }

    // Root namespaces keep their original path when possible. Nested paths
    // are deterministic and collision-safe within rgit's flat namespace.
    resolved.sort_by_key(|(namespace, _, _, depth)| (*depth, namespace.id));
    let mut used = HashSet::new();
    let mut namespaces = Vec::with_capacity(resolved.len());
    for (namespace, mut path, ancestry, depth) in resolved {
        let base = path.clone();
        if path.len() > 255 || used.contains(&path.to_ascii_lowercase()) {
            let suffix = format!("--{}", namespace.id);
            let max_base = 255_usize.saturating_sub(suffix.len());
            let mut boundary = max_base.min(path.len());
            while !path.is_char_boundary(boundary) {
                boundary -= 1;
            }
            path.truncate(boundary);
            path.push_str(&suffix);
        }
        if !used.insert(path.to_ascii_lowercase()) {
            anyhow::bail!(
                "cannot generate a unique flattened path for namespace {}",
                namespace.id
            );
        }
        if depth > 1 || path != base {
            report.warnings.push(format!(
                "group namespace {} path '{}' mapped to '{}'",
                namespace.id, base, path
            ));
        }
        namespaces.push(GlNamespace {
            id: namespace.id,
            name: namespace.name.clone(),
            path,
            kind: namespace.kind.clone(),
            owner_id: namespace.owner_id,
            ancestry,
        });
    }
    namespaces.sort_by_key(|namespace| namespace.id);
    Ok(namespaces)
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
    repository_storage: String,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

async fn read_projects(pg: &PgPool, report: &mut Report) -> anyhow::Result<Vec<GlProject>> {
    let legacy: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE COALESCE(storage_version, 0) < 2")
            .fetch_one(pg)
            .await?;
    if legacy > 0 {
        report.errors.push(format!(
            "{legacy} project(s) on legacy storage — run GitLab's hashed-storage migration first"
        ));
    }

    let rows = sqlx::query(
        r#"SELECT id::bigint, COALESCE(name,path), path, COALESCE(description,''),
                  namespace_id::bigint, visibility_level, archived,
                  COALESCE(lfs_enabled, true), COALESCE(repository_storage, 'default'),
                  created_at, updated_at
           FROM projects WHERE pending_delete = false OR pending_delete IS NULL
           ORDER BY id"#,
    )
    .fetch_all(pg)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            Ok(GlProject {
                id: r.try_get(0)?,
                name: r.try_get(1)?,
                path: r.try_get(2)?,
                description: r.try_get(3)?,
                namespace_id: r.try_get(4)?,
                visibility_level: r.try_get(5)?,
                archived: r.try_get(6)?,
                lfs_enabled: r.try_get(7)?,
                repository_storage: r.try_get(8)?,
                created_at: r.try_get(9)?,
                updated_at: r.try_get(10)?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?)
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
        r#"SELECT source_type, source_id::bigint, user_id::bigint, access_level FROM members
           WHERE user_id IS NOT NULL AND requested_at IS NULL AND invite_token IS NULL"#,
    )
    .fetch_all(pg)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            Ok(GlMember {
                source_type: r.try_get(0)?,
                source_id: r.try_get(1)?,
                user_id: r.try_get(2)?,
                access_level: r.try_get(3)?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?)
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
        r#"SELECT user_id::bigint, COALESCE(title,''), key, fingerprint_sha256
           FROM keys WHERE (type IS NULL OR type = 'Key') AND user_id IS NOT NULL
             AND key IS NOT NULL AND fingerprint_sha256 IS NOT NULL"#,
    )
    .fetch_all(pg)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            Ok(GlKey {
                user_id: r.try_get(0)?,
                title: r.try_get(1)?,
                key: r.try_get(2)?,
                fingerprint_sha256: base64::engine::general_purpose::STANDARD_NO_PAD
                    .encode(r.try_get::<Vec<u8>, _>(3)?),
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?)
}

#[derive(Default)]
struct GlLfs {
    /// oid → size
    objects: Vec<(String, i64)>,
    /// (project_id, oid)
    links: Vec<(i64, String)>,
}

async fn read_lfs(pg: &PgPool, report: &mut Report) -> anyhow::Result<GlLfs> {
    let remote: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lfs_objects WHERE file_store <> 1")
        .fetch_one(pg)
        .await?;
    if remote > 0 {
        report.errors.push(format!(
            "{remote} LFS object(s) in object storage — run `gitlab-rake gitlab:lfs:migrate_to_local` first"
        ));
    }

    let objects = sqlx::query("SELECT oid, size::bigint FROM lfs_objects ORDER BY id")
        .fetch_all(pg)
        .await?
        .into_iter()
        .map(|r| Ok((r.try_get::<String, _>(0)?, r.try_get::<i64, _>(1)?)))
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    let links = sqlx::query(
        r#"SELECT DISTINCT lp.project_id::bigint, o.oid
           FROM lfs_objects_projects lp JOIN lfs_objects o ON o.id = lp.lfs_object_id"#,
    )
    .fetch_all(pg)
    .await?
    .into_iter()
    .map(|r| Ok((r.try_get::<i64, _>(0)?, r.try_get::<String, _>(1)?)))
    .collect::<Result<Vec<_>, sqlx::Error>>()?;
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
        let state = if u.state == "active" {
            "active"
        } else {
            "blocked"
        };
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
    }

    let user_ids: HashMap<i64, ()> = users.iter().map(|u| (u.id, ())).collect();
    for n in namespaces {
        let owner_migrated = n
            .owner_id
            .is_some_and(|owner| user_ids.contains_key(&owner));
        let kind = if n.kind == "user" && !owner_migrated {
            report.warnings.push(format!(
                "namespace {} ('{}') owner was not migrated; preserving it as an admin-managed group",
                n.id, n.path
            ));
            "group"
        } else {
            n.kind.as_str()
        };
        sqlx::query(
            r#"INSERT INTO namespaces (id, path, name, kind, owner_user_id)
               VALUES (?1, ?2, ?3, ?4, ?5)"#,
        )
        .bind(n.id)
        .bind(&n.path)
        .bind(&n.name)
        .bind(kind)
        .bind(if kind == "user" { n.owner_id } else { None })
        .execute(&mut *tx)
        .await
        .with_context(|| format!("namespace {}", n.path))?;
    }

    for p in projects {
        // disk_id = GitLab project id keeps @hashed paths valid (DESIGN.md §5).
        sqlx::query(
            r#"INSERT INTO projects (id, namespace_id, path, name, description, visibility,
                                     archived, lfs_enabled, disk_id, disk_hash,
                                     created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"#,
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
        .bind(p.created_at)
        .bind(p.updated_at)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("project {}", p.path))?;
    }

    // Fork parents may have a larger id than their children, so apply the
    // self-referential foreign keys only after every project row exists.
    for (project_id, parent_id) in forks {
        let updated = sqlx::query(
            r#"
            UPDATE projects SET forked_from_project_id = ?1
            WHERE id = ?2 AND EXISTS (SELECT 1 FROM projects WHERE id = ?1)
            "#,
        )
        .bind(parent_id)
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            report.warnings.push(format!(
                "fork relation {project_id} -> {parent_id} skipped because one project was not migrated"
            ));
        }
    }

    for m in members
        .iter()
        .filter(|member| member.source_type == "Project")
    {
        if !user_ids.contains_key(&m.user_id) {
            continue;
        }
        let res = sqlx::query(
            r#"INSERT INTO project_members (project_id, user_id, access_level)
               SELECT ?1, ?2, ?3 WHERE EXISTS (SELECT 1 FROM projects WHERE id = ?1)
               ON CONFLICT (project_id, user_id) DO UPDATE SET
                   access_level = MAX(access_level, excluded.access_level)"#,
        )
        .bind(m.source_id)
        .bind(m.user_id)
        .bind(m.access_level)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() > 0 {
            report.project_members += 1;
        }
    }

    // rgit groups are flat. Materialize GitLab's inherited ancestor group
    // memberships onto every flattened descendant group.
    let mut group_levels: HashMap<(i64, i64), i32> = HashMap::new();
    for member in members
        .iter()
        .filter(|member| member.source_type == "Namespace")
    {
        if !user_ids.contains_key(&member.user_id) {
            continue;
        }
        for namespace in namespaces
            .iter()
            .filter(|namespace| namespace.kind == "group")
        {
            if namespace.ancestry.contains(&member.source_id) {
                group_levels
                    .entry((namespace.id, member.user_id))
                    .and_modify(|level| *level = (*level).max(member.access_level))
                    .or_insert(member.access_level);
            }
        }
    }
    for ((namespace_id, user_id), access_level) in group_levels {
        sqlx::query(
            r#"INSERT INTO group_members (namespace_id, user_id, access_level)
               VALUES (?1, ?2, ?3)
               ON CONFLICT (namespace_id, user_id) DO UPDATE SET
                   access_level = MAX(access_level, excluded.access_level)"#,
        )
        .bind(namespace_id)
        .bind(user_id)
        .bind(access_level)
        .execute(&mut *tx)
        .await?;
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

    report.users = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
        .fetch_one(db)
        .await? as u64;
    report.namespaces = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM namespaces")
        .fetch_one(db)
        .await? as u64;
    report.projects = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM projects")
        .fetch_one(db)
        .await? as u64;
    report.project_members = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM project_members")
        .fetch_one(db)
        .await? as u64;
    report.group_members = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM group_members")
        .fetch_one(db)
        .await? as u64;
    report.ssh_keys = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM ssh_keys")
        .fetch_one(db)
        .await? as u64;
    report.lfs_objects = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM lfs_objects")
        .fetch_one(db)
        .await? as u64;
    report.lfs_bytes = sqlx::query_scalar::<_, Option<i64>>("SELECT SUM(size) FROM lfs_objects")
        .fetch_one(db)
        .await?
        .unwrap_or(0) as u64;
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
        let source_root = repository_root(args, p)?;
        let src = source_root.join(&rel);
        let dst = dst_root.join(&rel);

        if !src.is_dir() {
            report.errors.push(format!(
                "project {} (id {}): repository missing at {}",
                p.path,
                p.id,
                src.display()
            ));
            continue;
        }
        copy_dir(&src, &dst).await?;
        report.repos_copied += 1;

        let source_refs = repository_refs(&src)
            .await
            .with_context(|| format!("read source refs {}", src.display()))?;

        // Preserve auxiliary GitLab repositories even though rgit does not
        // expose wiki or design-management features.
        for suffix in [".wiki.git", ".design.git"] {
            let auxiliary_rel = rel.replace(".git", suffix);
            let auxiliary_src = source_root.join(&auxiliary_rel);
            if auxiliary_src.is_dir() {
                copy_dir(&auxiliary_src, &dst_root.join(&auxiliary_rel)).await?;
            }
        }

        // Dissolve @pools alternates: make the copy self-contained (§13.3-3).
        let alternates = dst.join("objects/info/alternates");
        if alternates.exists() {
            run("git", &["-C", dst.to_str().unwrap(), "repack", "-a", "-d"])
                .await
                .with_context(|| format!("repack {}", dst.display()))?;
            std::fs::remove_file(&alternates)?;
            run(
                "git",
                &[
                    "-C",
                    dst.to_str().unwrap(),
                    "fsck",
                    "--connectivity-only",
                    "--no-progress",
                ],
            )
            .await
            .with_context(|| format!("fsck after repack {}", dst.display()))?;
            report.repos_repacked += 1;
        }

        let destination_refs = repository_refs(&dst)
            .await
            .with_context(|| format!("read destination refs {}", dst.display()))?;
        if source_refs != destination_refs {
            report.errors.push(format!(
                "project {} (id {}): refs changed while copying",
                p.path, p.id
            ));
        }
        if let Err(error) = run(
            "git",
            &[
                "-C",
                dst.to_str().unwrap(),
                "fsck",
                "--full",
                "--no-progress",
            ],
        )
        .await
        {
            report.errors.push(format!(
                "project {} (id {}): git fsck failed: {error:#}",
                p.path, p.id
            ));
        }

        for suffix in [".wiki.git", ".design.git"] {
            let auxiliary_rel = rel.replace(".git", suffix);
            let auxiliary_dst = dst_root.join(&auxiliary_rel);
            if auxiliary_dst.is_dir()
                && run(
                    "git",
                    &[
                        "-C",
                        auxiliary_dst.to_str().unwrap(),
                        "fsck",
                        "--full",
                        "--no-progress",
                    ],
                )
                .await
                .is_err()
            {
                report.errors.push(format!(
                    "project {} (id {}): auxiliary repository {} failed git fsck",
                    p.path, p.id, suffix
                ));
            }
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
                let source_hash = sha256_file(&src)?;
                if source_hash != *oid {
                    report
                        .errors
                        .push(format!("lfs {oid}: source sha256 mismatch ({source_hash})"));
                    continue;
                }
                std::fs::create_dir_all(dst.parent().unwrap())?;
                std::fs::copy(&src, &dst)?;
                let destination_hash = sha256_file(&dst)?;
                if destination_hash != *oid {
                    report.errors.push(format!(
                        "lfs {oid}: destination sha256 mismatch ({destination_hash})"
                    ));
                }
            }
            Ok(meta) => report.errors.push(format!(
                "lfs {oid}: size mismatch (db {size}, disk {})",
                meta.len()
            )),
            Err(_) => report
                .errors
                .push(format!("lfs {oid}: missing at {}", src.display())),
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
        if run(
            "git",
            &["-C", repo.to_str().unwrap(), "rev-parse", "--git-dir"],
        )
        .await
        .is_err()
        {
            report.errors.push(format!(
                "verify: project {path} (id {id}) unreadable at {}",
                repo.display()
            ));
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
        if let Ok(out) = run(
            "git",
            &[
                "-C",
                repo.to_str().unwrap(),
                "symbolic-ref",
                "--short",
                "HEAD",
            ],
        )
        .await
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

async fn populate_shared_default_branches(
    args: &Args,
    projects: &[GlProject],
    db: &SqlitePool,
    report: &mut Report,
) -> anyhow::Result<()> {
    for project in projects {
        let hash = rgit_core::storage::disk_hash(project.id);
        let repo = repository_root(args, project)?.join(rgit_core::storage::repo_rel_path(&hash));
        let head = std::fs::read_to_string(repo.join("HEAD"))
            .with_context(|| format!("read HEAD for project {}", project.id))?;
        let Some(reference) = head.trim().strip_prefix("ref: refs/heads/") else {
            report.warnings.push(format!(
                "project {} (id {}): detached or unrecognized HEAD; default branch left unset",
                project.path, project.id
            ));
            continue;
        };
        if reference.is_empty() || reference.contains('\n') || reference.contains('\r') {
            report.errors.push(format!(
                "project {} (id {}): invalid HEAD branch",
                project.path, project.id
            ));
            continue;
        }
        sqlx::query("UPDATE projects SET default_branch = ?1 WHERE id = ?2")
            .bind(reference)
            .bind(project.id)
            .execute(db)
            .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------

async fn copy_dir(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst.parent().unwrap())?;
    run("cp", &["-a", src.to_str().unwrap(), dst.to_str().unwrap()])
        .await
        .with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
    Ok(())
}

fn preflight_sources(
    args: &Args,
    projects: &[GlProject],
    lfs_objects: &[(String, i64)],
    report: &mut Report,
) -> anyhow::Result<()> {
    let mut storage_names = projects
        .iter()
        .map(|project| project.repository_storage.as_str())
        .collect::<HashSet<_>>();
    if args.reuse_storage && storage_names.iter().any(|name| *name != "default") {
        report.errors.push(
            "--reuse-storage currently requires every project to use the default repository storage"
                .into(),
        );
    }
    if storage_names.len() > 1 && args.gitlab_storages.is_empty() {
        let mut names = storage_names.drain().collect::<Vec<_>>();
        names.sort_unstable();
        report.errors.push(format!(
            "projects use multiple repository storages ({}); pass --gitlab-storage NAME=PATH for each storage",
            names.join(", ")
        ));
    }
    let mut mapped = HashSet::new();
    for mapping in &args.gitlab_storages {
        if !mapped.insert(mapping.name.as_str()) {
            report.errors.push(format!(
                "duplicate --gitlab-storage mapping for '{}'",
                mapping.name
            ));
        }
    }

    for project in projects {
        let hash = rgit_core::storage::disk_hash(project.id);
        let root = match repository_root(args, project) {
            Ok(root) => root,
            Err(error) => {
                report.errors.push(error.to_string());
                continue;
            }
        };
        let source = root.join(rgit_core::storage::repo_rel_path(&hash));
        if !source.is_dir() {
            report.errors.push(format!(
                "project {} (id {}): repository missing at {}",
                project.path,
                project.id,
                source.display()
            ));
        }
    }
    for (oid, size) in lfs_objects {
        if !rgit_core::storage::is_valid_lfs_oid(oid) {
            report
                .errors
                .push(format!("lfs object has invalid oid: {oid}"));
            continue;
        }
        let source = args
            .gitlab_lfs
            .join(&oid[0..2])
            .join(&oid[2..4])
            .join(&oid[4..]);
        match std::fs::metadata(&source) {
            Ok(metadata) if metadata.len() == *size as u64 => {}
            Ok(metadata) => report.errors.push(format!(
                "lfs {oid}: size mismatch (db {size}, disk {})",
                metadata.len()
            )),
            Err(error) => report.errors.push(format!(
                "lfs {oid}: cannot read {}: {error}",
                source.display()
            )),
        }
    }
    Ok(())
}

fn repository_root<'a>(args: &'a Args, project: &GlProject) -> anyhow::Result<&'a Path> {
    if let Some(mapping) = args
        .gitlab_storages
        .iter()
        .find(|mapping| mapping.name == project.repository_storage)
    {
        return Ok(&mapping.path);
    }
    if args.gitlab_storages.is_empty() || project.repository_storage == "default" {
        return Ok(&args.gitlab_repos);
    }
    bail!(
        "project {} (id {}) uses unmapped repository storage '{}'",
        project.path,
        project.id,
        project.repository_storage
    )
}

async fn repository_refs(repo: &Path) -> anyhow::Result<Vec<u8>> {
    run(
        "git",
        &[
            "-C",
            repo.to_str()
                .ok_or_else(|| anyhow::anyhow!("non-utf8 repository path"))?,
            "for-each-ref",
            "--sort=refname",
            "--format=%(refname)%00%(objectname)",
        ],
    )
    .await
}

fn sha256_file(path: &Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn nested_namespaces_are_flattened_with_collision_safe_paths() {
        let raw = vec![
            RawNamespace {
                id: 1,
                name: "User".into(),
                path: "user".into(),
                kind: "user".into(),
                owner_id: Some(1),
                parent_id: None,
            },
            RawNamespace {
                id: 10,
                name: "Team".into(),
                path: "team".into(),
                kind: "group".into(),
                owner_id: None,
                parent_id: None,
            },
            RawNamespace {
                id: 11,
                name: "Platform".into(),
                path: "platform".into(),
                kind: "group".into(),
                owner_id: None,
                parent_id: Some(10),
            },
            RawNamespace {
                id: 12,
                name: "Existing".into(),
                path: "team--platform".into(),
                kind: "group".into(),
                owner_id: None,
                parent_id: None,
            },
        ];
        let mut report = Report::default();
        let namespaces = flatten_namespaces(&raw, &mut report).expect("flatten namespaces");
        let nested = namespaces
            .iter()
            .find(|namespace| namespace.id == 11)
            .unwrap();
        let existing = namespaces
            .iter()
            .find(|namespace| namespace.id == 12)
            .unwrap();

        assert_eq!(existing.path, "team--platform");
        assert_eq!(nested.path, "team--platform--11");
        assert_eq!(nested.ancestry, vec![10, 11]);
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn sha256_file_hashes_full_content() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rgit-migrate-hash-{nonce}"));
        std::fs::write(&path, b"rgit migration").expect("write test file");
        assert_eq!(
            sha256_file(&path).expect("hash file"),
            "ec195ee95056efa6260a112e7ad5b4cb38a597fdd2c82d525283ab776312e567"
        );
        std::fs::remove_file(path).expect("remove test file");
    }
}
