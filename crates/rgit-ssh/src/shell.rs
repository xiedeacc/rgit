//! OpenSSH forced-command entry point, analogous to gitlab-shell.

use base64::Engine;
use rgit_core::auth::authorize_repo;
use rgit_core::config::AppConfig;
use rgit_core::models::{Namespace, Project, User};
use rgit_core::perm::RepoAction;
use rgit_git::protocol::Service;
use std::path::Path;

const LFS_TOKEN_TTL_SECONDS: i64 = 3600;

#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("only Git commands are supported")]
    InvalidCommand,
    #[error("repository not found")]
    RepositoryNotFound,
    #[error("access denied")]
    AccessDenied,
    #[error("project is archived (read-only)")]
    Archived,
    #[error("internal error")]
    Internal(#[source] anyhow::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCommand {
    pub action: ShellAction,
    pub namespace: String,
    pub project: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellAction {
    Git(Service),
    Lfs { can_write: bool },
}

impl ShellAction {
    fn repo_action(self) -> RepoAction {
        match self {
            Self::Git(Service::ReceivePack) | Self::Lfs { can_write: true } => RepoAction::Write,
            _ => RepoAction::Read,
        }
    }
}

pub fn parse_command(raw: &str) -> Result<ShellCommand, ShellError> {
    let command = raw.trim();
    let (name, argument) = command
        .split_once(char::is_whitespace)
        .ok_or(ShellError::InvalidCommand)?;
    let (name, argument) = if name == "git" {
        let (subcommand, argument) = argument
            .trim_start()
            .split_once(char::is_whitespace)
            .ok_or(ShellError::InvalidCommand)?;
        (format!("git-{subcommand}"), argument.trim())
    } else {
        (name.to_string(), argument.trim())
    };
    let (action, path) = if name == "git-lfs-authenticate" {
        let (path, operation) = split_lfs_arguments(argument)?;
        let can_write = match operation {
            "upload" => true,
            "download" => false,
            _ => return Err(ShellError::InvalidCommand),
        };
        (ShellAction::Lfs { can_write }, path)
    } else {
        let service = Service::from_wire(&name).ok_or(ShellError::InvalidCommand)?;
        (
            ShellAction::Git(service),
            unquote_single_argument(argument)?,
        )
    };
    let (namespace, project) = parse_repository_path(path)?;
    Ok(ShellCommand {
        action,
        namespace: namespace.to_string(),
        project: project.to_string(),
    })
}

fn parse_repository_path(path: &str) -> Result<(&str, &str), ShellError> {
    let path = path.strip_prefix('/').unwrap_or(path);
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (namespace, project) = path.split_once('/').ok_or(ShellError::InvalidCommand)?;
    if namespace.is_empty()
        || project.is_empty()
        || project.contains('/')
        || [namespace, project]
            .iter()
            .any(|part| part.chars().any(|c| c.is_control() || c == '\0'))
    {
        return Err(ShellError::InvalidCommand);
    }
    Ok((namespace, project))
}

fn split_lfs_arguments(argument: &str) -> Result<(&str, &str), ShellError> {
    let argument = argument.trim();
    if let Some(quote) = argument
        .as_bytes()
        .first()
        .copied()
        .filter(|q| *q == b'\'' || *q == b'"')
    {
        let rest = &argument[1..];
        let end = rest.find(quote as char).ok_or(ShellError::InvalidCommand)?;
        let path = &rest[..end];
        let operation = rest[end + 1..].trim();
        if operation.chars().any(char::is_whitespace) || operation.is_empty() {
            return Err(ShellError::InvalidCommand);
        }
        return Ok((path, operation));
    }
    let mut parts = argument.split_whitespace();
    let path = parts.next().ok_or(ShellError::InvalidCommand)?;
    let operation = parts.next().ok_or(ShellError::InvalidCommand)?;
    if parts.next().is_some() || path.contains(['\'', '"', '\\', ';', '`', '$']) {
        return Err(ShellError::InvalidCommand);
    }
    Ok((path, operation))
}

fn unquote_single_argument(argument: &str) -> Result<&str, ShellError> {
    if argument.len() >= 2 {
        let first = argument.as_bytes()[0];
        let last = argument.as_bytes()[argument.len() - 1];
        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            let inner = &argument[1..argument.len() - 1];
            if inner.contains(first as char) {
                return Err(ShellError::InvalidCommand);
            }
            return Ok(inner);
        }
    }
    if argument.chars().any(char::is_whitespace)
        || argument.contains(['\'', '"', '\\', ';', '`', '$'])
    {
        return Err(ShellError::InvalidCommand);
    }
    Ok(argument)
}

