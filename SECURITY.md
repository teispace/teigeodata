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

What each boundary promises, and the test or gate that shows it, is in the
[threat model](docs/06-engineering/threat-model.md).

## Supported versions

Before v1.0.0, only the latest `main` is supported. From v1.0.0:

| Version | Security fixes |
|---|---|
| The latest minor release of the current major | Yes, as a patch release |
| Earlier minor releases of the current major | No: upgrade to the latest minor, which is compatible ([what 1.0 promises](docs/06-engineering/semver.md)) |
| The previous major | For 6 months after the next major's first release |
| Data releases | The latest data release of each pack; a pack's fix is a new data release |

A fix is released for every package together (the Rust crates, the C library, every
binding), with an advisory on GitHub and a `CHANGELOG.md` entry under **Security**.

## Disclosure

A report stays private until a fixed release is out, or for 90 days, whichever comes first;
the timeline can be agreed differently with the reporter. The advisory names the affected
versions, the fixed version and, if they wish, the reporter.
