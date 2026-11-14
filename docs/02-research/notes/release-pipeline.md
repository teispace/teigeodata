---
title: "The release pipeline: one workflow, every registry, provenance on every artifact"
status: research
updated: 2026-11-14
owner: maintainer's agents
revisit: 2027-02-14
---

# The release pipeline

## 1. Question

Phase 5.10 asks for per-ecosystem publish jobs with provenance on every artifact, and a dry
run on GitHub (spike S9). Publishing itself needs the maintainer's accounts. How is a release
built, signed and published to crates.io, npm, PyPI, pub.dev, Maven Central, SwiftPM and the Go
proxy from this public repository, what can run before the accounts exist, and how is it
proven?

## 2. Summary and recommendation

- **One top-level `release.yml`** owns releases. It calls the existing build workflows
  (`c-package`, `apple`, `jvm`, `python`, `node`, `wasm`) through `workflow_call`, so a release
  builds and tests exactly what CI builds and tests. Their own `v*` tag triggers and
  `gh release upload` jobs go away: each uploads its outputs as an Actions artifact instead.
- **Registry steps run only in `release.yml`'s own jobs**, never inside a called workflow:
  PyPI refuses trusted publishing from a reusable workflow (its action's README), and every
  registry's trust policy then names one file, `release.yml`.
- **Trusted publishing (OIDC) wherever the registry has it:** crates.io
  (`rust-lang/crates-io-auth-action`), npm (CLI 11.5.1 or later, provenance published by
  default), PyPI (`pypa/gh-action-pypi-publish`, PEP 740 attestations by default) and pub.dev
  (GitHub OIDC on a tag push). Maven Central has no OIDC: a Portal user token and a GPG key,
  as secrets in a protected environment. NuGet is parked (D-299).
- **Provenance on every file:** `actions/attest-build-provenance` over every artifact (SLSA
  provenance signed through public-good Sigstore, verified with `gh attestation verify`), an
  SBOM per Rust artifact (`cargo-cyclonedx`), attested with `actions/attest-sbom`, and
  `SHA256SUMS`.
- **Draft, upload, publish:** GitHub's immutable releases accept assets only while the release
  is a draft, so the release is created as a draft, every asset is uploaded and attested, and
  only then is it published.
- **Swift and Go publish through git tags.** SwiftPM's `binaryTarget` names the zip's URL and
  checksum in the tagged `Package.swift`, and the zip's bytes differ per build (`ditto` stores
  file times, [Swift note](swift-xcframework.md)). So the workflow, not a person, makes the
  release commit: it builds, writes `Package.swift` with the checksum, commits, tags and uploads
  that same zip. Go's module in `bindings/go` needs the tag `bindings/go/vX.Y.Z` on the same
  commit.
- **Dry run:** `workflow_dispatch` with `dry_run: true` (the default) runs everything except
  the commit, the tags, the release and the uploads to registries. Each registry step runs its
  own check instead: `cargo publish --dry-run`, `npm publish --dry-run`, `twine check
  --strict`, `dart pub publish --dry-run`, the Maven bundle's layout and POM checked, the
  generated `Package.swift` parsed by `swift package dump-package`. Attestations are made in
  the dry run too, so the signing path is proven before the first release.
- **The maintainer's part:** create the registry accounts and namespaces (D-211), register
  `release.yml` as the trusted publisher on each, publish the first version of each crate and
  the NuGet package by hand where the registry requires it, add the Maven secrets, and approve
  the `release` environment on each real run.

## 3. Findings

