//! Shared application state passed to every server component.

use crate::config::AppConfig;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: sqlx::SqlitePool,
}

impl AppState {
    /// Open (creating if needed) the SQLite database, run migrations,
    /// prepare on-disk storage directories, and bootstrap the root admin.
    pub async fn init(config: AppConfig) -> anyhow::Result<Self> {
        let db = crate::db::connect(&config.db).await?;
        crate::db::migrate(&db).await?;
        crate::storage::ensure_dirs(&config.storage)?;
        seed_root(&db, &config).await?;
        Ok(Self { config: Arc::new(config), db })
    }
}

/// First start: create the `root` admin. Password comes from
/// $RGIT_INITIAL_ROOT_PASSWORD, or is generated and logged once.
async fn seed_root(db: &sqlx::SqlitePool, config: &AppConfig) -> anyhow::Result<()> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(db).await?;
    if users > 0 {
        return Ok(());
    }

    let (password, generated) = match std::env::var("RGIT_INITIAL_ROOT_PASSWORD") {
        Ok(p) if !p.is_empty() => (p, false),
        _ => {
            use rand::Rng;
            const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789";
            let mut rng = rand::thread_rng();
            let p: String =
                (0..20).map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char).collect();
            (p, true)
        }
    };
    let hash = crate::auth::password::hash_password(&password, config.auth.bcrypt_cost)?;

    let mut tx = db.begin().await?;
    let root_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO users (username, email, name, password_hash, is_admin)
        VALUES ('root', 'root@localhost', 'Administrator', ?1, 1) RETURNING id
        "#,
    )
    .bind(&hash)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO namespaces (path, name, kind, owner_user_id) VALUES ('root', 'Administrator', 'user', ?1)",
    )
    .bind(root_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    if generated {
        tracing::warn!("created initial admin 'root' with password: {password} — change it after first login");
    } else {
        tracing::info!("created initial admin 'root' with password from RGIT_INITIAL_ROOT_PASSWORD");
    }
    Ok(())
}
