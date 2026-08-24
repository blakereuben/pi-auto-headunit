use credential_store::{
    CredentialError, CredentialPaths, install_credentials, load_config, validate_credentials,
};
use std::path::{Path, PathBuf};

const DEFAULT_CONFIG_PATH: &str = "/etc/aa-headunit/config.toml";
/// Shared with [`crate::credentials_setup_wizard`], which installs to the
/// exact same destination the `credentials install` CLI command below
/// does — one default location, not two independently-maintained copies.
pub(crate) const DEFAULT_CERTIFICATE_PATH: &str = "/etc/aa-headunit/credentials/headunit.crt";
pub(crate) const DEFAULT_PRIVATE_KEY_PATH: &str = "/etc/aa-headunit/credentials/headunit.key";

/// Where [`crate::credentials_setup_wizard`] always stages a freshly
/// picked certificate/private-key pair, before anything actually
/// installs them to [`DEFAULT_CERTIFICATE_PATH`]/[`DEFAULT_PRIVATE_KEY_PATH`].
/// A plain, unprivileged path under the operator's own home directory —
/// deliberately not the real destination, since the wizard can run
/// before `/etc/aa-headunit` (and the group that owns it) exist at all
/// (`packaging/setup.sh`, operator's explicit direction 2026-08-24: run
/// the wizard first, have the install pick the result up). `None` only
/// if `$HOME` isn't set, which doesn't happen in a real desktop
/// session.
pub(crate) fn staging_paths() -> Option<CredentialPaths> {
    let home = std::env::var_os("HOME")?;
    let staging_dir = PathBuf::from(home).join(".local/share/aa-headunit/pending-credentials");
    Some(CredentialPaths {
        certificate: staging_dir.join("headunit.crt"),
        private_key: staging_dir.join("headunit.key"),
    })
}

/// Safety net: if a certificate/private-key pair is sitting in
/// [`staging_paths`] with nothing having adopted it yet — normally
/// `postinst` does this immediately after install
/// (`packaging/debian/aa-headunit-diagnostics.postinst`), so this is
/// only reached if that step was skipped (no active desktop session
/// detected at install time) or if `credentials setup` was run again
/// after the package was already installed — install it for real the
/// next time the app actually starts, rather than leaving it stranded.
/// Never fatal: any failure here just leaves the pair staged and
/// credentials still missing, exactly as if this had never run.
pub(crate) fn adopt_staged_credentials_if_present() {
    let Some(staged) = staging_paths() else {
        return;
    };
    if !staged.certificate.exists() || !staged.private_key.exists() {
        return;
    }
    let destination = CredentialPaths {
        certificate: PathBuf::from(DEFAULT_CERTIFICATE_PATH),
        private_key: PathBuf::from(DEFAULT_PRIVATE_KEY_PATH),
    };
    match install_credentials(&staged, &destination) {
        Ok(_) | Err(CredentialError::AlreadyInstalled(_)) => {
            let _ = std::fs::remove_file(&staged.certificate);
            let _ = std::fs::remove_file(&staged.private_key);
            if let Some(parent) = staged.certificate.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
        Err(_) => {
            // A real validation failure — leave the staged files in
            // place so `credentials setup`/`credentials check` can
            // still show the operator what's wrong, rather than
            // silently discarding them.
        }
    }
}

pub fn run(command: &str, args: &[String]) -> Result<(), CredentialError> {
    match command {
        "check" => {
            let source = parse_source_paths(args)?;
            print_status(&validate_credentials(&source, true)?);
        }
        "install" => {
            let source = parse_source_paths(args)?;
            let destination = CredentialPaths {
                certificate: PathBuf::from(DEFAULT_CERTIFICATE_PATH),
                private_key: PathBuf::from(DEFAULT_PRIVATE_KEY_PATH),
            };
            let status = install_credentials(&source, &destination)?;
            print_status(&status);
            println!("credential_installation=complete");
            println!("credential_certificate_path={DEFAULT_CERTIFICATE_PATH}");
            println!("credential_private_key_path={DEFAULT_PRIVATE_KEY_PATH}");
        }
        "status" => {
            let config_path = parse_config_path(args)?;
            let paths = CredentialPaths::from(load_config(&config_path)?);
            print_status(&validate_credentials(&paths, true)?);
            println!("credential_configuration={}", config_path.display());
        }
        _ => {
            return Err(CredentialError::Config(format!(
                "unknown credential command: {command}"
            )));
        }
    }
    Ok(())
}

fn parse_source_paths(args: &[String]) -> Result<CredentialPaths, CredentialError> {
    let mut certificate = None;
    let mut private_key = None;
    let mut index = 0;
    while index < args.len() {
        let flag = &args[index];
        let value = args
            .get(index + 1)
            .ok_or_else(|| CredentialError::Config(format!("{flag} requires a file path")))?;
        match flag.as_str() {
            "--certificate" if certificate.is_none() => {
                certificate = Some(PathBuf::from(value));
            }
            "--private-key" if private_key.is_none() => {
                private_key = Some(PathBuf::from(value));
            }
            _ => {
                return Err(CredentialError::Config(format!(
                    "unknown or repeated option: {flag}"
                )));
            }
        }
        index += 2;
    }
    Ok(CredentialPaths {
        certificate: certificate
            .ok_or_else(|| CredentialError::Config("--certificate is required".into()))?,
        private_key: private_key
            .ok_or_else(|| CredentialError::Config("--private-key is required".into()))?,
    })
}

fn parse_config_path(args: &[String]) -> Result<PathBuf, CredentialError> {
    match args {
        [] => Ok(PathBuf::from(DEFAULT_CONFIG_PATH)),
        [flag, path] if flag == "--config" => Ok(Path::new(path).to_path_buf()),
        _ => Err(CredentialError::Config(
            "status accepts only an optional --config PATH".into(),
        )),
    }
}

fn print_status(status: &credential_store::CredentialStatus) {
    println!("credential_pair=valid");
    println!("credential_certificate_time=valid");
    println!(
        "credential_private_key_mode={:04o}",
        status.private_key_mode
    );
    println!("credential_not_before={}", status.summary.not_before);
    println!("credential_not_after={}", status.summary.not_after);
    println!("credential_contents_logged=false");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_source_paths_in_either_order() {
        let paths = parse_source_paths(&[
            "--private-key".into(),
            "/tmp/key".into(),
            "--certificate".into(),
            "/tmp/cert".into(),
        ])
        .expect("valid paths");
        assert_eq!(paths.private_key, Path::new("/tmp/key"));
        assert_eq!(paths.certificate, Path::new("/tmp/cert"));
    }

    #[test]
    fn rejects_missing_or_repeated_paths() {
        assert!(parse_source_paths(&["--certificate".into(), "/tmp/cert".into()]).is_err());
        assert!(
            parse_source_paths(&[
                "--certificate".into(),
                "/tmp/one".into(),
                "--certificate".into(),
                "/tmp/two".into(),
            ])
            .is_err()
        );
    }
}
