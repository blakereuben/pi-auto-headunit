# `session-supervisor` reconnect race: investigation record

## Status: two real bugs found and fixed; one residual race left open, root
cause undetermined

Found while completing M4's "recover from unplug, phone rejection, timeout,
and service restart" checklist item (`MILESTONE_CHECKLIST.md`), specifically
while proving the kill/restart-mid-session sub-scenario with `usb
session-supervisor`. Running the supervisor unbounded (no
`AA_HEADUNIT_OBSERVATION_WINDOW_SECONDS` override, so each cycle runs its
default ~30s observation window) surfaced a previously-unknown, deterministic
pattern: the reconnect *immediately following a clean, successful* cycle
reliably collided with the phone — `protocol probe: encrypted frame received
before TLS handshake completed` — on roughly every other cycle, self-healing
automatically via the existing soft-reset escalation one cycle later.

## Two real, confirmed fixes (kept)

1. **The head unit never told the phone a session was ending.**
   `auth_discovery_probe::run` silently dropped the transport on a clean
   deadline-reached stop, with no protocol-level notice at all. Fixed:
   `send_byebye_request` now sends a `ByeByeRequest` (reason
   `UserSelection`) and waits (bounded, `BYEBYE_RESPONSE_WAIT`) for the
   phone's `ByeByeResponse` before returning. `crates/protocol-aap/src/byebye.rs`
   gained `encode_byebye_request` (previously receive-only) — confirmed
   against the pinned AASDK source (`src/Channel/Control/ControlServiceChannel.cpp`,
   `sendShutdownRequest`) that this is a legitimate, symmetric,
   head-unit-initiated message, not phone-only. Cross-checked against `f-io/LIVI`'s
   own head-unit-side `Session.ts` (`requestShutdown()`): same shape, same
   idea (send, wait for ack with a fallback timeout — LIVI uses 1s, this
   project uses `BYEBYE_RESPONSE_WAIT` = 3s). Real-hardware-confirmed: the
   phone reliably acknowledges every time (`probe_state=byebye_response_received`).

2. **`soft_reset` could fail on a condition it should have treated as
   success.** `LibUsbAoaBackend::soft_reset` (`crates/transport-usb/src/linux.rs`)
   already correctly mapped `RusbError::NotFound` *from `.reset()` itself*
   to `SoftResetOutcome::Reenumerated` (a 2026-08-16 fix, see
   `MILESTONE_CHECKLIST.md` M4). But the earlier `open_device` call inside
   the same function could *also* fail — with `AoaError::Unplugged` — if the
   device had already disappeared from the enumeration by the time this ran
   (observed right after a clean session end, consistent with the phone
   voluntarily dropping out of AOA accessory mode as part of its own
   teardown). That case wasn't caught, so it propagated as a hard error
   instead of the same "device is mid-reenumeration" signal — the caller
   (`session_supervisor::resolve_device`) then used `wait_for_reconnect`'s
   weaker "already present, no real wait" semantics instead of
   `wait_for_physical_replug`'s proper absence-then-presence wait. Fixed:
   `AoaError::Unplugged` from `open_device` now also maps to `Reenumerated`.
   Real-hardware-confirmed: `soft_reset` now reports `outcome=ok_reenumerated`
   on every cycle, never `outcome=failed`.

`session_supervisor::resolve_device` was also changed so the reconnect
immediately following a *successful* cycle (`consecutive_failures == 0`)
takes the same soft-reset path as recovering from an actual failure
(`consecutive_failures == 1`), rather than a faster no-reset path — matching
the already-proven M4 100-cycle-soak behavior (`--force-disconnect-each-cycle`).
This is a real, defensible correctness improvement (a fresh reconnect
deserves a real USB-level reset) but, per the trials below, does not by
itself close the residual race either.

## What did *not* fix the residual race (ruled out with real-hardware
evidence, not assumption)

Eight real-hardware trials, `--max-cycles 15` (or 10 for the final,
longest-delay one), each with a materially different fix in place:

| # | Change tested | Result |
|---|---|---|
| 1 | No `ByeByeRequest` at all (baseline) | Race on every single post-success cycle |
| 2 | `ByeByeRequest` sent, no wait for response | Race unchanged |
| 3 | `ByeByeRequest` + wait for `ByeByeResponse` | Race cut roughly in half, still frequent |
| 4 | + 750ms grace period after the response | No further change |
| 5 | Soft-reset made default even after success | No change — soft-reset succeeded every time, race unchanged |
| 6 | Soft-reset `open_device`-unplugged bug fixed | No change vs. #5 |
| 7 | + 2s settle delay *after* confirming presence | TLS collision eliminated, but replaced by a **new** failure: the confirmed address went stale during the sleep (device bounced a second time), causing a 10s write timeout instead |
| 8 | 2s delay moved to *before* confirming presence | Both failure modes could still occur, same cycle numbers |
| 9 | Delay increased to 8s (same ordering as #8) | **No change whatsoever** — identical cycle numbers failed as at 0ms |

The trial-9 result is the important one: an 8-second delay is generous by
any reasonable measure of "phone still tearing down," and it made zero
measurable difference. Combined with #5/#6 showing the *actual* USB-level
reconnect succeeding or failing doesn't correlate with the outcome either,
this rules out "not enough settle time" and "the USB reconnect itself is
unreliable" as explanations. After the `0`/`1` merge (fix in #5), the code
path taken by a reconnect right after a success and one right after a
failure is now structurally identical — same function calls, same order —
yet the outcome still alternates by cycle parity.

## Leading hypothesis, unconfirmed

The phone's *application-layer* Android Auto session state appears to reset
on some cadence not observable or controllable from the head-unit side —
plausibly an internal retry/cooldown counter in the phone's own (closed-source)
AA/AOA service. A hardware-level cause (a quirk specific to the exact USB
port/cable/hub in use) was raised as an alternative and not yet tested
(swapping the port/cable is the next cheap, informative experiment if this
is revisited).

## Why this was left as-is

Per Blake's explicit direction (2026-08-18): the two real fixes above are
kept; the ineffective settle-delay code was removed rather than left in
place adding latency for no measured benefit. The residual race:

- Only affects the reconnect immediately following a **clean** session end
  within the same running process — in real driving use (a long or
  unbounded observation window, not this investigation's rapid ~30s test
  cycles), this boundary is rare.
- **Always self-heals automatically** within one retry cycle via the
  existing soft-reset escalation, confirmed across all 8 trials — no hang,
  no crash, no operator action needed.
- Has no confirmed fix left to try that isn't speculative (the hardware-port
  test) or already ruled out (more/different delay placement, protocol
  messaging, reset-on-every-cycle).

If picked up again: try a different USB port/cable first (cheapest,
most different-in-kind test not yet run) before any further code changes.
