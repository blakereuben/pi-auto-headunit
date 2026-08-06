//! Local credential provisioning for the Raspberry Pi OS runtime.
//!
//! The crate handles user-supplied file paths and permissions. It contains no
//! production certificate, private key, or receiver identity.

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{
    CredentialConfig, CredentialError, CredentialMaterial, CredentialPaths, CredentialStatus,
    LoadedCredentials, install_credentials, load_config, load_credentials, validate_credentials,
};
