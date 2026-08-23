# Wireless Bootstrap Security Review

Status: closes the security-review strand of `MILESTONE_CHECKLIST.md` M8's
"Complete security, dependency, licence, privacy, and protocol-provenance
reviews" for the wireless bootstrap specifically — the one genuinely new
attack surface introduced since the wired-only security posture was last
reviewed (the wired AAP session's own TLS/credential handling is
unchanged and already covered by `tls-credential-policy.md`). Evidence
below is read directly from the current source
(`apps/aa-headunit-diagnostics/src/wireless_bootstrap.rs`,
`crates/transport-bluetooth/src/lib.rs`) and a live `bluetoothctl show`
capture on this Pi, 2026-08-23 — not inferred.

## What's being reviewed

`usb kiosk`'s wireless fallback (`bootstrap_wireless_transport`) does two
things before any Android Auto protocol traffic exists at all: brings up
a Wi-Fi access point, and becomes Bluetooth-discoverable to hand a
connecting phone that AP's credentials over an RFCOMM channel (the `aaw`
protobuf exchange). Both happen with no operator interaction, every time
wireless is used — exactly the surface a passer-by could attempt to
interact with, not just the intended phone.

## Wi-Fi access point: strong

`hostapd_config` (`wireless_bootstrap.rs:554`) sets `wpa=2`,
`wpa_key_mgmt=WPA-PSK`, `rsn_pairwise=CCMP` — WPA2-Personal with the
modern AES-CCMP cipher, not the deprecated TKIP. The passphrase itself
(`random_hex(8)`, `wireless_bootstrap.rs:316`) is 8 bytes read from
`/dev/urandom` — a real CSPRNG, not a weak PRNG — rendered as 16 hex
characters, giving 64 bits of entropy. It's generated fresh for every
single bootstrap attempt (never reused across sessions or persisted to
disk) and the access point itself is torn down
(`WifiAccessPoint`'s `Drop`) the moment the attempt ends. A 64-bit random
WPA2 passphrase is far outside the range any realistic offline
handshake-capture attack recovers in a relevant timeframe, and even a
successful crack only outlives one single bootstrap attempt.

**No weakness found here.**

## Bluetooth RFCOMM handoff: intentionally unauthenticated, bounded

This is the one real, honest finding of this review, not a defect
introduced by accident.

Both Bluetooth profile registrations
(`transport-bluetooth/src/lib.rs:255-296`, the AA-Wireless SDP profile
and the Handsfree AG profile) set `require_authentication: Some(false)`
and `require_authorization: Some(false)`. `run_aaw_bootstrap`
(`wireless_bootstrap.rs:190`) then hands the Wi-Fi SSID/password to
*whatever RFCOMM peer connects and speaks the correct `aaw` protobuf
message sequence* — there is no check that the peer is a specific,
previously-known, or cryptographically-verified phone. Any
Bluetooth-capable device within range that discovers this device (it's
`set_discoverable(true)`, `crates/transport-bluetooth/src/lib.rs:209`)
during the bootstrap window and correctly implements the `aaw` handshake
receives the same WPA2 credentials the real phone would.

**Why this is accepted, not a defect to fix:**

1. **Time-bounded, not indefinite.** `set_discoverable`/`set_pairable`
   are never explicitly reset in this project's own code, but a live
   `bluetoothctl show` on this Pi confirms BlueZ's own adapter default
   governs it: `DiscoverableTimeout: 0x000000b4 (180)` — 180 seconds, the
   same order of magnitude as this project's own
   `BLUETOOTH_ACCEPT_TIMEOUT` (also 180s, `wireless_bootstrap.rs:71`).
   The window an attacker would need to be in range *and* already
   listening is a few minutes around when the head unit starts a
   wireless attempt, not indefinite.
2. **Bounded blast radius if exploited.** What's disclosed is a
   single-use Wi-Fi PSK for a private access point that exists only for
   the duration of one bootstrap attempt. Joining that network does not
   itself grant access to anything — the actual Android Auto session
   that follows is the same TLS-secured, credential-gated AAP protocol
   this project's wired path already uses unmodified
   (`docs/protocol/wireless-source-assessment.md`'s own framing: "an
   ordinary AAP session over TCP reusing this project's existing
   transport-agnostic TLS/protocol stack unmodified"). An attacker who
   joined the AP still cannot complete an Android Auto session without
   also defeating that separate, already-reviewed TLS/credential layer
   (`tls-credential-policy.md`).
3. **Matches the real product's own UX contract, not a shortcut this
   project invented.** Requiring PIN confirmation or manual pairing
   approval on every wireless-AA connection would break the "hands-off"
   reconnect behaviour that's an explicit product requirement
   ([[feedback_wireless_connect_must_be_hands_off]] — Blake's standing
   rule that a wireless bootstrap requiring the operator to watch their
   phone is a bug). Real production wireless-AA head units make the same
   trade-off for the same reason.

**Residual risk, recorded rather than silently accepted:** a
sufficiently determined, correctly-implemented rogue `aaw` peer in
Bluetooth range during the discoverable window could join the temporary
AP. This is a known, bounded, low-impact risk given points 1-2 above, not
a demonstrated exploit path into the actual Android Auto session or any
stored credential. If tightened later, the natural next step is
requiring the connecting Bluetooth device to already be OS-paired
(`is_paired()`, already used elsewhere in this same file for the active-
reconnect loop) before running the `aaw` exchange at all — deliberately
not done now, since that would also break wireless bootstrap for a
phone's very first connection, which has no prior pairing yet.

## Everything else: unchanged from the already-reviewed wired posture

Credential handling, TLS, and the AAP session itself are identical code
paths to the wired transport, already covered by
`tls-credential-policy.md` and unaffected by any of the above — the
wireless bootstrap only ever runs *before* that layer, to acquire a
transport, exactly like USB/AOA discovery does on the wired side.

## Conclusion

No fix is needed. The Wi-Fi leg is strong by construction; the Bluetooth
leg is a deliberate, bounded, product-justified trade-off, not an
oversight — recorded here so it's a documented decision rather than an
unexamined gap the next M8 pass would otherwise have to rediscover from
scratch.
