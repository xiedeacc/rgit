//! SSH public key parsing and fingerprints.
//!
//! Fingerprint format matches GitLab's `fingerprint_sha256` column:
//! unpadded base64 of sha256(raw key blob), without the "SHA256:" prefix.

use base64::{engine::general_purpose, Engine as _};
use sha2::{Digest, Sha256};

use crate::{Error, Result};

const MAX_KEY_LINE_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub struct ParsedKey {
    /// Normalized "<algo> <base64>" line (comment stripped).
    pub normalized: String,
    /// Unpadded base64 sha256 fingerprint (no "SHA256:" prefix).
    pub fingerprint_sha256: String,
    pub algorithm: String,
    pub comment: String,
}

pub fn parse_public_key(input: &str) -> Result<ParsedKey> {
    let input = input.trim();
    if input.is_empty() || input.len() > MAX_KEY_LINE_BYTES {
        return Err(Error::invalid("invalid SSH public key length"));
    }

    let mut parts = input.split_whitespace();
    let algorithm = parts
        .next()
        .ok_or_else(|| Error::invalid("missing SSH key algorithm"))?;
    let encoded = parts
        .next()
        .ok_or_else(|| Error::invalid("missing SSH public key data"))?;
    let comment = parts.collect::<Vec<_>>().join(" ");

    let blob = general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| Error::invalid("invalid SSH public key base64"))?;
    let mut cursor = 0;
    let wire_algorithm = read_string(&blob, &mut cursor, "algorithm")?;
    if wire_algorithm != algorithm {
        return Err(Error::invalid(
            "SSH public key algorithm does not match key data",
        ));
    }

    validate_key_blob(algorithm, &blob, &mut cursor)?;
    if cursor != blob.len() {
        return Err(Error::invalid("SSH public key contains trailing data"));
    }

    let canonical = general_purpose::STANDARD.encode(&blob);
    let fingerprint_sha256 = general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(&blob));

    Ok(ParsedKey {
        normalized: format!("{algorithm} {canonical}"),
        fingerprint_sha256,
        algorithm: algorithm.to_string(),
        comment,
    })
}

fn validate_key_blob(algorithm: &str, blob: &[u8], cursor: &mut usize) -> Result<()> {
    match algorithm {
        "ssh-ed25519" => {
            require_length(read_field(blob, cursor, "Ed25519 key")?, 32, "Ed25519 key")?;
        }
        "ssh-rsa" => {
            let exponent = read_field(blob, cursor, "RSA exponent")?;
            validate_positive_mpint(exponent, "RSA exponent")?;
            if exponent.len() > 8 {
                return Err(Error::invalid("RSA exponent is too large"));
            }

            let modulus = read_field(blob, cursor, "RSA modulus")?;
            validate_positive_mpint(modulus, "RSA modulus")?;
            if mpint_bits(modulus) < 2048 {
                return Err(Error::invalid("RSA keys must be at least 2048 bits"));
            }
        }
        "ecdsa-sha2-nistp256" => validate_ecdsa(blob, cursor, "nistp256", 65)?,
        "ecdsa-sha2-nistp384" => validate_ecdsa(blob, cursor, "nistp384", 97)?,
        "ecdsa-sha2-nistp521" => validate_ecdsa(blob, cursor, "nistp521", 133)?,
        "sk-ssh-ed25519@openssh.com" => {
            require_length(read_field(blob, cursor, "Ed25519 key")?, 32, "Ed25519 key")?;
            require_nonempty(
                read_field(blob, cursor, "security key application")?,
                "security key application",
            )?;
        }
        "sk-ecdsa-sha2-nistp256@openssh.com" => {
            validate_ecdsa(blob, cursor, "nistp256", 65)?;
            require_nonempty(
                read_field(blob, cursor, "security key application")?,
                "security key application",
            )?;
        }
        "ssh-dss" => return Err(Error::invalid("DSA keys are not accepted")),
        _ => return Err(Error::invalid("unsupported SSH public key algorithm")),
    }
    Ok(())
}

fn validate_ecdsa(
    blob: &[u8],
    cursor: &mut usize,
    expected_curve: &str,
    point_length: usize,
) -> Result<()> {
    let curve = read_string(blob, cursor, "ECDSA curve")?;
    if curve != expected_curve {
        return Err(Error::invalid("ECDSA curve does not match key algorithm"));
    }

    let point = read_field(blob, cursor, "ECDSA point")?;
    require_length(point, point_length, "ECDSA point")?;
    if point.first() != Some(&4) {
        return Err(Error::invalid("ECDSA point must be uncompressed"));
    }
    Ok(())
}

