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

Install an authorised pair using:

```sh
sudo aa-headunit-diagnostics credentials install \
  --certificate /path/to/headunit.crt \
  --private-key /path/to/headunit.key
```

The command installs new files at:

- `/etc/aa-headunit/credentials/headunit.crt` with mode `0644`;
- `/etc/aa-headunit/credentials/headunit.key` with mode `0600`.

It refuses to overwrite an existing installation. Credential rotation will be a separate explicit operation so an interrupted setup cannot silently replace a working identity.

Check the configured installation without opening USB or network transport:

```sh
sudo aa-headunit-diagnostics credentials status
```

The default paths are declared in `/etc/aa-headunit/config.toml`. The `.deb` creates the empty credential directory with mode `0700`; it never installs credential contents.

## Safety boundary

These commands validate structure, dates, matching public/private material, and local file permissions only. They do not prove ownership, permission to present an identity, certification, or acceptance by a phone. Production use remains limited to credentials legitimately issued or authorised for the user and receiver.
