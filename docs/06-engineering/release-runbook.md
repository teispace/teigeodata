---
title: "Release runbook: the maintainer's setup, a release, and its checks"
status: accepted
updated: 2026-11-15
---

# Release runbook

How a code release reaches every registry ([releases](releases.md) has the versioning rules;
[D-302](../03-decisions/log.md) to D-305 the design). `release.yml` does the work. The
maintainer sets up the accounts once, starts each release and approves it.

## Once: what only the maintainer can set up

Every registry trusts one workflow file, `release.yml` (or `pub.yml` for pub.dev), in the
environment `release`. Names are those of D-211.

1. **GitHub** (repository settings):
   - Environment `release`: required reviewer the maintainer; deployment branches and tags
     `main` and `v*`.
   - In that environment, the secrets `RELEASE_DEPLOY_KEY` (the private half of an SSH key
     whose public half is a deploy key with write access: the workflow pushes the release
     commit and tags with it, so the tag push starts `pub.yml`), `MAVEN_CENTRAL_USERNAME` and
     `MAVEN_CENTRAL_TOKEN` (a Central Portal user token), `MAVEN_GPG_PRIVATE_KEY` (armoured)
     and `MAVEN_GPG_PASSPHRASE`.
   - Releases: immutable releases on.
   - If `main` is protected, the deploy key may bypass the rule (the release commit goes
     straight to `main`).
2. **crates.io:** the first version of each crate is published by hand, since a trusted
   publisher can be added only to a crate that exists. From the release's tag, with the
   maintainer's token, in this order: `teigeodata-core`, `-blob`, `-names`, `-format`, `-sync`,
   `-engine`, `-api`, `-cli` (`cargo publish -p <crate>`). Then, on each crate's settings page,
   add the trusted publisher: repository `teispace/teigeodata`, workflow `release.yml`,
   environment `release`.
3. **npm:** the scope `@teispace`. Each package of the release (`dist/teispace-*.tgz`: the
   main package, its per-platform addons, the WebAssembly package) is published once by hand
   (`npm publish --access public <file>`), then given the trusted publisher (organisation
   `teispace`, repository `teigeodata`, workflow `release.yml`, environment `release`).
4. **PyPI:** a pending trusted publisher for the project `teigeodata` (owner `teispace`,
   repository `teigeodata`, workflow `release.yml`, environment `release`): PyPI creates the
   project on the first upload.
5. **pub.dev:** the first version by hand from the release's tag (`dart pub publish` in
   `bindings/dart`, under the `teispace.com` verified publisher), then on the package's admin
   page: automated publishing from GitHub Actions, repository `teispace/teigeodata`, tag
   pattern `v{{version}}`, required environment `release`.
6. **Maven Central:** the namespace `com.teispace` verified (DNS TXT), a Portal user token, and
   a GPG key whose public half is on `keys.openpgp.org`.
7. **Go and Swift** need nothing: the tags are the release.

A registry not set up yet fails only its own job; the GitHub release and the other registries
are unaffected, and a re-run publishes what is missing (every publish step skips a version
that is already there).

## A release

1. Bump and record the version ([releases](releases.md#cutting-a-release), step 2); the ABI
   release gate requires at least the version `cargo xtask abi --since <last tag> --release`
   names. Push to `main`.
2. Dry run: `gh workflow run release.yml`. Every job green: the builds, every attestation
   verified, the release commit checked by Swift, Go, `dart pub publish --dry-run`,
   `npm publish --dry-run`, `twine check --strict`, the Maven bundle and
   `cargo publish --dry-run`.
3. The real run on the same commit: `gh workflow run release.yml -f dry_run=false`. Approve
   the `release` jobs as they come: the release commit and tags, the GitHub release, then
   crates.io, npm, PyPI, Maven Central and Go; the tag starts `pub.yml` for pub.dev.
4. If `main` moved after the dispatch, the push is refused and nothing is published: dispatch
   again from the new `main`.

## Checks after a release

- `gh release view vX.Y.Z` lists every file; `gh attestation verify <file> --repo
  teispace/teigeodata` passes on any of them; `sha256sum -c SHA256SUMS` in a folder of them.
- `cargo info teigeodata-engine@X.Y.Z`; `npm view @teispace/teigeodata@X.Y.Z`; `pip download
  teigeodata==X.Y.Z --no-deps`; `https://pub.dev/packages/teigeodata/versions/X.Y.Z`;
  `https://repo1.maven.org/maven2/com/teispace/teigeodata/X.Y.Z/`;
  `go list -m github.com/teispace/teigeodata/bindings/go@vX.Y.Z`; a scratch Swift package
  depending on `https://github.com/teispace/teigeodata` at `X.Y.Z` resolves.
- A Dart app and a Go program on a clean machine load the prebuilt library: the download is
  checked against the hashes the release commit pinned.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-15 | First version | phase 5.10c (D-305) |
