//! Embedded SSH server (DESIGN.md §8.2).
//!
//! Public-key auth against the ssh_keys table; exec-only, three whitelisted
//! git commands; channel bytes piped to/from the spawned git process.

use rgit_core::auth::authorize_repo;
use rgit_core::models::{Project, User};
use rgit_core::perm::RepoAction;
use rgit_core::state::AppState;
use rgit_git::protocol::Service;
use russh::keys::ssh_key::{self, rand_core::OsRng, Algorithm, HashAlg, LineEnding, PrivateKey};
use russh::server::{Auth, Handler, Msg, Server, Session};
use russh::{Channel, ChannelId, MethodSet};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn run(state: AppState) -> anyhow::Result<()> {
    let host_key = load_or_generate_host_key(&state)?;

    let config = Arc::new(russh::server::Config {
        methods: MethodSet::PUBLICKEY,
        keys: vec![host_key],
        auth_rejection_time: std::time::Duration::from_secs(1),
        auth_rejection_time_initial: Some(std::time::Duration::ZERO),
        inactivity_timeout: Some(std::time::Duration::from_secs(3600)),
        ..Default::default()
    });

    let addr = state.config.ssh.bind;
    tracing::info!(%addr, "SSH server listening");
    let mut server = GitSshServer { state };
    server
        .run_on_address(config, addr)
        .await
        .map_err(anyhow::Error::from)
}

/// data/ssh/host_ed25519 — generated on first start (DESIGN.md §8.2).
fn load_or_generate_host_key(state: &AppState) -> anyhow::Result<PrivateKey> {
    let dir = &state.config.ssh.host_key_dir;
    std::fs::create_dir_all(dir)?;
    let path = dir.join("host_ed25519");
    if path.exists() {
        let pem = std::fs::read_to_string(&path)?;
        Ok(PrivateKey::from_openssh(&pem)?)
    } else {
        let key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519)?;
        let pem = key.to_openssh(LineEnding::LF)?;
        std::fs::write(&path, pem.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        tracing::info!(?path, "generated SSH host key (ed25519)");
        Ok(key)
    }
}

struct GitSshServer {
    state: AppState,
}

impl Server for GitSshServer {
    type Handler = ClientSession;

    fn new_client(&mut self, peer: Option<std::net::SocketAddr>) -> ClientSession {
        ClientSession {
            state: self.state.clone(),
            peer,
            user: None,
            key_id: None,
            channels: HashMap::new(),
        }
    }
}

