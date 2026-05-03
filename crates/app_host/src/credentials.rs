use std::path::{Path, PathBuf};

use keyring::Entry;
use serde::{Deserialize, Serialize};
use state_store::{AuthAccountRecord, ProviderKind};

const KEYRING_SERVICE_PREFIX: &str = "com.branchforge.auth";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
struct AuthMetadata {
    #[serde(default)]
    accounts: Vec<AuthAccountMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AuthAccountMetadata {
    host: String,
    provider: Option<ProviderKind>,
    username: String,
}

pub struct StoredCredential {
    pub host: String,
    pub username: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SecretBackend {
    NativeKeyring,
    File { dir: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialVault {
    metadata_path: PathBuf,
    backend: SecretBackend,
}

impl CredentialVault {
    pub fn with_overrides(
        cwd: &Path,
        metadata_path: Option<&Path>,
        file_store: Option<&Path>,
    ) -> Self {
        let metadata_path = metadata_path
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("BRANCHFORGE_AUTH_METADATA").map(PathBuf::from))
            .unwrap_or_else(|| auth_metadata_path_for(cwd));
        let backend = file_store
            .map(|dir| SecretBackend::File {
                dir: dir.to_path_buf(),
            })
            .or_else(|| {
                std::env::var_os("BRANCHFORGE_AUTH_FILE_STORE")
                    .map(PathBuf::from)
                    .map(|dir| SecretBackend::File { dir })
            })
            .unwrap_or(SecretBackend::NativeKeyring);
        Self {
            metadata_path,
            backend,
        }
    }

    #[cfg(test)]
    pub fn file_backed(metadata_path: PathBuf, token_dir: PathBuf) -> Self {
        Self {
            metadata_path,
            backend: SecretBackend::File { dir: token_dir },
        }
    }

    pub fn store_token(
        &self,
        host: &str,
        username: &str,
        provider: Option<ProviderKind>,
        token: &str,
    ) -> Result<AuthAccountRecord, String> {
        let host = normalize_required("host", host)?;
        let username = normalize_required("username", username)?;
        if token.is_empty() {
            return Err("token cannot be empty".to_string());
        }
        let provider = provider.or_else(|| provider_from_host(&host));
        self.write_secret(&host, &username, token)?;

        let mut metadata = self.load_metadata()?;
        if let Some(account) = metadata
            .accounts
            .iter_mut()
            .find(|account| account.host == host && account.username == username)
        {
            account.provider = provider.clone();
        } else {
            metadata.accounts.push(AuthAccountMetadata {
                host: host.clone(),
                provider: provider.clone(),
                username: username.clone(),
            });
        }
        metadata.accounts.sort_by(|left, right| {
            left.host
                .cmp(&right.host)
                .then(left.username.cmp(&right.username))
        });
        self.save_metadata(&metadata)?;

        Ok(AuthAccountRecord {
            host,
            provider,
            username: Some(username),
            token_present: true,
        })
    }

    pub fn delete_token(&self, host: &str, username: Option<&str>) -> Result<Vec<String>, String> {
        let host = normalize_required("host", host)?;
        let mut metadata = self.load_metadata()?;
        let mut removed = Vec::new();
        let mut retained = Vec::new();

        for account in metadata.accounts {
            let username_matches = username
                .map(|raw| account.username == raw.trim())
                .unwrap_or(true);
            if account.host == host && username_matches {
                self.delete_secret(&account.host, &account.username)?;
                removed.push(account.username);
            } else {
                retained.push(account);
            }
        }

        if removed.is_empty() {
            return Err("no matching stored credential was found".to_string());
        }

        metadata.accounts = retained;
        self.save_metadata(&metadata)?;
        Ok(removed)
    }

    pub fn list_accounts(&self) -> Result<Vec<AuthAccountRecord>, String> {
        let metadata = self.load_metadata()?;
        Ok(metadata
            .accounts
            .into_iter()
            .map(|account| {
                let token_present = self
                    .read_secret(&account.host, &account.username)
                    .map(|token| !token.is_empty())
                    .unwrap_or(false);
                AuthAccountRecord {
                    host: account.host,
                    provider: account.provider,
                    username: Some(account.username),
                    token_present,
                }
            })
            .collect())
    }

    pub fn token_for_host(
        &self,
        host: &str,
        username: Option<&str>,
    ) -> Result<StoredCredential, String> {
        let host = normalize_required("host", host)?;
        let username = username.map(str::trim).filter(|value| !value.is_empty());
        let metadata = self.load_metadata()?;
        let account = metadata
            .accounts
            .into_iter()
            .find(|account| {
                account.host == host
                    && username
                        .map(|target| account.username == target)
                        .unwrap_or(true)
            })
            .ok_or_else(|| format!("no stored credential for {host}"))?;
        let token = self.read_secret(&account.host, &account.username)?;
        Ok(StoredCredential {
            host: account.host,
            username: account.username,
            token,
        })
    }

    fn load_metadata(&self) -> Result<AuthMetadata, String> {
        if !self.metadata_path.exists() {
            return Ok(AuthMetadata::default());
        }
        let raw = std::fs::read_to_string(&self.metadata_path)
            .map_err(|err| format!("{}: {err}", self.metadata_path.display()))?;
        serde_json::from_str(&raw).map_err(|err| format!("invalid auth metadata: {err}"))
    }

    fn save_metadata(&self, metadata: &AuthMetadata) -> Result<(), String> {
        if let Some(parent) = self.metadata_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        let raw = serde_json::to_string_pretty(metadata).map_err(|err| err.to_string())?;
        std::fs::write(&self.metadata_path, raw)
            .map_err(|err| format!("{}: {err}", self.metadata_path.display()))
    }

    fn write_secret(&self, host: &str, username: &str, token: &str) -> Result<(), String> {
        match &self.backend {
            SecretBackend::NativeKeyring => Entry::new(&service_name(host), username)
                .map_err(|err| format!("keyring entry failed: {err:?}"))?
                .set_password(token)
                .map_err(|err| format!("keyring write failed: {err:?}")),
            SecretBackend::File { dir } => {
                std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
                std::fs::write(secret_file_path(dir, host, username), token)
                    .map_err(|err| format!("file credential write failed: {err}"))
            }
        }
    }

    fn read_secret(&self, host: &str, username: &str) -> Result<String, String> {
        match &self.backend {
            SecretBackend::NativeKeyring => Entry::new(&service_name(host), username)
                .map_err(|err| format!("keyring entry failed: {err:?}"))?
                .get_password()
                .map_err(|err| format!("keyring read failed: {err:?}")),
            SecretBackend::File { dir } => {
                std::fs::read_to_string(secret_file_path(dir, host, username))
                    .map_err(|err| format!("file credential read failed: {err}"))
            }
        }
    }

    fn delete_secret(&self, host: &str, username: &str) -> Result<(), String> {
        match &self.backend {
            SecretBackend::NativeKeyring => {
                match Entry::new(&service_name(host), username)
                    .map_err(|err| format!("keyring entry failed: {err:?}"))?
                    .delete_credential()
                {
                    Ok(()) => Ok(()),
                    Err(err) => {
                        let text = format!("{err:?}");
                        if text.contains("NoEntry") {
                            Ok(())
                        } else {
                            Err(format!("keyring delete failed: {text}"))
                        }
                    }
                }
            }
            SecretBackend::File { dir } => {
                let path = secret_file_path(dir, host, username);
                match std::fs::remove_file(&path) {
                    Ok(()) => Ok(()),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(err) => Err(format!("{}: {err}", path.display())),
                }
            }
        }
    }
}

pub fn auth_metadata_path_for(cwd: &Path) -> PathBuf {
    cwd.join("target/tmp/branchforge-auth/accounts.json")
}

pub fn provider_from_host(host: &str) -> Option<ProviderKind> {
    match host.to_ascii_lowercase().as_str() {
        "github.com" => Some(ProviderKind::GitHub),
        "gitlab.com" => Some(ProviderKind::GitLab),
        _ => None,
    }
}

fn normalize_required(label: &str, value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        Err(format!("{label} cannot be empty"))
    } else {
        Ok(normalized)
    }
}

fn service_name(host: &str) -> String {
    format!("{KEYRING_SERVICE_PREFIX}.{}", sanitize_segment(host))
}

fn secret_file_path(dir: &Path, host: &str, username: &str) -> PathBuf {
    dir.join(format!(
        "{}__{}.token",
        sanitize_segment(host),
        sanitize_segment(username)
    ))
}

fn sanitize_segment(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "credential".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("branchforge-auth-{prefix}-{nanos}"))
    }

