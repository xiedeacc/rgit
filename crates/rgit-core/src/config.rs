//! Layered configuration: built-in defaults → rgit.toml (cwd) → $RGIT_CONFIG
//! file → environment variables (prefix `RGIT__`, `__` as section separator).
//!
//! Mirrors the rblog configuration conventions. See conf/rgit.example.toml
//! for the documented reference file.

use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    #[serde(default)]
    pub http: HttpConfig,
    #[serde(default)]
    pub ssh: SshConfig,
    #[serde(default)]
    pub db: DbConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub git: GitConfig,
    #[serde(default)]
    pub web: WebConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub lfs: LfsConfig,
    #[serde(default)]
    pub log: LogConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HttpConfig {
    /// Address the HTTP server binds to. nginx terminates TLS and proxies here.
    pub bind: SocketAddr,
    /// Public base URL (scheme + host), used to build clone/LFS URLs.
    pub external_url: String,
    /// Max in-memory JSON body size (bytes). Git/LFS bodies are streamed, not bounded by this.
    pub max_json_body: usize,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8000".parse().unwrap(),
            external_url: "http://localhost:8000".into(),
            max_json_body: 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SshConfig {
    pub enabled: bool,
    pub bind: SocketAddr,
    /// Directory holding server host keys; generated on first start if absent.
    pub host_key_dir: PathBuf,
    /// Host shown in displayed `git@host:path.git` clone URLs.
    pub clone_host: String,
    /// Port shown in displayed ssh clone URLs (may differ from bind port
    /// when NAT/nginx stream forwarding is in front).
    pub clone_port: u16,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: "0.0.0.0:2222".parse().unwrap(),
            host_key_dir: "data/ssh".into(),
            clone_host: "localhost".into(),
            clone_port: 2222,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DbConfig {
    /// SQLite database file path.
    pub path: PathBuf,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self { path: "data/rgit.db".into() }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct StorageConfig {
    /// Root of git repositories, GitLab hashed-storage layout:
    /// `<repositories>/@hashed/aa/bb/<sha256>.git`
    pub repositories: PathBuf,
    /// Root of LFS objects, GitLab layout: `<lfs_objects>/aa/bb/<oid[4..]>`
    pub lfs_objects: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            repositories: "data/repositories".into(),
            lfs_objects: "data/lfs-objects".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GitConfig {
    /// Path to the git binary.
    pub bin: String,
    /// Hard timeout for spawned git processes (seconds).
    pub timeout_secs: u64,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self { bin: "git".into(), timeout_secs: 3600 }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WebConfig {
    /// Directory of built Flutter web assets; served at `/`.
    pub static_dir: PathBuf,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self { static_dir: "web/build/web".into() }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AuthConfig {
    /// Session lifetime in hours (cookie + server side).
    pub session_ttl_hours: i64,
    /// bcrypt cost for new password hashes (GitLab-compatible verification
    /// is independent of this — it reads the cost from the stored hash).
    pub bcrypt_cost: u32,
    /// Whether self-service signup is open. Small-team default: admin creates users.
    pub signup_enabled: bool,
    pub min_password_length: usize,
    /// Consecutive failures before temporary lockout.
    pub max_login_failures: u32,
    pub lockout_minutes: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            session_ttl_hours: 24 * 14,
            bcrypt_cost: 12,
            signup_enabled: false,
            min_password_length: 10,
            max_login_failures: 10,
            lockout_minutes: 15,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LfsConfig {
    pub enabled: bool,
    /// Max single LFS object size in bytes; 0 = unlimited.
    pub max_file_size: u64,
}

impl Default for LfsConfig {
    fn default() -> Self {
        Self { enabled: true, max_file_size: 0 }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LogConfig {
    /// tracing filter, e.g. "info" or "rgit=debug,sqlx=warn".
    pub level: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self { level: "info".into() }
    }
}

impl AppConfig {
    /// Load configuration with layering:
    /// defaults → ./rgit.toml → $RGIT_CONFIG → env `RGIT__SECTION__KEY`.
    pub fn load() -> anyhow::Result<Self> {
        let mut builder = config::Config::builder()
            .add_source(config::File::with_name("rgit").required(false));

        if let Ok(path) = std::env::var("RGIT_CONFIG") {
            builder = builder.add_source(config::File::with_name(&path).required(true));
        }

        builder = builder.add_source(
            config::Environment::with_prefix("RGIT")
                .prefix_separator("__")
                .separator("__"),
        );

        let cfg: AppConfig = builder.build()?.try_deserialize()?;
        Ok(cfg)
    }
}
