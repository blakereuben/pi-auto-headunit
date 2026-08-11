// Portions derived from AASDK's replaceable cryptor boundary.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TlsProgress {
    pub outbound: Vec<u8>,
    pub complete: bool,
}

pub trait TlsClient {
    type Error: std::error::Error + Send + Sync + 'static;

    fn start(&mut self) -> Result<TlsProgress, Self::Error>;

    fn feed(&mut self, inbound: &[u8]) -> Result<TlsProgress, Self::Error>;

    /// Encrypts `plaintext` and returns the TLS record bytes to place in an
    /// AAP frame payload. Requires the handshake to be complete.
    fn encrypt_application_data(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, Self::Error>;

    /// Feeds ciphertext (TLS record bytes taken from an AAP frame payload)
    /// and returns all plaintext currently available. May return an empty
    /// `Vec` if no complete record has arrived yet; this is not an error.
    /// Does not assume one call's input maps to exactly one output chunk.
    fn decrypt_application_data(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, Self::Error>;
}
