# Development

Revise is written in Rust (PDFium, Parley, egui on wgpu, krilla). It rebuilds
PDFs using deterministic geometry and typography heuristics — no AI, OCR or
ML. See [ARCHITECTURE.md](ARCHITECTURE.md) and
[RECONSTRUCTION.md](RECONSTRUCTION.md) for how it works.

## Setup

```bash
scripts/fetch-pdfium.sh
```

```bash
cargo run --release -- fixtures/pdf/resume-single.pdf
```

## Building the Mac app

```bash
scripts/package-macos.sh --install
```

This builds and installs `/Applications/Revise.app`, registered so Finder's
*Open With* lists it for PDFs. `--dmg` also produces `dist/Revise.dmg`. The
app is self-contained: PDFium is bundled in `Contents/Frameworks`. Without
`--release` it is signed ad-hoc, which only runs on your own Mac.

## Releasing

Bump `version` in the workspace `Cargo.toml` first — installed copies offer
the update only when the release's version is higher than their own. Then:

```bash
scripts/package-macos.sh --release
```

Signs the app with the first "Developer ID Application" identity in the
keychain (override with `SIGN_IDENTITY`) using the hardened runtime, builds
`dist/Revise.dmg`, notarizes it with the notarytool keychain profile
`revise-notary` (override with `NOTARY_PROFILE`), staples the ticket and
prints the dmg's SHA-256. The version comes from `Cargo.toml`.

Then attach the dmg to a GitHub release tagged `v<version>` and update
`version` and `sha256` in `Casks/revise.rb` in the
[tcmarkfeld/homebrew-tap](https://github.com/tcmarkfeld/homebrew-tap) repo.

The app checks `releases/latest` of `tcmarkfeld/revise-pdf` once a day (and
on *Revise → Check for Updates…*). Installing downloads the release's `.dmg`,
requires the app inside to pass Gatekeeper and carry the same Team ID as the
running app, swaps the bundle in place and relaunches. Drafts and
pre-releases are never offered.

## Debugging

*View → Reconstruction Inspector* (⌥⌘I) shows the original render, a
difference overlay and structure boxes.

## CLI

```bash
cargo run -- outline fixtures/pdf/resume-two-column.pdf
```

```bash
cargo run -- compare fixtures/pdf/resume-single.pdf /tmp/cmp
```

`convert <in.pdf> <out.revise>` and `export <in> <out.pdf>` are also
available.

## Tests

```bash
cargo test --workspace
```

Synthetic fixtures with exactly known geometry live in
`crates/reconstruction/tests`; realistic fixtures (`fixtures/pdf`) are
generated from `fixtures/html` with WebKit via `scripts/html2pdf.swift`.