pub async fn run(config_path: &Path, key_id: i64, original: &str) -> Result<i32, ShellError> {
    let config = AppConfig::load_file(config_path).map_err(internal)?;
    if !config.db.path.is_file() {
        return Err(ShellError::Internal(anyhow::anyhow!("database is missing")));
    }
    let db = rgit_core::db::connect(&config.db).await.map_err(internal)?;
    let command = parse_command(original)?;

    let user = sqlx::query_as::<_, User>(
        r#"
        SELECT u.* FROM users u
        JOIN ssh_keys k ON k.user_id = u.id
        WHERE k.id = ?1 AND u.state = 'active'
        "#,
    )
    .bind(key_id)
    .fetch_optional(&db)
    .await
    .map_err(internal)?
    .ok_or(ShellError::AccessDenied)?;

    let namespace = sqlx::query_as::<_, Namespace>("SELECT * FROM namespaces WHERE path = ?1")
        .bind(&command.namespace)
        .fetch_optional(&db)
        .await
        .map_err(internal)?
        .ok_or(ShellError::RepositoryNotFound)?;
    let project = sqlx::query_as::<_, Project>(
        "SELECT * FROM projects WHERE namespace_id = ?1 AND path = ?2",
    )
    .bind(namespace.id)
    .bind(&command.project)
    .fetch_optional(&db)
    .await
    .map_err(internal)?
    .ok_or(ShellError::RepositoryNotFound)?;

    let action = command.action.repo_action();
    authorize_repo(&db, Some(&user), &project, action)
        .await
        .map_err(|_| {
            if action == RepoAction::Write && project.archived {
                ShellError::Archived
            } else {
                ShellError::AccessDenied
            }
        })?;

    sqlx::query("UPDATE ssh_keys SET last_used_at = datetime('now') WHERE id = ?1")
        .bind(key_id)
        .execute(&db)
        .await
        .map_err(internal)?;

    let code = match command.action {
        ShellAction::Git(service) => {
            let repository = rgit_core::storage::repo_path(&config.storage, &project.disk_hash);
            let protocol = std::env::var("GIT_PROTOCOL").ok();
            let status = rgit_git::protocol::run_ssh_service_stdio(
                &config.git,
                service,
                &repository,
                protocol.as_deref(),
            )
            .await
            .map_err(|error| ShellError::Internal(anyhow::Error::from(error)))?;
            if status.success() && service == Service::ReceivePack {
                update_after_push(&config, &db, &project).await?;
            }
            status.code().unwrap_or(1)
        }
        ShellAction::Lfs { can_write } => {
            if !config.lfs.enabled || !project.lfs_enabled {
                return Err(ShellError::RepositoryNotFound);
            }
            let token = rgit_core::auth::lfs_token::issue(
                &db,
                user.id,
                project.id,
                can_write,
                LFS_TOKEN_TTL_SECONDS,
            )
            .await
            .map_err(|error| ShellError::Internal(anyhow::Error::from(error)))?;
            let basic = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{token}", user.username));
            let href = format!(
                "{}/{}/{}.git/info/lfs",
                config.http.external_url.trim_end_matches('/'),
                command.namespace,
                project.path
            );
            println!(
                "{}",
                serde_json::json!({
                    "header": {"Authorization": format!("Basic {basic}")},
                    "href": href,
                    "expires_in": LFS_TOKEN_TTL_SECONDS,
                })
            );
            0
        }
    };
    db.close().await;
    Ok(code)
}

async fn update_after_push(
    config: &AppConfig,
    db: &sqlx::SqlitePool,
    project: &Project,
) -> Result<(), ShellError> {
    let repository = rgit_core::storage::repo_path(&config.storage, &project.disk_hash);
    let head = rgit_git::repo::head_branch(&config.git, &repository)
        .await
        .map_err(|error| ShellError::Internal(anyhow::Error::from(error)))?;
    sqlx::query(
        "UPDATE projects SET default_branch = ?1, updated_at = datetime('now') WHERE id = ?2",
    )
    .bind(head)
    .bind(project.id)
    .execute(db)
    .await
    .map_err(internal)?;
    Ok(())
}

fn internal(error: impl Into<anyhow::Error>) -> ShellError {
    ShellError::Internal(error.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_whitelisted_git_commands() {
        let parsed = parse_command("git-upload-pack 'team/repo.git'").unwrap();
        assert_eq!(parsed.action, ShellAction::Git(Service::UploadPack));
        assert_eq!(parsed.namespace, "team");
        assert_eq!(parsed.project, "repo");
        assert_eq!(
            parse_command("git upload-archive \"team/repo.git\"")
                .unwrap()
                .action,
            ShellAction::Git(Service::UploadArchive)
        );
        assert_eq!(
            parse_command("git-lfs-authenticate 'team/repo.git' upload")
                .unwrap()
                .action,
            ShellAction::Lfs { can_write: true }
        );
        assert_eq!(
            parse_command("git-lfs-authenticate team/repo.git download")
                .unwrap()
                .action,
            ShellAction::Lfs { can_write: false }
        );
        assert!(parse_command("rm -rf /").is_err());
        assert!(parse_command("git-upload-pack 'team/repo.git'; id").is_err());
        assert!(parse_command("git-upload-pack 'a/b/c.git'").is_err());
        assert!(parse_command("git-lfs-authenticate team/repo.git delete").is_err());
        assert!(parse_command("git-lfs-authenticate 'team/repo.git' upload;id").is_err());
    }
}
