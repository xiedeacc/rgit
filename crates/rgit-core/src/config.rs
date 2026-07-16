//! Layered configuration: built-in defaults → rgit.toml (cwd) → $RGIT_CONFIG
//! file → environment variables (prefix `RGIT__`, `__` as section separator).
//!
//! Mirrors the rblog configuration conventions. See conf/rgit.example.toml
//! for the documented reference file.

use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize)]
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
    /// Trust X-Real-IP/X-Forwarded-For only when the direct peer is loopback.
    pub trust_forwarded_headers: bool,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8000".parse().unwrap(),
            external_url: "http://localhost:8000".into(),
            max_json_body: 1024 * 1024,
            trust_forwarded_headers: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SshConfig {
    pub enabled: bool,
    /// Host shown in displayed `git@host:path.git` clone URLs.
    pub clone_host: String,
    /// Port shown in displayed ssh clone URLs (may differ from bind port
    /// when NAT/nginx stream forwarding is in front).
    pub clone_port: u16,
    /// Derived OpenSSH key file, rebuilt atomically from SQLite.
    pub authorized_keys_file: PathBuf,
    /// Absolute command paths embedded in authorized_keys forced commands.
    pub shell_path: PathBuf,
    pub shell_config: PathBuf,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            clone_host: "localhost".into(),
            clone_port: 2222,
            authorized_keys_file: "data/ssh/authorized_keys".into(),
            shell_path: "/opt/usr/local/rgit/bin/rgit-shell".into(),
            shell_config: "/opt/usr/local/rgit/conf/rgit.toml".into(),
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
        Self {
            path: "data/rgit.db".into(),
        }
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
            repositories: "/zfs/gitlab_data/repositories".into(),
            lfs_objects: "/zfs/gitlab_data/lfs-objects".into(),
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
    /// Global cap across HTTP, SSH, archive, and repository browsing.
    pub max_concurrent_operations: usize,
    /// Maximum time a request waits for a Git operation slot.
    pub queue_timeout_secs: u64,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            bin: "git".into(),
            timeout_secs: 3600,
            max_concurrent_operations: 16,
            queue_timeout_secs: 30,
        }
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
        Self {
            static_dir: "web/build/web".into(),
        }
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
        Self {
            enabled: true,
            max_file_size: 0,
        }
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
        Self {
            level: "info".into(),
        }
    }
}

impl AppConfig {
    /// Load configuration with layering:
    /// defaults → ./rgit.toml → $RGIT_CONFIG → env `RGIT__SECTION__KEY`.
    pub fn load() -> anyhow::Result<Self> {
        let mut builder =
            config::Config::builder().add_source(config::File::with_name("rgit").required(false));

        if let Ok(path) = std::env::var("RGIT_CONFIG") {
            builder = builder.add_source(config::File::with_name(&path).required(true));
        }

        builder = builder.add_source(
            config::Environment::with_prefix("RGIT")
                .prefix_separator("__")
                .separator("__"),
        );

        let cfg: AppConfig = builder.build()?.try_deserialize()?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Load one exact config file without environment overrides. This is used
    /// by rgit-shell so SSH clients cannot influence server configuration.
    pub fn load_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let cfg: AppConfig = config::Config::builder()
            .add_source(config::File::from(path).required(true))
            .build()?
            .try_deserialize()?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.http.max_json_body >= 1024,
            "http.max_json_body must be at least 1024"
        );
        anyhow::ensure!(
            self.git.timeout_secs > 0,
            "git.timeout_secs must be greater than zero"
        );
        anyhow::ensure!(
            self.git.max_concurrent_operations > 0,
            "git.max_concurrent_operations must be greater than zero"
        );
        if self.ssh.enabled {
            for (name, path) in [
                ("ssh.shell_path", &self.ssh.shell_path),
                ("ssh.shell_config", &self.ssh.shell_config),
            ] {
                anyhow::ensure!(path.is_absolute(), "{name} must be an absolute path");
                let value = path.to_string_lossy();
                anyhow::ensure!(
                    !value.is_empty()
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_./:-".contains(&byte)),
                    "{name} contains characters unsafe for authorized_keys"
                );
            }
        }
        anyhow::ensure!(
            self.git.queue_timeout_secs > 0,
            "git.queue_timeout_secs must be greater than zero"
        );
        anyhow::ensure!(
            self.auth.session_ttl_hours > 0,
            "auth.session_ttl_hours must be positive"
        );
        anyhow::ensure!(
            (4..=31).contains(&self.auth.bcrypt_cost),
            "auth.bcrypt_cost must be 4..=31"
        );
        anyhow::ensure!(
            self.auth.min_password_length >= 10,
            "auth.min_password_length must be at least 10"
        );
        anyhow::ensure!(
            self.auth.max_login_failures > 0,
            "auth.max_login_failures must be positive"
        );
        anyhow::ensure!(
            self.auth.lockout_minutes > 0,
            "auth.lockout_minutes must be positive"
        );
        anyhow::ensure!(
            !self.http.trust_forwarded_headers || self.http.bind.ip().is_loopback(),
            "http.trust_forwarded_headers requires a loopback bind address"
        );

        let external = url::Url::parse(&self.http.external_url)
            .map_err(|error| anyhow::anyhow!("invalid http.external_url: {error}"))?;
        anyhow::ensure!(
            matches!(external.scheme(), "http" | "https"),
            "http.external_url must use http or https"
        );
        anyhow::ensure!(
            external.host_str().is_some(),
            "http.external_url must include a host"
        );
        anyhow::ensure!(
            external.username().is_empty() && external.password().is_none(),
            "http.external_url must not contain credentials"
        );
        anyhow::ensure!(
            external.path() == "/" && external.query().is_none() && external.fragment().is_none(),
            "http.external_url must not contain a path, query, or fragment"
        );
        if external.scheme() == "http" {
            let host = external.host_str().unwrap_or_default();
            let local = host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback());
            anyhow::ensure!(local, "non-local http.external_url must use https");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_insecure_public_url_and_untrusted_proxy_bind() {
        let mut config = AppConfig::default();
        config.http.external_url = "http://git.example.com".into();
        assert!(config.validate().is_err());

        config.http.external_url = "https://git.example.com".into();
        config.http.bind = "0.0.0.0:8000".parse().unwrap();
        config.http.trust_forwarded_headers = true;
        assert!(config.validate().is_err());
    }
}