fn read_string<'a>(blob: &'a [u8], cursor: &mut usize, name: &str) -> Result<&'a str> {
    let field = read_field(blob, cursor, name)?;
    std::str::from_utf8(field).map_err(|_| Error::invalid(format!("invalid {name}")))
}

fn read_field<'a>(blob: &'a [u8], cursor: &mut usize, name: &str) -> Result<&'a [u8]> {
    let prefix_end = (*cursor)
        .checked_add(4)
        .ok_or_else(|| Error::invalid(format!("invalid {name}")))?;
    let length_bytes: [u8; 4] = blob
        .get(*cursor..prefix_end)
        .ok_or_else(|| Error::invalid(format!("truncated {name}")))?
        .try_into()
        .map_err(|_| Error::invalid(format!("invalid {name}")))?;
    *cursor = prefix_end;

    let length = u32::from_be_bytes(length_bytes) as usize;
    let field_end = (*cursor)
        .checked_add(length)
        .ok_or_else(|| Error::invalid(format!("invalid {name}")))?;
    let field = blob
        .get(*cursor..field_end)
        .ok_or_else(|| Error::invalid(format!("truncated {name}")))?;
    *cursor = field_end;
    Ok(field)
}

fn validate_positive_mpint(value: &[u8], name: &str) -> Result<()> {
    require_nonempty(value, name)?;
    if value[0] & 0x80 != 0 || value.iter().all(|byte| *byte == 0) {
        return Err(Error::invalid(format!("{name} must be positive")));
    }
    if value.len() > 1 && value[0] == 0 && value[1] & 0x80 == 0 {
        return Err(Error::invalid(format!("{name} is not minimally encoded")));
    }
    Ok(())
}

fn mpint_bits(value: &[u8]) -> usize {
    let value = value.strip_prefix(&[0]).unwrap_or(value);
    value
        .first()
        .map(|first| value.len() * 8 - first.leading_zeros() as usize)
        .unwrap_or(0)
}

fn require_length(value: &[u8], expected: usize, name: &str) -> Result<()> {
    if value.len() != expected {
        return Err(Error::invalid(format!("invalid {name} length")));
    }
    Ok(())
}

fn require_nonempty(value: &[u8], name: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::invalid(format!("{name} cannot be empty")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(value: &[u8]) -> Vec<u8> {
        let mut encoded = (value.len() as u32).to_be_bytes().to_vec();
        encoded.extend_from_slice(value);
        encoded
    }

    fn key_line(algorithm: &str, fields: &[&[u8]]) -> String {
        let mut blob = field(algorithm.as_bytes());
        for value in fields {
            blob.extend(field(value));
        }
        format!(
            "{algorithm} {} test comment",
            general_purpose::STANDARD.encode(blob)
        )
    }

    #[test]
    fn parses_ed25519_and_strips_comment() {
        let line = key_line("ssh-ed25519", &[&[7; 32]]);
        let parsed = parse_public_key(&line).unwrap();

        assert_eq!(parsed.algorithm, "ssh-ed25519");
        assert_eq!(parsed.comment, "test comment");
        assert_eq!(parsed.normalized.split_whitespace().count(), 2);
        assert!(!parsed.fingerprint_sha256.contains('='));
    }

    #[test]
    fn accepts_2048_bit_rsa_and_rejects_weak_rsa() {
        let exponent = [1, 0, 1];
        let mut strong_modulus = vec![0; 257];
        strong_modulus[1] = 0x80;
        parse_public_key(&key_line("ssh-rsa", &[&exponent, &strong_modulus])).unwrap();

        let mut weak_modulus = vec![0; 129];
        weak_modulus[1] = 0x80;
        let error =
            parse_public_key(&key_line("ssh-rsa", &[&exponent, &weak_modulus])).unwrap_err();
        assert!(error.to_string().contains("2048"));
    }

    #[test]
    fn rejects_mismatched_algorithm_and_trailing_data() {
        let mismatched =
            key_line("ssh-ed25519", &[&[7; 32]]).replacen("ssh-ed25519", "ecdsa-sha2-nistp256", 1);
        assert!(parse_public_key(&mismatched).is_err());

        let trailing = key_line("ssh-ed25519", &[&[7; 32], b"extra"]);
        assert!(parse_public_key(&trailing).is_err());
    }
}
