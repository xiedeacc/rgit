//! Password hashing. bcrypt, verbatim-compatible with GitLab/Devise
//! `encrypted_password` ($2a$/$2b$ hashes copied by the migration verify as-is).

use crate::{Error, Result};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Semaphore;

const PASSWORD_WORKERS: usize = 4;
const PASSWORD_WORKER_WAIT: Duration = Duration::from_secs(10);

fn workers() -> &'static Semaphore {
    static WORKERS: OnceLock<Semaphore> = OnceLock::new();
    WORKERS.get_or_init(|| Semaphore::new(PASSWORD_WORKERS))
}

pub fn hash_password(plain: &str, cost: u32) -> Result<String> {
    bcrypt::hash(plain, cost).map_err(|e| Error::Internal(anyhow::anyhow!(e)))
}

/// Constant-time verification is provided by the bcrypt crate itself.
pub fn verify_password(plain: &str, hash: &str) -> bool {
    if plain.len() > 72 {
        return false;
    }
    bcrypt::verify(plain, hash).unwrap_or(false)
}

pub async fn hash_password_async(plain: String, cost: u32) -> Result<String> {
    let _permit = tokio::time::timeout(PASSWORD_WORKER_WAIT, workers().acquire())
        .await
        .map_err(|_| Error::Internal(anyhow::anyhow!("password worker pool is busy")))?
        .map_err(|_| Error::Internal(anyhow::anyhow!("password worker pool is closed")))?;
    tokio::task::spawn_blocking(move || hash_password(&plain, cost))
        .await
        .map_err(|error| Error::Internal(anyhow::anyhow!(error)))?
}

pub async fn verify_password_async(plain: String, hash: String) -> bool {
    let Ok(Ok(_permit)) = tokio::time::timeout(PASSWORD_WORKER_WAIT, workers().acquire()).await
    else {
        return false;
    };
    tokio::task::spawn_blocking(move || verify_password(&plain, &hash))
        .await
        .unwrap_or(false)
}

/// Basic strength policy; the full policy is enforced at the API layer.
pub fn check_password_policy(plain: &str, min_len: usize) -> Result<()> {
    if plain.chars().count() < min_len {
        return Err(Error::invalid(format!(
            "password must be at least {min_len} characters"
        )));
    }
    if plain.len() > 72 {
        return Err(Error::invalid("password must be at most 72 UTF-8 bytes"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_passwords_beyond_bcrypt_input_limit() {
        assert!(check_password_policy(&"a".repeat(72), 10).is_ok());
        assert!(check_password_policy(&"a".repeat(73), 10).is_err());

        let hash = hash_password(&"a".repeat(72), 4).expect("hash password");
        assert!(verify_password(&"a".repeat(72), &hash));
        assert!(!verify_password(&"a".repeat(73), &hash));
    }
}
