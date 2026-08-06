use credential_store::{
    CredentialError, CredentialPaths, install_credentials, load_config, validate_credentials,
};
use std::path::{Path, PathBuf};

const DEFAULT_CONFIG_PATH: &str = "/etc/aa-headunit/config.toml";
const DEFAULT_CERTIFICATE_PATH: &str = "/etc/aa-headunit/credentials/headunit.crt";
const DEFAULT_PRIVATE_KEY_PATH: &str = "/etc/aa-headunit/credentials/headunit.key";

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
