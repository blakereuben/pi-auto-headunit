# TLS Credential Policy

## Current decision

The project will not copy, embed, present, or redistribute credentials, certificates, private keys, authentication material, or security bypasses from OpenAuto, OpenAuto Pro, AASDK, or another head-unit implementation. GPL compatibility for source code does not make third-party identity material appropriate or legitimate to reuse.

## Verified source behaviour

At pinned AASDK revision `9bf6adf933665dee26532201719fac14a047ccf1`, `Cryptor.cpp` loads a fixed certificate and matching RSA private key into an OpenSSL client context. `SSLWrapper.cpp` uses a client connection over memory BIOs and disables peer-certificate verification. Both files carry GPL-3.0-or-later notices and are listed in the adoption record.

The certificate subject names JVC Kenwood and its issuer names Google Automotive Link. Its paired private key is already public in AASDK, so it must not be treated as a repository secret. Public availability and GPL licensing do not by themselves establish that presenting or redistributing the named identity is appropriate for this independent project.

## Implemented boundary

`security-openssl` accepts certificate and private-key PEM bytes at runtime, checks that they match, and drives OpenSSL through bounded in-memory transport. Tests generate temporary credentials at runtime. The repository contains no compatibility credential. Bounded phone probes reached TLS over both USB/AOA and the official developer tunnel, with the negative results below.

A deterministic mutual-TLS test now uses separate, synthetic client and server identities. The server requires and verifies the generated client certificate, and both sides complete the handshake. This confirms that the adapter presents its configured certificate and correctly completes the memory-buffer handshake when the peer trusts that identity. It does not provide or emulate Android Auto trust and does not weaken the live-probe lockout.

The diagnostic now contains an explicit `usb tls-probe` path that generates a fresh keypair in memory for each invocation. It cannot run without `--allow-live-aap`, rejects an already-accessory-mode phone, and stops before authentication completion and service discovery. Passing native tests does not count as a live interoperability result.

## Product integration rule

Production Android Auto authentication must use a legitimate project-owned or officially provisioned route. Until such a route is available:

1. do not attempt to bypass Android Auto's security decision;
2. do not treat a commercial product's functionality as permission to copy its internal material;
3. use fake peers for continued protocol, media, UI, and transport development; the official DHU may be used only as an external reference because the custom client was also rejected through the developer tunnel;
4. keep production wired projection marked blocked by authentication rather than claiming compatibility;
5. pursue an official partner/certification route if a distributable production receiver requires provisioning unavailable to independent projects.

No credential experiments may log PEM material, decrypted traffic, phone identifiers, or user content.

A narrowly controlled bench handshake with temporary project-generated credentials was used to determine whether an independent identity is accepted. The result is conclusive and the experiment must not be repeated. It remained opt-in, stopped before authentication completion and service discovery, sent no media/channel traffic, and recorded only sanitized states.

## First bench result

On 4 August 2026, the Pi 5 probe reached an accepted AAP 1.6 version response and then timed out before TLS completed. No authentication-complete or service-discovery response was sent. The TLS-1.2 comparison received peer TLS data, and the phone displayed Android Auto error 7 stating that the software had not met its security requirement. Together, these observations establish that the temporary project-generated identity was rejected.

A separate `--tls12-compat` mode is now available because the pinned AASDK source explicitly selects `TLSv1_2_client_method()` when built against OpenSSL older than 1.1. The ordinary probe retains AASDK's newer `TLS_client_method()` behaviour. Comparing these modes is source-backed diagnosis, not an inferred protocol requirement.

The repository will not repeat the generated-identity experiment and will not test AASDK's public shared compatibility certificate/key. The implementation remains useful as a bounded, replaceable TLS engine and negative interoperability fixture, but no third-party credential will be added later merely to make the phone accept the software.

## Developer-tunnel result

The user enabled Android Auto developer mode and started the documented head-unit server on a Samsung Galaxy S23 Ultra. The Pi used ADB forwarding on loopback TCP port 5277. The project sent the same bounded version/TLS probe with fresh credentials: Android Auto accepted protocol version 1.6, returned TLS peer data, closed the connection, and displayed error 7 on the phone. No authentication-complete message, service discovery, channel setup, media, identifiers, or user content was sent or logged.

This proves the developer transport works through the first protocol/TLS exchange, but it also proves that developer mode is not an identity bypass for this client. The temporary ADB forward was removed after the result, and the restarted phone server was not probed again.

## Connection feasibility decision

The evidence separates protocol transport from receiver authorisation:

- USB/AOA and the documented developer tunnel both reach AAP 1.6 and receive peer TLS data.
- The synthetic verifier test proves that the TLS adapter can present a client identity and complete mutual TLS when that identity is trusted.
- Android Auto rejects the independently generated identity on both live transports before TLS completion.

No remaining pre-service protocol or OpenSSL state-machine difference has been identified in the approved AASDK paths. The release connection gate therefore requires legitimate receiver provisioning accepted by Android Auto. Until an official route supplies that authorisation, wired and wireless Android Auto interoperability remain blocked; service, media, and UI work cannot turn an unrecognised identity into an accepted receiver.

Both live generated-identity CLI paths now fail before opening USB or TCP transport, even when their former explicit opt-in flag is supplied. Automated coverage enforces this permanent lockout.
