# User-supplied credential provisioning

The repository and Debian package contain no receiver certificate, private key, shared identity, or authentication bypass. A user who is authorised to use a credential pair provisions it locally after installation.

## Development checkout

The repository-root `secrets/` directory is a local convenience only. Git ignores every file in it except `README.md`. Do not force-add credential files, paste their contents into issues or logs, or use the directory as the production location.

Check a pair offline on Raspberry Pi OS before installation:

```sh
chmod 600 /path/to/headunit.key
aa-headunit-diagnostics credentials check \
  --certificate /path/to/headunit.crt \
  --private-key /path/to/headunit.key
```

The check reads bounded regular files, verifies current certificate dates, verifies that the key matches the certificate, and rejects group/world-readable private keys. It does not contact a phone and does not establish that Android Auto will accept the identity.

## System installation

**One-time group setup**: the package's `postinst` creates a dedicated `aa-headunit` system group and makes `/etc/aa-headunit` group-writable (`root:aa-headunit`, `0770`) so that authorised operators never need `sudo`/root for day-to-day credential or probe commands. Add yourself once, then start a fresh login session (group membership only takes effect in new sessions):

```sh
sudo usermod -aG aa-headunit "$USER"
# log out and back in (or `newgrp aa-headunit` for the current shell only)
```

Install an authorised pair — no `sudo` needed once the group membership above is active:

```sh
aa-headunit-diagnostics credentials install \
  --certificate /path/to/headunit.crt \
  --private-key /path/to/headunit.key
```

The command creates `/etc/aa-headunit/credentials` itself, owned by the invoking user, and installs new files at:

- `/etc/aa-headunit/credentials/headunit.crt` with mode `0644`;
- `/etc/aa-headunit/credentials/headunit.key` with mode `0600`, owned solely by the user who ran the command — `crates/credential-store`'s loader rejects any group- or other-readable private key regardless of group membership, so group access to the parent directory never weakens the key file's own protection.

It refuses to overwrite an existing installation. Credential rotation will be a separate explicit operation so an interrupted setup cannot silently replace a working identity.

Check the configured installation without opening USB or network transport:

```sh
aa-headunit-diagnostics credentials status
```

The default paths are declared in `/etc/aa-headunit/config.toml`.

**Migrating a pair installed before this change**: earlier installs (via `sudo credentials install`) left `/etc/aa-headunit/credentials` and its files owned by `root`, which still requires `sudo` regardless of group membership — file ownership, not group access, is what the private-key check keys off. To move an existing pair onto the unprivileged path, re-own it to the real operating user (replace `USER` and adjust the filenames if different), then confirm with `credentials status` unprivileged:

```sh
sudo chown -R "$USER:aa-headunit" /etc/aa-headunit/credentials
aa-headunit-diagnostics credentials status
```

This changes ownership metadata only — it never reads, prints, or otherwise touches the certificate/private-key contents.

## Bounded interoperability probe

After an authorised pair is installed, the diagnostics application can load it at runtime for an explicitly selected, bounded USB interoperability probe — unprivileged, once the group membership above is active:

```text
aa-headunit-diagnostics usb credential-probe \
  --device BUS:ADDRESS \
  --allow-live-aap
```

The command uses Android Auto compatibility identification with this project's own URI and development serial, logs only named protocol states, and stops at TLS completion before authentication completion or service discovery. It never prints certificate or private-key contents. The existing generated-identity probe remains permanently disabled.

## Safety boundary

These commands validate structure, dates, matching public/private material, and local file permissions only. They do not prove ownership, permission to present an identity, certification, or acceptance by a phone. Production use remains limited to credentials legitimately issued or authorised for the user and receiver.
