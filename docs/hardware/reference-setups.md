# Reference Hardware Setups

## Primary native build target

- Raspberry Pi 5, 8 GB
- NVMe storage
- Raspberry Pi OS 64-bit Trixie
- SSH development from Windows 11

## Raspberry Pi compatibility target

- Raspberry Pi 4, 8 GB
- Storage and audio configuration to be recorded during bring-up

## Compute Module target

- Compute Module 4, 4 GB, eMMC, no onboard Wi-Fi/Bluetooth
- Waveshare CM4-IO-BASE-B Mini Base Board (B), revision 3.1
- NVMe validation deferred by owner
- USB Wi-Fi and Bluetooth required for wireless work
- USB sound card available

## Display

- Original official Raspberry Pi 7-inch Touch Display
- 800 × 480 DSI
- Larger thin-bezel HDMI/USB-touch or DSI display remains a planned second profile

## Evidence still required

- Exact eMMC capacity
- Exact USB radio and sound-card USB IDs/drivers (the diagnostic will discover these)
- Exact CarPiHAT model/revision
- CM5 and CM5 carrier reference setup
- USB topology/current measurements under simultaneous phone, radios, and audio load

