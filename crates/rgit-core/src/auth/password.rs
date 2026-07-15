//! Password hashing. bcrypt, verbatim-compatible with GitLab/Devise
//! `encrypted_password` ($2a$/$2b$ hashes copied by the migration verify as-is).

use crate::{Error, Result};

pub fn hash_password(plain: &str, cost: u32) -> Result<String> {
    bcrypt::hash(plain, cost).map_err(|e| Error::Internal(anyhow::anyhow!(e)))
}

/// Constant-time verification is provided by the bcrypt crate itself.
pub fn verify_password(plain: &str, hash: &str) -> bool {
    bcrypt::verify(plain, hash).unwrap_or(false)
}

/// Basic strength policy; the full policy is enforced at the API layer.
pub fn check_password_policy(plain: &str, min_len: usize) -> Result<()> {
    if plain.chars().count() < min_len {
        return Err(Error::invalid(format!(
            "password must be at least {min_len} characters"
        )));
    }
    Ok(())
}
