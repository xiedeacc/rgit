//! On-disk storage paths, byte-compatible with GitLab hashed storage.
//!
//! Repositories: `<repositories>/@hashed/<h[0..2]>/<h[2..4]>/<h>.git`
//! where `h = hex(sha256(disk_id.to_string()))` and `disk_id` is the project's
//! immutable numeric disk identifier (equal to the GitLab project id for
//! migrated projects, so migrated repositories are found in place).
//!
//! Wikis/design repositories (kept for data preservation, not served):
//! `<h>.wiki.git` / `<h>.design.git`.
//!
//! LFS objects: `<lfs_objects>/<oid[0..2]>/<oid[2..4]>/<oid[4..]>`
//! (oid = lowercase hex sha256 of content — identical to GitLab).

use crate::config::StorageConfig;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// hex(sha256(disk_id)) — the hashed-storage identifier of a project.
pub fn disk_hash(disk_id: i64) -> String {
    hex::encode(Sha256::digest(disk_id.to_string().as_bytes()))
}

/// Relative path (under the repositories root) of the bare repo for a hash.
pub fn repo_rel_path(hash: &str) -> String {
    format!("@hashed/{}/{}/{}.git", &hash[0..2], &hash[2..4], hash)
}

/// Absolute path of a project's bare repository.
pub fn repo_path(cfg: &StorageConfig, hash: &str) -> PathBuf {
    cfg.repositories.join(repo_rel_path(hash))
}

/// Absolute path of a project's wiki repository (preserved, not served).
pub fn wiki_path(cfg: &StorageConfig, hash: &str) -> PathBuf {
    cfg.repositories.join(format!(
        "@hashed/{}/{}/{}.wiki.git",
        &hash[0..2],
        &hash[2..4],
        hash
    ))
}

/// Absolute path of a project's design-management repository.
pub fn design_path(cfg: &StorageConfig, hash: &str) -> PathBuf {
    cfg.repositories.join(format!(
        "@hashed/{}/{}/{}.design.git",
        &hash[0..2],
        &hash[2..4],
        hash
    ))
}

/// Absolute path of an LFS object by oid (lowercase hex sha256).
pub fn lfs_path(cfg: &StorageConfig, oid: &str) -> PathBuf {
    cfg.lfs_objects
        .join(&oid[0..2])
        .join(&oid[2..4])
        .join(&oid[4..])
}

/// Validate an LFS oid: exactly 64 lowercase hex chars.
pub fn is_valid_lfs_oid(oid: &str) -> bool {
    oid.len() == 64 && oid.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn ensure_dirs(cfg: &StorageConfig) -> std::io::Result<()> {
    std::fs::create_dir_all(cfg.repositories.join("@hashed"))?;
    std::fs::create_dir_all(&cfg.lfs_objects)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cfg.repositories, std::fs::Permissions::from_mode(0o700))?;
        std::fs::set_permissions(&cfg.lfs_objects, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Remove upload fragments left by an interrupted process. A single rgit
/// instance owns the storage root, so no live upload exists during startup.
pub fn cleanup_lfs_temp(cfg: &StorageConfig) -> std::io::Result<usize> {
    let tmp = cfg.lfs_objects.join("tmp");
    std::fs::create_dir_all(&tmp)?;
    let mut removed = 0;
    for entry in std::fs::read_dir(tmp)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry.path().extension().and_then(|value| value.to_str()) == Some("part")
        {
            std::fs::remove_file(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashed_storage_matches_gitlab() {
        // GitLab: Digest::SHA2.hexdigest(project.id.to_s), sharded aa/bb.
        // sha256("1") well-known value:
        assert_eq!(
            disk_hash(1),
            "6b86b273ff34fce19d6b804eff5a3f5747ada4eaa22f1d49c01e52ddb7875b4b"
        );
        assert!(design_path(
            &StorageConfig {
                repositories: "/data/repositories".into(),
                lfs_objects: "/data/lfs-objects".into(),
            },
            &disk_hash(1)
        )
        .to_string_lossy()
        .ends_with(".design.git"));
        assert_eq!(
            repo_rel_path(&disk_hash(1)),
            "@hashed/6b/86/6b86b273ff34fce19d6b804eff5a3f5747ada4eaa22f1d49c01e52ddb7875b4b.git"
        );
    }

    #[test]
    fn lfs_sharding() {
        let cfg = StorageConfig {
            repositories: "/data/repositories".into(),
            lfs_objects: "/data/lfs-objects".into(),
        };
        let oid = "91eff75a492a3ed0dfcb544d7f31326bc4014c8551849c192fd1e48d4dd2c897";
        assert_eq!(
            lfs_path(&cfg, oid).to_str().unwrap(),
            "/data/lfs-objects/91/ef/f75a492a3ed0dfcb544d7f31326bc4014c8551849c192fd1e48d4dd2c897"
        );
    }

    #[test]
    fn cleanup_removes_only_lfs_upload_fragments() {
        let root = std::env::temp_dir().join(format!(
            "rgit-lfs-cleanup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cfg = StorageConfig {
            repositories: root.join("repositories"),
            lfs_objects: root.join("lfs-objects"),
        };
        let tmp = cfg.lfs_objects.join("tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("upload.part"), b"partial").unwrap();
        std::fs::write(tmp.join("keep.txt"), b"keep").unwrap();

        assert_eq!(cleanup_lfs_temp(&cfg).unwrap(), 1);
        assert!(!tmp.join("upload.part").exists());
        assert!(tmp.join("keep.txt").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
