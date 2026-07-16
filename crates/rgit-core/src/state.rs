//! Shared application state passed to every server component.

use crate::config::AppConfig;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: sqlx::SqlitePool,
    operations: OperationTracker,
    git_slots: Arc<Semaphore>,
}

#[derive(Clone, Default)]
pub struct OperationTracker {
    inner: Arc<OperationTrackerInner>,
}

#[derive(Default)]
struct OperationTrackerInner {
    active: AtomicUsize,
    idle: tokio::sync::Notify,
}

pub struct OperationGuard {
    inner: Arc<OperationTrackerInner>,
}

pub struct GitOperationGuard {
    _operation: OperationGuard,
    _permit: OwnedSemaphorePermit,
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        if self.inner.active.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.inner.idle.notify_waiters();
        }
    }
}

impl OperationTracker {
    pub fn begin(&self) -> OperationGuard {
        self.inner.active.fetch_add(1, Ordering::AcqRel);
        OperationGuard {
            inner: self.inner.clone(),
        }
    }

    pub fn active(&self) -> usize {
        self.inner.active.load(Ordering::Acquire)
    }

    pub async fn wait_idle(&self) {
        loop {
            let notified = self.inner.idle.notified();
            if self.active() == 0 {
                return;
            }
            notified.await;
        }
    }
}

impl AppState {
    /// Open (creating if needed) the SQLite database, run migrations,
    /// prepare on-disk storage directories, and bootstrap the root admin.
    pub async fn init(config: AppConfig) -> anyhow::Result<Self> {
        config.validate()?;
        let db = crate::db::connect(&config.db).await?;
        crate::db::migrate(&db).await?;
        crate::storage::ensure_dirs(&config.storage)?;
        let stale_uploads = crate::storage::cleanup_lfs_temp(&config.storage)?;
        if stale_uploads > 0 {
            tracing::info!(stale_uploads, "removed stale LFS upload fragments");
        }
        seed_root(&db, &config).await?;
        Ok(Self::from_parts(config, db))
    }

    pub fn from_parts(config: AppConfig, db: sqlx::SqlitePool) -> Self {
        let git_slots = Arc::new(Semaphore::new(config.git.max_concurrent_operations));
        Self {
            config: Arc::new(config),
            db,
            operations: OperationTracker::default(),
            git_slots,
        }
    }

    pub fn begin_operation(&self) -> OperationGuard {
        self.operations.begin()
    }

    pub async fn begin_git_operation(&self) -> crate::Result<GitOperationGuard> {
        let permit = tokio::time::timeout(
            std::time::Duration::from_secs(self.config.git.queue_timeout_secs),
            self.git_slots.clone().acquire_owned(),
        )
        .await
        .map_err(|_| crate::Error::Busy)?
        .map_err(|_| crate::Error::Busy)?;
        Ok(GitOperationGuard {
            _operation: self.operations.begin(),
            _permit: permit,
        })
    }

    pub fn operations(&self) -> &OperationTracker {
        &self.operations
    }
}

/// First start: create the `root` admin. Password comes from
/// $RGIT_INITIAL_ROOT_PASSWORD, or is generated and logged once.
async fn seed_root(db: &sqlx::SqlitePool, config: &AppConfig) -> anyhow::Result<()> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(db)
        .await?;
    if users > 0 {
        return Ok(());
    }

    let (password, generated) = match std::env::var("RGIT_INITIAL_ROOT_PASSWORD") {
        Ok(p) if !p.is_empty() => (p, false),
        _ => {
            use rand::Rng;
            const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789";
            let mut rng = rand::thread_rng();
            let p: String = (0..20)
                .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
                .collect();
            (p, true)
        }
    };
    crate::auth::password::check_password_policy(&password, config.auth.min_password_length)?;
    let hash =
        crate::auth::password::hash_password_async(password.clone(), config.auth.bcrypt_cost)
            .await?;

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
        tracing::warn!(
            "created initial admin 'root' with password: {password} — change it after first login"
        );
    } else {
        tracing::info!(
            "created initial admin 'root' with password from RGIT_INITIAL_ROOT_PASSWORD"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn git_operations_are_bounded() {
        let mut config = AppConfig::default();
        config.git.max_concurrent_operations = 1;
        config.git.queue_timeout_secs = 1;
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite");
        let state = AppState::from_parts(config, db);

        let first = state.begin_git_operation().await.expect("first slot");
        assert!(matches!(
            state.begin_git_operation().await,
            Err(crate::Error::Busy)
        ));
        drop(first);
        assert!(state.begin_git_operation().await.is_ok());
    }
}
