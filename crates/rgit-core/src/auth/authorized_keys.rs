//! OpenSSH authorized_keys materialization for the system-sshd transport.

use crate::auth::sshkey;
use crate::config::SshConfig;
use crate::{Error, Result};
use sqlx::SqlitePool;
use std::io::Write;
use std::path::Path;

static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub async fn sync_if_enabled(db: &SqlitePool, config: &SshConfig) -> Result<usize> {
    if config.enabled {
        return sync(db, config).await;
    }
    Ok(0)
}

/// Rebuild the complete file from active users and atomically replace it.
pub async fn sync(db: &SqlitePool, config: &SshConfig) -> Result<usize> {
    let _guard = SYNC_LOCK.lock().await;
    let rows: Vec<(i64, String)> = sqlx::query_as(
        r#"
        SELECT k.id, k.key
          FROM ssh_keys k
          JOIN users u ON u.id = k.user_id
         WHERE u.state = 'active'
         ORDER BY k.id
        "#,
    )
    .fetch_all(db)
    .await?;

    let mut body = String::new();
    for (key_id, key) in &rows {
        let key = sshkey::parse_public_key(key)?;
        body.push_str(&format!(
            "command=\"{} --config {} --key-id {}\",no-port-forwarding,no-X11-forwarding,no-agent-forwarding,no-pty {}\n",
            config.shell_path.display(),
            config.shell_config.display(),
            key_id,
            key.normalized
        ));
    }
    atomic_write(&config.authorized_keys_file, body.as_bytes())?;
    Ok(rows.len())
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("authorized_keys_file has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".authorized_keys.tmp.{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let result = (|| -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DbConfig, SshConfig};

    #[tokio::test]
    async fn writes_only_active_keys_with_forced_commands() {
        let root = std::env::temp_dir().join(format!(
            "rgit-authorized-keys-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = crate::db::connect(&DbConfig {
            path: root.join("rgit.db"),
        })
        .await
        .unwrap();
        crate::db::migrate(&db).await.unwrap();
        sqlx::query(
            r#"
            INSERT INTO users (id, username, email, password_hash, state)
            VALUES (1, 'active', 'active@example.test', 'x', 'active'),
                   (2, 'blocked', 'blocked@example.test', 'x', 'blocked');
            INSERT INTO ssh_keys (id, user_id, title, key, fingerprint_sha256)
            VALUES (11, 1, 'active', 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIP4f7Agpkm6fA66cPIPJ2NDvT0J/Dt0hOe3guWaLQOC3 active', 'one'),
                   (12, 2, 'blocked', 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIP4f7Agpkm6fA66cPIPJ2NDvT0J/Dt0hOe3guWaLQOC3 blocked', 'two');
            "#,
        )
        .execute(&db)
        .await
        .unwrap();
        let config = SshConfig {
            authorized_keys_file: root.join("authorized_keys"),
            shell_path: "/opt/rgit/bin/rgit-shell".into(),
            shell_config: "/opt/rgit/conf/rgit.toml".into(),
            ..SshConfig::default()
        };
        assert_eq!(sync(&db, &config).await.unwrap(), 1);
        let output = std::fs::read_to_string(&config.authorized_keys_file).unwrap();
        assert!(output.contains("--key-id 11"));
        assert!(output.contains("no-port-forwarding"));
        assert!(!output.contains("--key-id 12"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&config.authorized_keys_file)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        db.close().await;
        std::fs::remove_dir_all(root).unwrap();
    }
}
