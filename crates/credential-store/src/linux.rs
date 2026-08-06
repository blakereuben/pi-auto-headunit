use security_openssl::{CredentialSummary, validate_credential_pair};
use serde::Deserialize;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const MAX_PEM_SIZE: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CredentialConfig {
    pub certificate_path: PathBuf,
    pub private_key_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialPaths {
    pub certificate: PathBuf,
    pub private_key: PathBuf,
}

impl From<CredentialConfig> for CredentialPaths {
    fn from(config: CredentialConfig) -> Self {
        Self {
            certificate: config.certificate_path,
            private_key: config.private_key_path,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialStatus {
    pub summary: CredentialSummary,
    pub private_key_mode: u32,
}

#[derive(Debug)]
pub enum CredentialError {
    Io(io::Error),
    Config(String),
    InvalidFile(String),
    InvalidCredentials(String),
    InsecurePrivateKeyPermissions(u32),
    Missing(PathBuf),
    AlreadyInstalled(PathBuf),
}

impl fmt::Display for CredentialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O: {error}"),
            Self::Config(error) => write!(formatter, "configuration: {error}"),
            Self::InvalidFile(error) => write!(formatter, "invalid credential file: {error}"),
            Self::InvalidCredentials(error) => write!(formatter, "credential validation: {error}"),
            Self::InsecurePrivateKeyPermissions(mode) => write!(
                formatter,
                "private key permissions {mode:04o} allow group or other access; require 0600"
            ),
            Self::Missing(path) => write!(
                formatter,
                "credentials are not configured: missing {}",
                path.display()
            ),
            Self::AlreadyInstalled(path) => write!(
                formatter,
                "credential already exists at {}; remove or rotate it explicitly",
                path.display()
            ),
        }
    }
}

impl std::error::Error for CredentialError {}

impl From<io::Error> for CredentialError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Deserialize)]
struct RootConfig {
    credentials: CredentialConfig,
}

pub fn load_config(path: &Path) -> Result<CredentialConfig, CredentialError> {
    let text = fs::read_to_string(path)?;
    let config = toml::from_str::<RootConfig>(&text)
        .map(|root| root.credentials)
        .map_err(|error| CredentialError::Config(error.to_string()))?;
    if !config.certificate_path.is_absolute() || !config.private_key_path.is_absolute() {
        return Err(CredentialError::Config(
            "credential paths must be absolute".into(),
        ));
    }
    Ok(config)
}

pub fn validate_credentials(
    paths: &CredentialPaths,
    require_secure_permissions: bool,
) -> Result<CredentialStatus, CredentialError> {
    let certificate = read_regular_bounded_file(&paths.certificate, "certificate")?;
    let private_key = read_regular_bounded_file(&paths.private_key, "private key")?;
    let mode = fs::metadata(&paths.private_key)?.permissions().mode() & 0o777;
    if require_secure_permissions && mode & 0o077 != 0 {
        return Err(CredentialError::InsecurePrivateKeyPermissions(mode));
    }
    let summary = validate_credential_pair(&certificate, &private_key)
        .map_err(|error| CredentialError::InvalidCredentials(error.to_string()))?;
    drop(private_key);
    Ok(CredentialStatus {
        summary,
        private_key_mode: mode,
    })
}

pub fn install_credentials(
    source: &CredentialPaths,
    destination: &CredentialPaths,
) -> Result<CredentialStatus, CredentialError> {
    validate_credentials(source, true)?;
    if destination.certificate.exists() {
        return Err(CredentialError::AlreadyInstalled(
            destination.certificate.clone(),
        ));
    }
    if destination.private_key.exists() {
        return Err(CredentialError::AlreadyInstalled(
            destination.private_key.clone(),
        ));
    }
    let certificate_parent = destination.certificate.parent().ok_or_else(|| {
        CredentialError::InvalidFile("certificate destination has no parent directory".into())
    })?;
    let key_parent = destination.private_key.parent().ok_or_else(|| {
        CredentialError::InvalidFile("private-key destination has no parent directory".into())
    })?;
    if certificate_parent != key_parent {
        return Err(CredentialError::InvalidFile(
            "certificate and private key must share a destination directory".into(),
        ));
    }
    fs::create_dir_all(certificate_parent)?;
    fs::set_permissions(certificate_parent, fs::Permissions::from_mode(0o700))?;

    let certificate = read_regular_bounded_file(&source.certificate, "certificate")?;
    let private_key = read_regular_bounded_file(&source.private_key, "private key")?;
    write_new_file(&destination.certificate, &certificate, 0o644)?;
    if let Err(error) = write_new_file(&destination.private_key, &private_key, 0o600) {
        let _ = fs::remove_file(&destination.certificate);
        return Err(error);
    }
    drop(private_key);
    validate_credentials(destination, true)
}

