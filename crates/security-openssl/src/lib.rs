//! Linux OpenSSL implementation of the replaceable AAP TLS client boundary.
//!
//! Credentials are injected by the application or packaging layer. This crate
//! intentionally does not embed the shared certificate and private key found in
//! AASDK.

// Portions derived from AASDK's OpenSSL memory-BIO behaviour.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{
    CredentialSummary, EphemeralCredentials, OpenSslTlsClient, OpenSslTlsError, TlsVersionPolicy,
    generate_ephemeral_credentials, validate_credential_pair,
};