- **crates.io** trusted publishing: `rust-lang/crates-io-auth-action` v1.0.6 (2026-10-21)
  exchanges the job's OIDC token for a short-lived token, which `cargo publish` then uses as
  `CARGO_REGISTRY_TOKEN`; the first version of each crate is published by hand
  ([action](https://github.com/rust-lang/crates-io-auth-action),
  [RFC 3691](https://rust-lang.github.io/rfcs/3691-trusted-publishing-cratesio.html)). Cargo
  1.99 publishes a workspace in dependency order (`cargo publish --workspace`, checked
  locally 2026-11-14). Every crate here is `publish = false` today; D-198 names the stable
  surface (`teigeodata-engine`, `teigeodata-sync`, `tgd`).
- **npm** trusted publishing has been generally available since 2025-07-31; it needs npm CLI
  11.5.1 or later and `id-token: write`, and publishes provenance without `--provenance`
  ([npm docs](https://docs.npmjs.com/trusted-publishers),
  [changelog](https://github.blog/changelog/2025-07-31-npm-trusted-publishing-with-oidc-is-generally-available/)).
  Local npm is 11.20.0. Configurations made after 2026-09-03 allow staged publishing.
- **PyPI**: `pypa/gh-action-pypi-publish` v1.14.3 (2026-10-20) makes PEP 740 attestations by
  default with trusted publishing, and cannot be used from a reusable workflow
  ([action](https://github.com/marketplace/actions/pypi-publish),
  [PyPA guide](https://packaging.python.org/en/latest/guides/publishing-package-distribution-releases-using-github-actions-ci-cd-workflows/)).
- **pub.dev** publishes from GitHub Actions through OIDC when a tag matching the package's
  pattern (`v{{version}}`) is pushed; a tag pushed by another workflow does not trigger a
  workflow, so the publishing job must run in the workflow that pushes the tag, or be
  dispatched ([Dart docs](https://dart.dev/tools/pub/automated-publishing)).
- **Maven Central** (Central Portal since OSSRH closed in 2025) needs every artifact signed with
  GPG, a `-sources.jar`, a `-javadoc.jar` and a POM; no OIDC was found
  ([Scalar](https://scalar.com/products/sdks/publishing/java),
  [Eclipse CBI](https://eclipse.dev/cbi/best-practices/github-actions/central-portal/)).
- **Immutable releases** (generally available): assets can be added only to a draft; GitHub
  records a release attestation of the tag, commit and assets when it is published
  ([GitHub docs](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases)).
- **Attestations:** `actions/attest-build-provenance` v4.2.2 (2026-08-06) and
  `actions/attest-sbom` v4.1.0 (2026-03-18) need `id-token: write` and `attestations: write`;
  for a public repository they are signed through public-good Sigstore.
- **Today's state (2026-11-14):** six workflows build and test the artifacts on every push and
  attach them on a `v*` tag with `gh release upload`; no registry is published to, nothing is
  attested, and the root `Package.swift` points at a local build.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| One `release.yml` calling the build workflows, registry steps in its own jobs | Builds what CI tests; one trust policy file; the order (draft, attest, publish) in one place | The build workflows gain `workflow_call` and artifacts | ours | GitHub Actions | **Chosen** |
| Each workflow publishes on its tag | Already half there | Six trust policies; no single place to attest and order; PyPI's reusable-workflow limit; races between workflows | ours | in use | Replaced |
| `release-plz` 0.4.3 | Versions, changelog, crates.io | Rust only; our version is shared by every ecosystem | MIT / Apache-2.0 | 2026-11-02 | Not now |
| `cargo-dist` 0.34.0 | Installers and checksums for `tgd` | CLI binaries only, not libraries | MIT / Apache-2.0 | 2026-11-02 | Later, for `tgd` installers |

## 5. What we build ourselves, and why

`release.yml` and the changes to the six build workflows. Its proof is the dry run on GitHub:
every artifact built and attested, `gh attestation verify` passing on each, and every
registry's own check passing, with nothing published.

## 6. Impact on the plan

5.10 in three sub-steps:

- **5.10a** the build workflows callable (`workflow_call`, artifacts), `release.yml` assembling
  them with checksums, SBOMs and attestations; the dry run green on GitHub, attestations
  verified.
- **5.10b** each registry's job in dry-run form: the crates made publishable (the stable
  surface and what it needs), npm, PyPI, pub.dev, the Maven bundle, `Package.swift` and the Go
  tag.
- **5.10c** the real path behind the `release` environment and the maintainer's runbook: what
  to create on each registry, in which order, and how to verify a release.

## 7. Risks and open items

- pub.dev's trigger rule (a tag pushed by the workflow does not start another workflow) is met
  by publishing in the same workflow run; whether pub.dev accepts an OIDC token from a
  `workflow_dispatch` run is **unverified**: if not, the maintainer pushes the tag and the
  workflow runs on it.
- Maven Central's GPG key is a long-lived secret: it lives only in the `release` environment,
  which needs the maintainer's approval per run.
- A release built from a dispatch commits to `main`; a branch protection rule must allow the
  workflow's token, or the release commit goes through a pull request.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| actions/attest-build-provenance | v4.2.2 (2026-08-06) | 2026-11-14 | GitHub releases |
| actions/attest-sbom | v4.1.0 (2026-03-18) | 2026-11-14 | GitHub releases |
| rust-lang/crates-io-auth-action | v1.0.6 (2026-10-21) | 2026-11-14 | GitHub releases |
| pypa/gh-action-pypi-publish | v1.14.3 (2026-10-20) | 2026-11-14 | GitHub releases |
| CycloneDX cargo-cyclonedx | 0.5.10 (2026-11-10) | 2026-11-14 | GitHub releases |
| anchore/sbom-action | v0.24.3 (2026-10-02) | 2026-11-14 | GitHub releases |
| sigstore/cosign | v3.1.4 (2026-10-20) | 2026-11-14 | GitHub releases |
| release-plz | 0.4.3 (2026-11-02) | 2026-11-14 | GitHub releases |
| cargo-dist | 0.34.0 (2026-11-02) | 2026-11-14 | GitHub releases |
| npm CLI | 11.20.0 (local) | 2026-11-14 | `npm --version` |
| cargo | 1.99.1 | 2026-11-14 | `cargo --version` |

## 9. Sources

- [npm trusted publishers](https://docs.npmjs.com/trusted-publishers), accessed 2026-11-14.
- [npm trusted publishing GA](https://github.blog/changelog/2025-07-31-npm-trusted-publishing-with-oidc-is-generally-available/), GitHub changelog, 2025-07-31.
- [crates-io-auth-action](https://github.com/rust-lang/crates-io-auth-action), accessed 2026-11-14.
- [RFC 3691, trusted publishing for crates.io](https://rust-lang.github.io/rfcs/3691-trusted-publishing-cratesio.html).
- [Publishing with GitHub Actions](https://packaging.python.org/en/latest/guides/publishing-package-distribution-releases-using-github-actions-ci-cd-workflows/), PyPA, accessed 2026-11-14.
- [pypi-publish action](https://github.com/marketplace/actions/pypi-publish), accessed 2026-11-14.
- [Automated publishing to pub.dev](https://dart.dev/tools/pub/automated-publishing), accessed 2026-11-14.
- [Central Portal with GitHub Actions](https://eclipse.dev/cbi/best-practices/github-actions/central-portal/), Eclipse CBI, accessed 2026-11-14.
- [Immutable releases](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases), GitHub docs, accessed 2026-11-14.
