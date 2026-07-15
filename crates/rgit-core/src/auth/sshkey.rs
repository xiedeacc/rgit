//! SSH public key parsing and fingerprints.
//!
//! Fingerprint format matches GitLab's `fingerprint_sha256` column:
//! unpadded base64 of sha256(raw key blob), without the "SHA256:" prefix.

use crate::{Error, Result};
use ssh_key::PublicKey;

pub struct ParsedKey {
    /// Normalized "<algo> <base64>" line (comment stripped).
    pub normalized: String,
    /// Unpadded base64 sha256 fingerprint (no "SHA256:" prefix).
    pub fingerprint_sha256: String,
    pub algorithm: String,
    pub comment: String,
}

pub fn parse_public_key(input: &str) -> Result<ParsedKey> {
    let key = PublicKey::from_openssh(input.trim())
        .map_err(|e| Error::invalid(format!("invalid SSH public key: {e}")))?;

    // Reject weak keys.
    if let ssh_key::public::KeyData::Rsa(rsa) = key.key_data() {
        let bits = rsa.n.as_bytes().len() * 8;
        if bits < 2048 {
            return Err(Error::invalid("RSA keys must be at least 2048 bits"));
        }
    }
    if matches!(key.algorithm(), ssh_key::Algorithm::Dsa) {
        return Err(Error::invalid("DSA keys are not accepted"));
    }

    let fp = key.fingerprint(ssh_key::HashAlg::Sha256).to_string();
    // ssh_key renders "SHA256:<base64>"; GitLab stores only the base64 part.
    let fingerprint_sha256 = fp.strip_prefix("SHA256:").unwrap_or(&fp).to_string();

    let comment = key.comment().to_string();
    let mut stripped = key.clone();
    stripped.set_comment("");
    let normalized = stripped
        .to_openssh()
        .map_err(|e| Error::invalid(format!("cannot re-encode key: {e}")))?
        .trim()
        .to_string();

    Ok(ParsedKey {
        normalized,
        fingerprint_sha256,
        algorithm: key.algorithm().to_string(),
        comment,
    })
}
