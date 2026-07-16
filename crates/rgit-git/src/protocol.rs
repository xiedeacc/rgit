//! git smart protocol plumbing (stateless-rpc), shared by HTTP and SSH fronts.
//!
//! HTTP flow (DESIGN.md §8.1):
//!   GET  info/refs?service=X  → pkt-line "# service=X" + flush + `X --advertise-refs`
//!   POST git-upload-pack / git-receive-pack → body → stdin, stdout → response

use rgit_core::config::GitConfig;
use rgit_core::{Error, Result};
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use tokio::process::{Child, Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    UploadPack,
    ReceivePack,
    UploadArchive,
}

impl Service {
    /// The wire name, e.g. "git-upload-pack".
    pub fn name(self) -> &'static str {
        match self {
            Service::UploadPack => "git-upload-pack",
            Service::ReceivePack => "git-receive-pack",
            Service::UploadArchive => "git-upload-archive",
        }
    }

    /// git subcommand name, e.g. "upload-pack".
    pub fn subcommand(self) -> &'static str {
        match self {
            Service::UploadPack => "upload-pack",
            Service::ReceivePack => "receive-pack",
            Service::UploadArchive => "upload-archive",
        }
    }

    pub fn from_wire(name: &str) -> Option<Self> {
        match name {
            "git-upload-pack" => Some(Service::UploadPack),
            "git-receive-pack" => Some(Service::ReceivePack),
            "git-upload-archive" => Some(Service::UploadArchive),
            _ => None,
        }
    }
}

/// Encode one pkt-line.
pub fn pkt_line(data: &str) -> Vec<u8> {
    let mut out = format!("{:04x}", data.len() + 4).into_bytes();
    out.extend_from_slice(data.as_bytes());
    out
}

pub const FLUSH_PKT: &[u8] = b"0000";

/// Spawn `git <service> --stateless-rpc --advertise-refs <repo>`.
/// Caller streams: pkt_line("# service=...") + FLUSH + child stdout.
/// `git_protocol` is the client's `Git-Protocol` header value, passed
/// through as the GIT_PROTOCOL env (protocol v2 negotiation).
pub fn spawn_advertise_refs(
    cfg: &GitConfig,
    service: Service,
    repo: &Path,
    git_protocol: Option<&str>,
) -> Result<Child> {
    spawn(
        cfg,
        service,
        repo,
        &["--stateless-rpc", "--advertise-refs"],
        git_protocol,
    )
}

/// Spawn `git <service> --stateless-rpc <repo>`; caller wires stdin/stdout.
pub fn spawn_stateless_rpc(
    cfg: &GitConfig,
    service: Service,
    repo: &Path,
    git_protocol: Option<&str>,
) -> Result<Child> {
    spawn(cfg, service, repo, &["--stateless-rpc"], git_protocol)
}

/// Spawn the plain (bidirectional) service for the SSH transport.
pub fn spawn_ssh_service(
    cfg: &GitConfig,
    service: Service,
    repo: &Path,
    git_protocol: Option<&str>,
) -> Result<Child> {
    spawn(cfg, service, repo, &[], git_protocol)
}

/// Run an SSH service with stdio inherited from an OpenSSH forced command.
pub async fn run_ssh_service_stdio(
    cfg: &GitConfig,
    service: Service,
    repo: &Path,
    git_protocol: Option<&str>,
) -> Result<ExitStatus> {
    let repo = repo
        .to_str()
        .ok_or_else(|| Error::invalid("non-utf8 repository path"))?;
    let mut command = Command::new(&cfg.bin);
    command
        .arg(service.subcommand())
        .arg(repo)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    set_git_protocol(&mut command, git_protocol);
    let mut child = command
        .spawn()
        .map_err(|error| Error::Git(format!("failed to spawn {}: {error}", service.name())))?;
    wait_with_timeout(cfg, &mut child).await
}

/// Wait for a streaming git service with the configured hard timeout.
/// A timed-out child is killed before returning so it cannot outlive the
/// request or SSH channel that spawned it.
pub async fn wait_with_timeout(cfg: &GitConfig, child: &mut Child) -> Result<ExitStatus> {
    let timeout = std::time::Duration::from_secs(cfg.timeout_secs);
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(result) => result.map_err(|e| Error::Git(format!("waiting for git failed: {e}"))),
        Err(_) => {
            let _ = child.kill().await;
            Err(Error::Git(format!(
                "git service timed out after {}s",
                cfg.timeout_secs
            )))
        }
    }
}

fn spawn(
    cfg: &GitConfig,
    service: Service,
    repo: &Path,
    extra: &[&str],
    git_protocol: Option<&str>,
) -> Result<Child> {
    let repo = repo
        .to_str()
        .ok_or_else(|| Error::invalid("non-utf8 repository path"))?;

    let mut cmd = Command::new(&cfg.bin);
    cmd.arg(service.subcommand())
        .args(extra)
        .arg(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    set_git_protocol(&mut cmd, git_protocol);
    cmd.spawn()
        .map_err(|e| Error::Git(format!("failed to spawn {}: {e}", service.name())))
}

fn set_git_protocol(command: &mut Command, git_protocol: Option<&str>) {
    if let Some(protocol) = git_protocol {
        // Sanitized pass-through of the client's protocol request
        // (e.g. "version=2"); upload-pack honors it, receive-pack ignores it.
        if protocol.len() <= 64 && protocol.bytes().all(|byte| byte.is_ascii_graphic()) {
            command.env("GIT_PROTOCOL", protocol);
        }
    }
}
