# Security policy

## Reporting a vulnerability

Please **do not open a public issue.** Use GitHub's private vulnerability reporting:
**Security → Report a vulnerability** on this repository. We acknowledge reports within 72
hours, agree a fix and disclosure timeline with you, and credit you unless you prefer not.

## Scope

- Parsing of `.tgd` packs, catalogs and trust metadata (packs from untrusted sources must never
  cause memory unsafety, panics across the FFI, unbounded memory or CPU use).
- Signature and checksum verification, and the update chain (rollback and freeze protection).
- The C ABI and every binding.
- The MCP and HTTP servers.
- The build and release pipeline (supply chain).

Data errors (a wrong position or name) are not security issues. Report them as
[data corrections](CONTRIBUTING.md#fixing-data-no-rust-needed).

## Supported versions

Before v1.0.0, only the latest `main` is supported. After v1.0.0, the latest minor release of
the current major version.
