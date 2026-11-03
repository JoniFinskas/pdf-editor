# Versioning

PDF Editor follows [Semantic Versioning 2.0.0](https://semver.org): `MAJOR.MINOR.PATCH`.

## One version for the workspace

The `core` and `app` crates share one version, set only in the root
`Cargo.toml` under `[workspace.package]`. Both crates inherit it with
`version.workspace = true`. The editor shows it in the status bar and in
Settings, read at compile time from `CARGO_PKG_VERSION`. Never set a version in
a member manifest.

## When to increase it

While the version is below 1.0.0, the public API is not yet stable:

| Change | Bump | Example |
| --- | --- | --- |
| Breaking change to a public core function or type in CORE_API.md | MINOR | Renaming `Engine::edit`, changing `TextObject` fields |
| New user-visible feature or changed editing behavior | MINOR | New toolbar action, different font fallback rule |
| Incompatible settings file or output change | MINOR | New `settings.txt` format |
| Bug or security fix without new behavior | PATCH | Crash fix, wrong hit test, input validation |
| pdfium.dll or dependency update for security or maintenance | PATCH | New pinned PDFium release and checksum |
| Docs, tests, refactors, tooling with no effect on the shipped program | none | README edits, new fixtures |

Changes that do not bump the version are still listed under `Unreleased` in
CHANGELOG.md and ship with the next release.

1.0.0 requires verified documented features, a reproducible release build and
completed interactive release acceptance. From 1.0.0 on, a breaking core
API change or an incompatible settings or behavior change bumps MAJOR, new
features bump MINOR, and fixes bump PATCH.

Increase the version at most once per release, to the highest bump required by
the changes since the previous release. Lower parts reset to zero
(0.3.4 -> 0.4.0). Test builds before a release may use a pre-release suffix such
as `0.4.0-rc.1`.

## Release steps

1. Set the new version in the root `Cargo.toml`.
2. Run `cargo build` so Cargo.lock records it, then `cargo test` and
   `cargo deny check`.
3. In CHANGELOG.md, rename `Unreleased` to `[X.Y.Z] - YYYY-MM-DD` and start a new
   empty `Unreleased` section.
4. Commit only these changes as `chore: release vX.Y.Z`.
5. Create an annotated tag `vX.Y.Z` on that commit. Publishing the tag or a
   release build is a separate step.
6. Build with `./scripts/build-release.ps1`, then
   run `./scripts/package-release.ps1` from a clean checkout. It verifies the
   console binary's version without opening a window, includes licenses and
   source/build details, and writes a portable folder, ZIP and SHA-256 manifest
   under ignored `dist/`. Existing versioned packages are never overwritten.

`pdf-editor-cli.exe --version` prints the workspace version. The desktop also
shows it in Settings and the status bar. Packaging does not install the app,
change file associations, access documents, download dependencies or publish files.

Portable builds remap compiler source paths. Review packaged contents before
distribution.
