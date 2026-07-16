//! SQLite pool setup and embedded migrations.

use std::{borrow::Cow, sync::LazyLock};

use crate::config::DbConfig;
use sqlx::migrate::{Migration, MigrationType, Migrator};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

static MIGRATOR: LazyLock<Migrator> = LazyLock::new(|| Migrator {
    migrations: Cow::Owned(vec![
        migration(1, "init", include_str!("../migrations/0001_init.sql")),
        migration(
            2,
            "project disk id allocations",
            include_str!("../migrations/0002_project_disk_id_allocations.sql"),
        ),
        migration(
            3,
            "protect ownership invariants",
            include_str!("../migrations/0003_protect_ownership_invariants.sql"),
        ),
        migration(
            4,
            "lfs auth tokens",
            include_str!("../migrations/0004_lfs_auth_tokens.sql"),
        ),
        migration(
            5,
            "membership constraints",
            include_str!("../migrations/0005_membership_constraints.sql"),
        ),
    ]),
    ignore_missing: false,
    locking: true,
    no_tx: false,
});

fn migration(version: i64, description: &'static str, sql: &'static str) -> Migration {
    Migration::new(
        version,
        Cow::Borrowed(description),
        MigrationType::Simple,
        Cow::Borrowed(sql),
        false,
    )
}

pub async fn connect(cfg: &DbConfig) -> anyhow::Result<SqlitePool> {
    if let Some(parent) = cfg.path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let options = SqliteConnectOptions::new()
        .filename(&cfg.path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(16)
        .connect_with(options)
        .await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cfg.path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(pool)
}

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    MIGRATOR.run(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn database_protects_last_admin_and_group_owner() {
        let path = std::env::temp_dir().join(format!(
            "rgit-invariants-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let pool = connect(&DbConfig { path: path.clone() }).await.unwrap();
        migrate(&pool).await.unwrap();
        sqlx::query(
            r#"
            INSERT INTO users (id, username, email, password_hash, is_admin)
            VALUES (1, 'admin', 'admin@example.test', 'unused', 1),
                   (2, 'member', 'member@example.test', 'unused', 0);
            INSERT INTO namespaces (id, path, name, kind)
            VALUES (1, 'team', 'Team', 'group');
            INSERT INTO group_members (namespace_id, user_id, access_level)
            VALUES (1, 1, 50);
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(sqlx::query("UPDATE users SET is_admin = 0 WHERE id = 1")
            .execute(&pool)
            .await
            .is_err());
        assert!(
            sqlx::query("DELETE FROM group_members WHERE namespace_id = 1 AND user_id = 1")
                .execute(&pool)
                .await
                .is_err()
        );

        sqlx::query(
            "INSERT INTO namespaces (id, path, name, kind, owner_user_id) VALUES (2, 'member', 'Member', 'user', 2)",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(sqlx::query(
            "INSERT INTO group_members (namespace_id, user_id, access_level) VALUES (2, 2, 30)",
        )
        .execute(&pool)
        .await
        .is_err());
        assert!(sqlx::query(
            "INSERT INTO group_members (namespace_id, user_id, access_level) VALUES (1, 2, 99)",
        )
        .execute(&pool)
        .await
        .is_err());

        // Cascading membership deletion while deleting the group itself is valid.
        sqlx::query("DELETE FROM namespaces WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let _ = std::fs::remove_file(path);
    }
}
