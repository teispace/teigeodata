# teigeodata's signed catalog

This branch is served by GitHub Pages. It holds TUF metadata only: two catalogs under one root,
`main/` and `lite/` (D-384). The pack files live in GitHub releases named `catalog-*`.
Written by `cargo xtask tuf` and the `catalog-timestamp` workflow; never edit it by hand.
See docs/06-engineering/catalog-keys.md on `main`.
