# TLS Credential Policy

## Current decision

The project may implement AASDK-derived TLS behaviour, but it will not embed, present, or redistribute AASDK's shared certificate/private-key pair until its compatibility and identity implications have been reviewed and explicitly approved.

## Verified source behaviour

At pinned AASDK revision `9bf6adf933665dee26532201719fac14a047ccf1`, `Cryptor.cpp` loads a fixed certificate and matching RSA private key into an OpenSSL client context. `SSLWrapper.cpp` uses a client connection over memory BIOs and disables peer-certificate verification. Both files carry GPL-3.0-or-later notices and are listed in the adoption record.

The certificate subject names JVC Kenwood and its issuer names Google Automotive Link. Its paired private key is already public in AASDK, so it must not be treated as a repository secret. Public availability and GPL licensing do not by themselves establish that presenting or redistributing the named identity is appropriate for this independent project.

## Implemented boundary

`security-openssl` accepts certificate and private-key PEM bytes at runtime, checks that they match, and drives OpenSSL through bounded in-memory transport. Tests generate temporary credentials at runtime. The repository contains no compatibility credential, and the backend is not yet connected to a phone session.

The diagnostic now contains an explicit `usb tls-probe` path that generates a fresh keypair in memory for each invocation. It cannot run without `--allow-live-aap`, rejects an already-accessory-mode phone, and stops before authentication completion and service discovery. Passing native tests does not count as a live interoperability result.

## Gate for product integration

Before normal phone-session integration or redistribution of compatibility credentials, record:

1. whether the AASDK pair is technically required or an independently generated identity is accepted, based on an approved source or controlled interoperability test;
2. the legal/trademark rationale for any third-party-named certificate that would be presented or distributed;
3. the package location, permissions, replacement mechanism, and disclosure that the key is a public compatibility credential rather than a secret;
4. the security consequence of disabled peer verification and any safer compatible alternative;
5. exact provenance and notices for whichever credential material is selected.

No credential experiments may log PEM material, decrypted traffic, phone identifiers, or user content.

A narrowly controlled bench handshake with temporary project-generated credentials may be used to determine whether an independent identity is accepted. It must remain opt-in, stop at the first named handshake result, send no service-discovery response or media/channel traffic, and record only a sanitized success/failure state.

## First bench result

On 4 August 2026, the Pi 5 probe reached an accepted AAP 1.6 version response and then timed out before TLS completed. No authentication-complete or service-discovery response was sent. The TLS-1.2 comparison received peer TLS data, and the phone displayed Android Auto error 7 stating that the software had not met its security requirement. Together, these observations establish that the temporary project-generated identity was rejected.

A separate `--tls12-compat` mode is now available because the pinned AASDK source explicitly selects `TLSv1_2_client_method()` when built against OpenSSL older than 1.1. The ordinary probe retains AASDK's newer `TLS_client_method()` behaviour. Comparing these modes is source-backed diagnosis, not an inferred protocol requirement.

The repository will not repeat the generated-identity experiment. The next interoperability decision is whether to present AASDK's public shared compatibility certificate/key. That pair is GPL-covered source material but carries third-party identity names, so its use and redistribution require explicit project-owner approval and a documented independent-project disclaimer; legal certainty cannot be inferred from technical compatibility.
