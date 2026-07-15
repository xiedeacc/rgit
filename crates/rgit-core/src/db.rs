//! SQLite pool setup and embedded migrations.

use crate::config::DbConfig;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

pub async fn connect(cfg: &DbConfig) -> anyhow::Result<SqlitePool> {
    if let Some(parent) = cfg.path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let options = SqliteConnectOptions::new()
        .filename(&cfg.path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(16)
        .connect_with(options)
        .await?;
    Ok(pool)
}

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
