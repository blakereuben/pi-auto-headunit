# Android Auto Protocol Source Assessment — 4 August 2026

This assessment concerns source provenance and project policy. It is not legal advice. No Android Auto session code, protocol definitions, captures, or proprietary binaries were copied or inspected while preparing it.

## Result

The original assessment found no complete public specification. The project owner subsequently approved GPLv3-licensed AASDK as the protocol implementation source. That decision and the pinned upstream revision are recorded in [the AASDK adoption record](aasdk-adoption.md).

## Sources assessed

### Official public documentation

- AOSP documents generic Android Open Accessory negotiation and USB bulk transport.
- Google's Desktop Head Unit guide confirms USB AOA and development ADB/TCP transports, plus configurable display and input capabilities.
- Google's public Android for Cars material is aimed primarily at phone-app and AAOS developers. It refers car makers and partners to their Google contact for partner access.

These sources do not publish the production receiver's framing, security handshake, service discovery, channel identifiers, protobuf schemas, media messages, input messages, or wireless bootstrap.

### `f1xpl/aasdk`

The repository describes AASDK as a C++ implementation of the Android Auto protocol and declares GNU GPLv3 in its README. Its listed features cover the missing session, encryption, media, input, sensor, and control layers. OpenAuto's own repository states that OpenAuto is built on AASDK.

This establishes an open-source copyright licence declaration for AASDK. It does not, by itself, establish how the undocumented protocol definitions were obtained, whether confidential material was involved, or whether any separate patent, certification, trademark, or contractual issue applies. At the time of this initial assessment, the project had not inspected or copied AASDK protocol files. Inspection began only after the owner's approval and is tracked in the adoption record.

### `opencardev/aasdk`

This is a maintained public fork of `f1xpl/aasdk`. It contains the same protocol lineage and makes broader completeness claims, but the public material reviewed does not add a traceable origin for the protocol definitions. A fork cannot cure an unresolved provenance question merely by relicensing or restating the implementation.

### OpenAuto and other receivers

OpenAuto is GPLv3 but is explicitly excluded as a code source by the project owner. Other public receivers found during the survey either derive from AASDK/OpenAuto or describe reverse engineering and protocol sniffing. They are not independent public specifications and are not approved sources.

### Google Desktop Head Unit binary

The DHU is a downloadable testing tool, not published receiver source or a public protocol specification. The project will not disassemble, translate, redistribute, or use it as an implementation oracle.

## Candidate routes

1. **Written AASDK provenance plus review:** ask the original AASDK author to identify authorship and origin of its protocol definitions, confirm whether any NDA/confidential source was used, and confirm the intended GPLv3 grant for those files. Review the answer before adopting anything.
2. **Official partner route:** obtain receiver documentation or implementation rights directly from Google with terms that permit this public GPL project. Public Google material does not currently provide a hobby-project application path.
3. **Legally reviewed clean-room interoperability:** establish a documented two-team process using lawfully observed behaviour, with no DHU/OpenAuto/AASDK code supplied to the implementation team. This is substantial work and requires jurisdiction-specific legal advice before starting.
4. **Reduce scope:** retain the proven AOA/platform/media work but do not claim Android Auto receiver functionality if none of the routes above becomes acceptable.

## Provenance questions retained from the initial assessment

- Who authored the protocol schemas, numeric identifiers, framing rules, and security/session behaviour?
- Were they produced from public documentation, independent observation, licensed partner documentation, decompilation, or another source?
- Was any NDA, confidential SDK, leaked material, or restricted binary involved?
- Does the copyright holder affirm that these specific files are offered for reuse and modification under GPLv3?
- Are there contributions or bundled materials with different terms that require separate permission or notices?

These questions remain useful provenance context, but the project owner has accepted the GPLv3 source and authorised implementation. AASDK is now approved subject to file-level licence verification, attribution, and the scope controls in the adoption record.

## References

- AOSP Android Open Accessory: https://source.android.com/docs/core/interaction/accessories/aoa
- Google Desktop Head Unit testing: https://developer.android.com/training/cars/testing/dhu
- Google Android for Cars: https://developers.google.com/cars
- Original AASDK repository: https://github.com/f1xpl/aasdk
- OpenAuto repository: https://github.com/f1xpl/openauto
- Maintained AASDK fork: https://github.com/opencardev/aasdk
