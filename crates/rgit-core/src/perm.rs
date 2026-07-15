//! Access levels and visibility — same integer constants as GitLab so the
//! migration is a straight copy.

use serde::{Deserialize, Serialize};

/// Project/group membership level. Values match GitLab's Gitlab::Access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(i32)]
pub enum AccessLevel {
    Guest = 10,
    Reporter = 20,
    Developer = 30,
    Maintainer = 40,
    Owner = 50,
}

impl AccessLevel {
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            10 => Some(Self::Guest),
            20 => Some(Self::Reporter),
            30 => Some(Self::Developer),
            40 => Some(Self::Maintainer),
            50 => Some(Self::Owner),
            _ => None,
        }
    }
}

/// Values match GitLab's Gitlab::VisibilityLevel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(i32)]
pub enum Visibility {
    Private = 0,
    Internal = 10,
    Public = 20,
}

impl Visibility {
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Private),
            10 => Some(Self::Internal),
            20 => Some(Self::Public),
            _ => None,
        }
    }
}

/// What a (possibly anonymous) caller may do on a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoAction {
    /// Browse code, clone/fetch, download archives and LFS objects.
    Read,
    /// Push (git-receive-pack), upload LFS objects.
    Write,
    /// Change settings, members, delete/archive/transfer.
    Admin,
}

/// Minimum membership level required for an action on a *private* project.
pub fn required_level(action: RepoAction) -> AccessLevel {
    match action {
        RepoAction::Read => AccessLevel::Guest,
        RepoAction::Write => AccessLevel::Developer,
        RepoAction::Admin => AccessLevel::Maintainer,
    }
}