fn read_regular_bounded_file(path: &Path, label: &str) -> Result<Vec<u8>, CredentialError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(CredentialError::Missing(path.to_path_buf()));
        }
        Err(error) => return Err(CredentialError::Io(error)),
    };
    if !metadata.file_type().is_file() {
        return Err(CredentialError::InvalidFile(format!(
            "{label} must be a regular file"
        )));
    }
    if metadata.len() == 0 || metadata.len() > MAX_PEM_SIZE {
        return Err(CredentialError::InvalidFile(format!(
            "{label} must be between 1 byte and {MAX_PEM_SIZE} bytes"
        )));
    }
    Ok(fs::read(path)?)
}

fn write_new_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), CredentialError> {
    let parent = path.parent().ok_or_else(|| {
        CredentialError::InvalidFile("credential destination has no parent directory".into())
    })?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            CredentialError::InvalidFile("credential destination has no file name".into())
        })?;
    let temporary = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temporary)?;
    if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(CredentialError::Io(error));
    }
    drop(file);
    if let Err(error) = fs::set_permissions(&temporary, fs::Permissions::from_mode(mode)) {
        let _ = fs::remove_file(&temporary);
        return Err(CredentialError::Io(error));
    }
    if let Err(error) = fs::hard_link(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return if error.kind() == io::ErrorKind::AlreadyExists {
            Err(CredentialError::AlreadyInstalled(path.to_path_buf()))
        } else {
            Err(CredentialError::Io(error))
        };
    }
    fs::remove_file(temporary)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use security_openssl::generate_ephemeral_credentials;
    use tempfile::tempdir;

    fn write_source(directory: &Path) -> CredentialPaths {
        let credentials = generate_ephemeral_credentials().expect("synthetic credentials");
        let paths = CredentialPaths {
            certificate: directory.join("source.crt"),
            private_key: directory.join("source.key"),
        };
        fs::write(&paths.certificate, credentials.certificate_pem).expect("certificate");
        fs::write(&paths.private_key, credentials.private_key_pem).expect("private key");
        fs::set_permissions(&paths.private_key, fs::Permissions::from_mode(0o600))
            .expect("private-key permissions");
        paths
    }

    #[test]
    fn loads_paths_from_config() {
        let directory = tempdir().expect("temporary directory");
        let config_path = directory.path().join("config.toml");
        fs::write(
            &config_path,
            "[credentials]\ncertificate_path = '/one/headunit.crt'\nprivate_key_path = '/one/headunit.key'\n",
        )
        .expect("configuration");
        let config = load_config(&config_path).expect("valid configuration");
        assert_eq!(config.certificate_path, Path::new("/one/headunit.crt"));
        assert_eq!(config.private_key_path, Path::new("/one/headunit.key"));
    }

    #[test]
    fn rejects_relative_config_paths() {
        let directory = tempdir().expect("temporary directory");
        let config_path = directory.path().join("config.toml");
        fs::write(
            &config_path,
            "[credentials]\ncertificate_path = 'relative.crt'\nprivate_key_path = 'relative.key'\n",
        )
        .expect("configuration");
        assert!(matches!(
            load_config(&config_path),
            Err(CredentialError::Config(_))
        ));
    }

    #[test]
    fn validates_synthetic_matching_pair() {
        let directory = tempdir().expect("temporary directory");
        let paths = write_source(directory.path());
        let status = validate_credentials(&paths, true).expect("valid credentials");
        assert_eq!(status.private_key_mode, 0o600);
    }

    #[test]
    fn rejects_group_readable_private_key() {
        let directory = tempdir().expect("temporary directory");
        let paths = write_source(directory.path());
        fs::set_permissions(&paths.private_key, fs::Permissions::from_mode(0o640))
            .expect("private-key permissions");
        assert!(matches!(
            validate_credentials(&paths, true),
            Err(CredentialError::InsecurePrivateKeyPermissions(0o640))
        ));
    }

    #[test]
    fn reports_missing_credentials_as_not_configured() {
        let directory = tempdir().expect("temporary directory");
        let paths = CredentialPaths {
            certificate: directory.path().join("missing.crt"),
            private_key: directory.path().join("missing.key"),
        };
        let error = validate_credentials(&paths, true).expect_err("credentials must be absent");
        assert!(error.to_string().contains("not configured"));
    }

    #[test]
    fn installs_without_overwriting() {
        let source_directory = tempdir().expect("source directory");
        let destination_directory = tempdir().expect("destination directory");
        let source = write_source(source_directory.path());
        let destination = CredentialPaths {
            certificate: destination_directory.path().join("installed/headunit.crt"),
            private_key: destination_directory.path().join("installed/headunit.key"),
        };
        let status = install_credentials(&source, &destination).expect("installation");
        assert_eq!(status.private_key_mode, 0o600);
        assert!(matches!(
            install_credentials(&source, &destination),
            Err(CredentialError::AlreadyInstalled(_))
        ));
    }
}
