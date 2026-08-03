# Development Debian package

This packaging preview builds only `aa-headunit-diagnostics`. It installs no always-on service and does not modify `/boot/firmware/config.txt`.

The udev rule grants the active user/`plugdev` group access to MTP-class devices and the documented Google AOA accessory IDs. Some phones expose neither a matching MTP property nor accessory ID before the AOA transition; those devices will report a permission error until a narrowly reviewed rule is added. Do not solve this by running the diagnostic as root or granting access to all USB devices.

Build on Raspberry Pi OS Trixie with standard Debian source-package tooling from the repository root, using `packaging/debian` as the Debian metadata directory in the release builder.