    #[test]
    fn file_backed_vault_stores_metadata_without_token() {
        let root = unique_temp_dir("store");
        let vault = CredentialVault::file_backed(root.join("accounts.json"), root.join("tokens"));
        let record = vault
            .store_token(
                "GitHub.com",
                "Octo",
                Some(ProviderKind::GitHub),
                "ghp_secret",
            )
            .expect("store token");

        assert_eq!(record.host, "github.com");
        assert!(record.token_present);
        let metadata = std::fs::read_to_string(root.join("accounts.json")).expect("metadata");
        assert!(metadata.contains("github.com"));
        assert!(!metadata.contains("ghp_secret"));

        let stored = vault
            .token_for_host("github.com", Some("octo"))
            .expect("stored token");
        assert_eq!(stored.token, "ghp_secret");
    }

    #[test]
    fn delete_token_removes_matching_account() {
        let root = unique_temp_dir("delete");
        let vault = CredentialVault::file_backed(root.join("accounts.json"), root.join("tokens"));
        vault
            .store_token(
                "gitlab.com",
                "dev",
                Some(ProviderKind::GitLab),
                "glpat_secret",
            )
            .expect("store token");

        let removed = vault
            .delete_token("gitlab.com", Some("dev"))
            .expect("delete token");
        assert_eq!(removed, vec!["dev".to_string()]);
        assert!(vault.list_accounts().expect("accounts").is_empty());
    }
}
