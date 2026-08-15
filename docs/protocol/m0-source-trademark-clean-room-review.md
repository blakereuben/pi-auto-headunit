# M0 close-out: protocol-source, trademark, and clean-room policy review

This is a closure record, not new research. It consolidates decisions and
evidence already established elsewhere to confirm `MILESTONE_CHECKLIST.md`'s
M0 item — "Complete the protocol-source, trademark, and clean-room policy
review needed for implementation" — against `MILESTONES.md`'s own M0 exit
gate: **"no unresolved legal/provenance ambiguity about the source
permitted for Milestone 1."** That gate is scoped to Milestone 1 specifically
(the documented USB/AOA vertical slice), not the full Android Auto session
protocol — the broader protocol-source gate for session/media/channel
behaviour is `MILESTONES.md`'s own M2 exit gate, already closed by
[ADR-0002](../architecture/decisions/0002-android-auto-protocol-source-gate.md)
and the adoption records it points to.

## Protocol source

Milestone 1 implements only the publicly documented Android Open Accessory
(AOA) transition and bulk-endpoint discovery — no post-AOA Android Auto
session behaviour. [The 4 August 2026 source
assessment](source-assessment-2026-08-04.md) confirms AOA itself carries no
provenance ambiguity: it is specified by AOSP's own public documentation
(https://source.android.com/docs/core/interaction/accessories/aoa), which
`MILESTONE_01.md` cites directly as this milestone's reference, and which
`MILESTONE_01.md`'s own definition of done requires reviewers be able to map
every implemented AOA control operation against. No AASDK, OpenAuto, LIVI,
or other third-party protocol source was used for the M1 slice — those
adoptions all came later, during M2+ protocol work, and are governed by
their own separate adoption records and ADR-0002, not by M0.

**Result: no unresolved protocol-source ambiguity for M1.** The M2-level
gate for later session-protocol sources (AASDK/OpenAuto/LIVI) is separately
closed by ADR-0002 and is not a blocker for this M0 item.

## Trademark

The project already carries the trademark posture `RISK_REGISTER.md`'s
R-004 calls for: a neutral working name, an explicit non-affiliation
notice, no Google logos/brand assets, and uncertified/R&D positioning.
Concretely: `README.md`'s licensing section states the project "is not
affiliated with, endorsed by, sponsored by, or certified by Google LLC,"
names Android/Android Auto as Google LLC trademarks, and confirms no
Google logos or brand assets are distributed; `PRD.md` §1 states the
project is "an independent, uncertified implementation" that "must not be
represented as Google-certified or as suitable for safety-critical vehicle
functions."

`PRD.md` §1 separately flags that "Pi Auto Head Unit" is a working name and
"a trademark review is required before naming the project." That is a
distinct, later action tied to *final product naming* (expected near 1.0
release, per the project's own working-name caveat), not a blocker for M1's
technical implementation — M1 makes no product-naming claim, ships no
public-facing branding, and the interim disclosure language already in
place is sufficient for continued development under the working name.

**Result: no unresolved trademark ambiguity blocking M1.** Final-name
trademark clearance remains explicitly open and tracked (`PRD.md` §1,
`RISK_REGISTER.md` R-004) as later, pre-1.0 work — not an M0/M1 blocker.

## Clean-room

[The 4 August 2026 source assessment](source-assessment-2026-08-04.md)
identifies "legally reviewed clean-room interoperability" as one of four
candidate routes considered for the *Android Auto session protocol*
(candidate route 3) — explicitly described there as "substantial work"
requiring jurisdiction-specific legal advice, and explicitly not the route
taken (the project instead pursued, and the owner approved, the pinned
GPLv3 AASDK/OpenAuto/LIVI adoption route, recorded in their own adoption
documents and ADR-0002).

M1 needed no clean-room process at all: it implements only publicly
documented AOA behaviour, with no third-party protocol source of any kind
involved. Clean-room procedure remains a live consideration only for any
*future* undocumented protocol behaviour that isn't covered by an approved
source — ADR-0002 already commits to recording clean-room procedure "if
applicable" for such cases, matching `MILESTONES.md`'s M2 exit-gate wording
verbatim.

**Result: no clean-room process was required for M1, and none is owed by
this item** — it remains correctly tracked as a conditional, future
consideration under ADR-0002, not an M0 gap.

## Conclusion

All three strands of this M0 item are closed for the purpose this item
gates: **M1's implementation had no unresolved legal/provenance ambiguity
about its source**, matching `MILESTONES.md`'s M0 exit gate exactly. Two
related items remain deliberately open elsewhere, not here: the M2-level
protocol-source gate for session behaviour (closed separately by
ADR-0002) and final-product-naming trademark clearance (tracked in
`PRD.md` §1 and `RISK_REGISTER.md` R-004 as pre-1.0 work).
