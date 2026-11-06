# Contributing to teigeodata

Thank you for helping. There are two kinds of contribution, and both are welcome.

## Fixing data (no Rust needed)

A wrong position, a wrong elevation, a misspelled name, a missing place, an old postal code:

1. **Fix it upstream first where you can.** If the error is in OpenStreetMap or Wikidata, fixing
   it there helps everyone, and our next data release picks it up.
2. Otherwise, open an issue with the **"Data correction"** form and give your evidence: a
   coordinate, a photo of a sign, a GPS track, or an official document.
3. Or open a pull request that adds a correction file under `data/countries/<cc>/corrections/`.
   The format and examples are in `docs/09-guidelines/` (the correction guide comes with
   Phase 3). CI validates it and posts a preview.

Spellings follow the sourced lexicon in `data/countries/<cc>/lexicon.yaml` (for example चोक is spelled **Chowk**). If
you think a lexicon rule is wrong, open an issue with your sources.

## Changing code

1. Read [AGENTS.md](AGENTS.md) (it applies to people too) and the
   [quality bar](docs/06-engineering/quality-bar.md).
2. Install the toolchain: `rustup` picks the version from `rust-toolchain.toml`.
3. Enable the hooks once: `git config core.hooksPath .githooks`.
4. Everything runs through `cargo xtask` (`cargo xtask help`). Tests use small fixtures, so you
   never need the full data.
5. Large changes to the format, the API or the data model start with an RFC: an issue with the
   **RFC** label describing the problem, the options and the measurements.

## Commits

Conventional Commits, signed off for the [DCO](DCO) (`git commit -s`). See
[commits and branches](docs/09-guidelines/commits-and-branches.md).

## Conduct

Everyone follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Licence of contributions

Code contributions are under Apache-2.0. Data contributions (corrections, lexicon entries) are
under the licence of the data build they enter (ODbL main, CC-BY 4.0 lite). By signing off you
certify you have the right to contribute them. Do not contribute data copied from sources whose
licence does not allow it (for example Google Maps).
