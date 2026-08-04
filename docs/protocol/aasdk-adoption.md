# AASDK Adoption Record

## Decision

The project owner approved AASDK as a GPLv3-compatible source for Android Auto Protocol behaviour on 4 August 2026. OpenAuto remains excluded as a code source. Protocol work will use Rust by default and preserve AASDK attribution wherever behaviour or definitions are derived.

## Pinned upstream

- Repository: https://github.com/opencardev/aasdk
- Branch reviewed: `newdev`
- Revision: `9bf6adf933665dee26532201719fac14a047ccf1`
- Upstream lineage: fork of https://github.com/f1xpl/aasdk
- Licence notices in the adopted framing files: GNU GPL version 3 or later
- Copyright notices in the adopted framing files: 2018 f1x.studio (Michal Szwaj); 2024 CubeOne (Simon Dean)

The maintained repository README links to a root `LICENSE` file that is absent at the pinned revision. Adoption therefore relies on the explicit GPL-3.0-or-later notices in each source file used, not the broken README link.

## First derived scope

The Rust `protocol-aap` framing implementation is derived from these files:

- `include/aasdk/Messenger/FrameHeader.hpp`
- `include/aasdk/Messenger/FrameType.hpp`
- `include/aasdk/Messenger/EncryptionType.hpp`
- `include/aasdk/Messenger/MessageType.hpp`
- `include/aasdk/Messenger/FrameSize.hpp`
- `include/aasdk/Messenger/FrameSizeType.hpp`
- `include/aasdk/Messenger/MessageOutStream.hpp`
- `src/Messenger/FrameHeader.cpp`
- `src/Messenger/FrameSize.cpp`
- `src/Messenger/MessageInStream.cpp`
- `src/Messenger/MessageOutStream.cpp`

Derived facts currently cover the two-byte frame header, flag layout, big-endian short/extended sizes, first-frame total size, the `0x4000` frame payload limit, and per-channel fragment reassembly. The Rust implementation adds stricter reserved-bit validation, independent bounded total-message/concurrent-channel limits, and rejects restarted, incomplete, inconsistent, or metadata-changing fragment sequences.

## Expansion rule

Before another AASDK behaviour, schema, identifier, certificate, or asset is used, add its exact upstream path and purpose here, verify its file-level licence/copyright notice, and update third-party notices. No OpenAuto file may be used to fill a gap.
