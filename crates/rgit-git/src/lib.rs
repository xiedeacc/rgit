//! rgit-git — git repository management and protocol plumbing.
//!
//! All git operations shell out to the system `git` binary (like gitolite /
//! gitlab-shell); no libgit2. Repository layout on disk is GitLab
//! hashed-storage compatible (see rgit-core::storage).

pub mod protocol;
pub mod read;
pub mod repo;
