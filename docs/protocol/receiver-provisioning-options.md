# Android Auto Receiver Provisioning Options

Status: research shortlist, 5 August 2026. No vendor is selected, contacted, endorsed, or approved for integration.

## Purpose

The Pi 5 reaches AAP 1.6 and exchanges TLS data over both USB/AOA and Google's documented developer transport, but Android Auto rejects the project's independent identity with error 7. A synthetic mutual-TLS verifier confirms that the project presents its configured client certificate and completes the handshake when that identity is trusted. The remaining release gate is legitimate Android Auto receiver provisioning.

This document records lawful commercial or partner routes that may satisfy that gate without copying another receiver's credentials or adding a security bypass.

## Public Google route

Google's public Android for Cars material describes Android Auto projection to compatible vehicle head units and directs car makers toward an Android for Cars partner relationship. It does not publish a self-service receiver SDK, test identity, hobbyist provisioning process, or public head-unit partner application.

Consequently, ordinary Android app developer registration is not evidence that a head unit is authorised. Any Google route must explicitly cover receiver implementation, provisioning, testing, and the intended custom hardware product.

## Supplier shortlist

| Candidate | Publicly stated offering | Fit with this project | Unresolved gate |
| --- | --- | --- | --- |
| Cinemo CORE Projection | Pre-certified, platform-agnostic projection SDK for Android Auto, CarPlay, and CarLife; USB and wireless; all operating systems and SoC platforms claimed | Strongest stated match for 64-bit Raspberry Pi OS and the CM4/CM5 custom-carrier goal | Raspberry Pi/Broadcom support, hobby/evaluation access, pricing, redistribution, GPL boundary, and final-product certification are unconfirmed |
| ART SDK | Linux-based infotainment SDK with wired and wireless Android Auto and a UI integration layer; described for hardware meeting its requirements | Potential Linux route while retaining a project-owned UI | Raspberry Pi support, receiver provisioning, commercial terms, and whether projection can be licensed separately are unconfirmed |
| ECORE KOREA | Certified production Android Auto integration across Linux, Android, and RTOS platforms, including wired/wireless systems | Possible engineering or custom-hardware partner | Public examples use automotive SoCs rather than Raspberry Pi; minimum volume, cost, source boundary, and CM carrier support are unconfirmed |
| NXP Professional Services | Complete Android Auto projection solution and professional integration services | Confirmed example of a legitimate commercial route | Restricted to NXP i.MX processors; adopting it would replace the Raspberry Pi target and is therefore not the preferred route |

Marketing statements are not proof that a licence, credential, or certification transfers to this project. Only written supplier terms and successful evaluation on the target hardware can close the gate.

## Required inquiry

The first inquiry should describe the project honestly as a free, AI-created and owner-orchestrated GPLv3 hobby project targeting Raspberry Pi CM4, CM5, Pi 4, and Pi 5. It should ask:

1. Is an evaluation available for 64-bit Raspberry Pi OS on Pi 5 or CM5 using Broadcom VideoCore hardware?
2. Does the offering include legitimate Android Auto receiver provisioning for wired and wireless operation, rather than only protocol software?
3. Does its pre-certification apply to a custom head-unit product, or must each final PCB and software build complete separate Google testing?
4. Can the proprietary component be isolated behind an IPC or process boundary and redistributed alongside a GPLv3 application in a Debian package?
5. May the open-source repository remain public while credentials and licensed binaries stay outside Git and are provisioned securely?
6. What are the evaluation fee, engineering fee, production licence, minimum volume, recurring fee, and support lifecycle?
7. Which Linux interfaces are supported for DRM/Wayland display, V4L2/GStreamer video, PipeWire/ALSA audio, microphone input, USB, Bluetooth, and 5 GHz Wi-Fi?
8. What secure storage, unique-device identity, manufacturing provisioning, and revocation facilities are required?
9. Can one software product cover CM4, CM5, Pi 4, and Pi 5, including CM variants without onboard Wi-Fi/Bluetooth?
10. Is written permission available to name Android Auto compatibility in project documentation and releases after certification?

## Acceptance criteria

A candidate may enter a private evaluation only after written answers establish:

- legitimate receiver authorisation and a documented certification path;
- explicit Raspberry Pi 5 or CM5 `aarch64` Linux support;
- no shared, copied, or customer-exposed private credential;
- licence terms compatible with a public GPLv3 codebase and native `.deb` distribution;
- a viable hobby or low-volume cost;
- wired support now and a contractual or documented wireless path;
- credentials and proprietary binaries remain outside the public repository;
- the project can retain its Rust protocol, media, UI, and board-service separation where permitted.

If no candidate meets these criteria, Android Auto remains blocked. The project must then either retain the implementation as a fake-peer research platform or adopt a different, openly implementable phone-integration target. It must not solve the gate by reusing OpenAuto, AASDK, OpenAuto Pro, or another product's identity.

## Sources

- Google, Android for Cars: <https://developers.google.com/cars>
- Google, Android Auto requirements and compatible stereos: <https://www.android.com/auto/>
- Cinemo, CORE Projection: <https://automotive.cinemo.com/products-and-services/cinemo-core/core-projection/>
- ART, Software Development Kit: <https://www.artgroup-spa.com/product/software-development-kit-sdk/>
- ECORE KOREA: <https://ecore-korea.com/>
- NXP, Android Auto Projection: <https://www.nxp.com/design/software/embedded-software/android-auto-projection%3AANDROID-AUTO-PROJECTION>

