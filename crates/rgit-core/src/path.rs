//! URL path-segment validation shared by HTTP, SSH, and auto-create flows.

use crate::{Error, Result};

/// Reserved first path segments that can never be namespaces.
pub const RESERVED_ROOTS: &[&str] = &[
    "api", "admin", "login", "settings", "groups", "assets", "-", "explore",
];

pub fn is_reserved_root(s: &str) -> bool {
    RESERVED_ROOTS.contains(&s)
}

/// Path-segment policy (DESIGN.md §7.5): alnum start, then [A-Za-z0-9_.-];
/// no ".."/"."; no reserved suffixes; not starting with '@' (reserved for
/// storage prefixes like @hashed).
pub fn validate_path_segment(s: &str) -> Result<()> {
    let ok_chars = s
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
    let ok = !s.is_empty()
        && s.len() <= 255
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && ok_chars
        && s != "."
        && s != ".."
        && !s.starts_with('@')
        && !s.ends_with(".git")
        && !s.ends_with(".wiki")
        && !s.ends_with(".atom");
    if ok {
        Ok(())
    } else {
        Err(Error::invalid("invalid path segment"))
    }
}
