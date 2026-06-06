# Security Policy

## Supported Versions

Goose is currently in alpha. Only the latest commit on `main` is supported.

| Version | Supported |
|---------|-----------|
| main (latest) | ✅ |
| older commits | ❌ |

## Reporting a Vulnerability

**Please do not report security vulnerabilities via public GitHub issues.**

If you discover a security vulnerability, report it privately:

1. Open a [GitHub Security Advisory](https://github.com/tigercraft4/goose/security/advisories/new) — this is confidential and only visible to maintainers.
2. Or contact the maintainer directly via the group chat linked in [CONTRIBUTING.md](CONTRIBUTING.md).

Include:
- A description of the vulnerability and its potential impact
- Steps to reproduce or a proof of concept
- Any suggested mitigations

You can expect an acknowledgement within 72 hours and a resolution timeline within 14 days for confirmed issues.

## Scope

Security issues in scope:

- The iOS app (`GooseSwift/`) — data leakage, insecure Keychain usage, URL scheme abuse
- The Rust core (`Rust/core/`) — memory safety, FFI boundary issues
- The self-hosted server (`server/`) — injection, authentication bypass, data exposure
- CI/CD workflows (`.github/workflows/`) — supply chain risks

Out of scope:

- Issues requiring physical access to an unlocked device
- Denial-of-service via large BLE payloads
- Issues in third-party dependencies (report to the upstream project directly)

## Disclosure Policy

We follow responsible disclosure. Once a fix is deployed, we will publish a security advisory crediting the reporter (unless anonymity is requested).
