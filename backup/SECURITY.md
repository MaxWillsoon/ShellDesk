# Security Policy

ShellDesk is a desktop SSH client that handles sensitive local data such as host records, SSH passwords, private keys, passphrases, root passwords, sync settings, and API keys. Please report security issues responsibly.

## Supported Versions

Security fixes are generally applied to the default branch and included in the next public release. If a release line is no longer maintained, maintainers may ask you to verify the issue against the latest release or current default branch.

| Version | Supported |
| --- | --- |
| Latest release | Yes |
| Default branch | Yes, for unreleased fixes |
| Older releases | Best effort |

## Reporting a Vulnerability

Do not open a public GitHub issue for a suspected vulnerability.

Preferred reporting path:

1. Use GitHub private vulnerability reporting or a private security advisory for this repository, if available.
2. If private reporting is not available, open a minimal public issue asking for a private disclosure channel. Do not include exploit details, credentials, hostnames, screenshots with secrets, logs with tokens, or reproduction steps that would enable abuse.

Include as much safe detail as possible:

- Affected ShellDesk version, commit, or branch
- Local operating system
- Remote operating system or service involved, if relevant
- A short description of the impact
- Safe reproduction steps using placeholder data
- Whether the issue requires local access, a malicious remote server, a malicious sync endpoint, a crafted config file, or user interaction
- Any known workaround or mitigation

## Sensitive Data

Never share:

- SSH passwords, root passwords, passphrases, private keys, API keys, tokens, cookies, or sync credentials
- Real production hostnames, IP addresses, usernames, file paths, database names, or service URLs unless they are fully masked
- Full vault/config files containing secrets
- Screenshots or recordings that reveal terminals, keys, paths, credentials, private network topology, or customer data

Use placeholders such as:

```text
user@example-host
192.0.2.10
REDACTED_PASSWORD
REDACTED_PRIVATE_KEY
```

## Security-Sensitive Areas

Please be especially careful when reporting or changing code in these areas:

- Vault/config storage and import/export
- `electron/preload.cjs` and exposed `window.guiSSH` APIs
- IPC validation and command/path sanitization
- SSH authentication, jump hosts, port forwarding, and proxy handling
- SFTP file read/write, upload/download, archive, and permissions operations
- Remote command execution and privilege escalation (`sudo` / `su root`)
- VNC tunneling and WebSocket proxying
- Browser/webview navigation, proxy rules, and webContents guards
- WebDAV sync and conflict handling
- AI provider settings and API key storage
- Packaging, auto-update, and release artifacts

## Disclosure Expectations

Maintainers will try to acknowledge valid security reports promptly, but response time may vary. Please allow reasonable time for investigation and remediation before public disclosure.

When a fix is ready, maintainers may publish a release, mention the issue in release notes, or create a security advisory depending on severity and project needs.