struct ClientSession {
    state: AppState,
    peer: Option<std::net::SocketAddr>,
    /// Set after successful public-key auth.
    user: Option<User>,
    key_id: Option<i64>,
    /// Session channels awaiting their exec request.
    channels: HashMap<ChannelId, Channel<Msg>>,
}

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error(transparent)]
    Russh(#[from] russh::Error),
    #[error("{0}")]
    Internal(#[from] anyhow::Error),
}

impl ClientSession {
    /// "git-upload-pack '<ns>/<proj>.git'" → (service, ns, proj)
    fn parse_command(raw: &[u8]) -> Option<(Service, String, String)> {
        let cmd = std::str::from_utf8(raw).ok()?.trim();
        let (name, rest) = match cmd.split_once(' ') {
            Some((n, r)) => (n, r.trim()),
            None => return None,
        };
        // Accept both "git-upload-pack" and "git upload-pack".
        let (name, rest) = if name == "git" {
            let (sub, r) = rest.split_once(' ')?;
            (format!("git-{sub}"), r.trim())
        } else {
            (name.to_string(), rest)
        };
        let service = Service::from_wire(&name)?;

        let path = rest.trim_matches(|c| c == '\'' || c == '"');
        let path = path.strip_prefix('/').unwrap_or(path);
        let path = path.strip_suffix(".git").unwrap_or(path);
        let (ns, proj) = path.split_once('/')?;
        if proj.contains('/') || ns.is_empty() || proj.is_empty() {
            return None; // exactly two segments
        }
        Some((service, ns.to_string(), proj.to_string()))
    }

    async fn authorize(
        &self,
        service: Service,
        ns: &str,
        proj: &str,
    ) -> Result<Project, String> {
        let user = self.user.as_ref().ok_or("not authenticated")?;
        let nsrow = sqlx::query_as::<_, rgit_core::models::Namespace>(
            "SELECT * FROM namespaces WHERE path = ?1",
        )
        .bind(ns)
        .fetch_optional(&self.state.db)
        .await
        .map_err(|_| "internal error")?
        .ok_or("repository not found")?;
        let project = sqlx::query_as::<_, Project>(
            "SELECT * FROM projects WHERE namespace_id = ?1 AND path = ?2",
        )
        .bind(nsrow.id)
        .bind(proj)
        .fetch_optional(&self.state.db)
        .await
        .map_err(|_| "internal error")?
        .ok_or("repository not found")?;

        let action = match service {
            Service::ReceivePack => RepoAction::Write,
            _ => RepoAction::Read,
        };
        authorize_repo(&self.state.db, Some(user), &project, action)
            .await
            .map_err(|_| match action {
                RepoAction::Write if project.archived => "project is archived (read-only)",
                _ => "access denied",
            })?;
        Ok(project)
    }
}

#[async_trait::async_trait]
impl Handler for ClientSession {
    type Error = SshError;

    async fn auth_publickey(
        &mut self,
        _user: &str,
        key: &ssh_key::PublicKey,
    ) -> Result<Auth, Self::Error> {
        // GitLab-format fingerprint: unpadded base64, no "SHA256:" prefix.
        let fp = key.fingerprint(HashAlg::Sha256).to_string();
        let fp = fp.strip_prefix("SHA256:").unwrap_or(&fp);

        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT id, user_id FROM ssh_keys WHERE fingerprint_sha256 = ?1",
        )
        .bind(fp)
        .fetch_optional(&self.state.db)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

        let Some((key_id, user_id)) = row else {
            tracing::info!(peer = ?self.peer, "SSH auth: unknown key");
            return Ok(Auth::Reject { proceed_with_methods: None });
        };
        let user = sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE id = ?1 AND state = 'active'",
        )
        .bind(user_id)
        .fetch_optional(&self.state.db)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

        match user {
            Some(user) => {
                sqlx::query("UPDATE ssh_keys SET last_used_at = datetime('now') WHERE id = ?1")
                    .bind(key_id)
                    .execute(&self.state.db)
                    .await
                    .ok();
                tracing::info!(peer = ?self.peer, user = %user.username, "SSH auth ok");
                self.user = Some(user);
                self.key_id = Some(key_id);
                Ok(Auth::Accept)
            }
            None => Ok(Auth::Reject { proceed_with_methods: None }),
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        self.channels.insert(channel.id(), channel);
        Ok(true)
    }

    async fn exec_request(
        &mut self,
        channel_id: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let handle = session.handle();
        let Some(channel) = self.channels.remove(&channel_id) else {
            session.channel_failure(channel_id)?;
            return Ok(());
        };

        let Some((service, ns, proj)) = Self::parse_command(data) else {
            let _ = handle
                .data(channel_id, "rgit: only git commands are supported\n".into())
                .await;
            session.channel_failure(channel_id)?;
            return Ok(());
        };

        let project = match self.authorize(service, &ns, &proj).await {
            Ok(p) => p,
            Err(msg) => {
                let _ = handle
                    .extended_data(channel_id, 1, format!("rgit: {msg}\n").into())
                    .await;
                session.channel_failure(channel_id)?;
                let _ = handle.close(channel_id).await;
                return Ok(());
            }
        };

        let repo = rgit_core::storage::repo_path(&self.state.config.storage, &project.disk_hash);
        let mut child =
            rgit_git::protocol::spawn_ssh_service(&self.state.config.git, service, &repo, None)
                .map_err(|e| anyhow::anyhow!(e))?;

        session.channel_success(channel_id)?;

        // Wire channel ⇆ child, then report exit status.
        let state = self.state.clone();
        let project_id = project.id;
        tokio::spawn(async move {
            let mut stdin = child.stdin.take().expect("stdin piped");
            let mut stdout = child.stdout.take().expect("stdout piped");
            let mut stream = channel.into_stream();

            let mut client_buf = [0u8; 32 * 1024];
            let mut git_buf = [0u8; 32 * 1024];
            let mut client_open = true;
            let mut git_open = true;
            while git_open {
                tokio::select! {
                    r = stream.read(&mut client_buf), if client_open => match r {
                        Ok(0) => {
                            client_open = false;
                            let _ = stdin.shutdown().await;
                        }
                        Ok(n) => {
                            if stdin.write_all(&client_buf[..n]).await.is_err() {
                                client_open = false;
                            }
                        }
                        Err(_) => break,
                    },
                    r = stdout.read(&mut git_buf) => match r {
                        Ok(0) => git_open = false,
                        Ok(n) => {
                            if stream.write_all(&git_buf[..n]).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => git_open = false,
                    },
                }
            }
            drop(stdin);
            let status = child.wait().await;
            let code = status.as_ref().ok().and_then(|s| s.code()).unwrap_or(1) as u32;

            let _ = handle.exit_status_request(channel_id, code).await;
            let _ = handle.eof(channel_id).await;
            let _ = handle.close(channel_id).await;

            if service == Service::ReceivePack && code == 0 {
                if let Err(e) = update_after_push(&state, project_id).await {
                    tracing::warn!(error = %e, project_id, "post-receive update failed");
                }
            }
        });

        Ok(())
    }
}

async fn update_after_push(state: &AppState, project_id: i64) -> anyhow::Result<()> {
    let project = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = ?1")
        .bind(project_id)
        .fetch_one(&state.db)
        .await?;
    let repo = rgit_core::storage::repo_path(&state.config.storage, &project.disk_hash);
    let head = rgit_git::repo::head_branch(&state.config.git, &repo).await?;
    sqlx::query(
        "UPDATE projects SET default_branch = ?1, updated_at = datetime('now') WHERE id = ?2",
    )
    .bind(head)
    .bind(project_id)
    .execute(&state.db)
    .await?;
    Ok(())
}
