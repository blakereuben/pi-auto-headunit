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
}
