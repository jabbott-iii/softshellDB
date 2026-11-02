# 05 — Versioning and release

## What gets released

| Artifact | Where | Built by |
| :- | :- | :- |
| The library crates (`softshelldb` and the `softshell-*` crates it depends on) | crates.io | The release workflow |
| `softshell` CLI and server binary | GitHub Releases: `.tar.gz` for Linux and macOS, `.zip` for Windows, with a `SHA256SUMS` file | The release workflow, one job per target |
| Tidepool | Not released; it lives in the repository as the reference application | — |

## Crate names

- `softshell` is **taken on crates.io** by an unrelated project (checked
  2026-11-01). The facade therefore publishes as **`softshelldb`**, and
  code names it `softshelldb`.
- Lookups of `softshelldb` and `softshell-core` on 2026-11-01 returned a
  client error, which suggests the names are free. **Confirm every crate
  name is free, then reserve them, before the first release (M12.5).**
- Crates that are internal to the workspace and not needed by users of the
  library (`xtask`, `softshell-bench`, `tidepool`) set `publish = false`.

## Version numbers

- One version for every published crate, set in `[workspace.package]`.
  `cargo xtask crates` (M12.5) checks that every published crate is at it,
  carries a description, license, repository and copies of `LICENSE` and
  `NOTICE`.
- SemVer. Before 1.0, a minor version (`0.x`) may break the Rust API; a
  patch version (`0.x.y`) may not.
- The minimum supported Rust version (`rust-version`) is the pinned
  toolchain. Raising it is a minor-version change and its own task.

## On-disk format compatibility

On-disk formats are versioned separately from the crates (ADR-0021):

| Format | Magic | First version |
| :- | :- | :- |
| WAL segment (`*.wal`) | `SSWL` | 1 |
| SST file (`*.sst`) | `SSST` (in the footer) | 1 |
| Manifest (`MANIFEST-*`) | `SSMF` | 1 |
| `IDENTITY` | `SSID` | 1 |
| SDOC value encoding | version byte | 1 |
| Series chunk | version byte | 1 |
| Catalog entries | `version` field | 1 |
| Wire protocol | handshake `protocol` field | 1 |

Rules:

- Any change to what a format writes bumps its version, and follows
  `/change-on-disk-format`: a reader for every supported old version or an
  explicit `UnsupportedVersion` error, fixtures for the new version, and a
  fuzz corpus update.
- **Before 1.0:** a release may drop support for formats older than the
  previous release. It must say so in the release notes, and
  `softshell export` from the previous release must be able to move the
  data (JSON Lines).
- **From 1.0:** every 1.x release reads every format 1.0 wrote. New format
  versions are written only after the database is upgraded explicitly
  (`softshell upgrade <dir>`), so a database can be opened by the previous
  minor release until then.
- A database records the format versions it uses in its manifest. Opening
  a database that uses a version newer than the running build is an error
  naming the version, never an attempt to guess.

## Protocol compatibility

- The handshake states the protocol version. A server accepts the current
  version and the one before it; a client offers the newest it knows.
- New request kinds and new optional fields are additive. Removing or
  changing one bumps the protocol version.

## Release process

Releases are cut by the maintainer, never by an agent (`AGENTS.md`,
rule 7).

1. `main` is green on every target, and the M12 per-OS checklist in
   `docs/03-platforms.md` was run on each OS for this commit.
2. The maintainer updates the version in `[workspace.package]` and the
   changelog in a normal pull request.
3. The maintainer tags the merge commit `v<version>` and pushes the tag.
4. The release workflow (M12.5):
   - checks that the tag is on `main` and matches `[workspace.package]`;
   - runs `cargo xtask ci` again;
   - builds the binaries for each target and writes `SHA256SUMS`;
   - publishes the crates to crates.io in dependency order, with the token
     from CI secret storage;
   - creates the GitHub release with the binaries and the notes.
5. Nothing in `xtask` uploads, publishes or reads a secret. Publishing
   happens only in the release workflow, which runs only on tags.

## Security releases

- A fix for a reported vulnerability (`SECURITY.md`) lands on `main` and is
  released as a patch version as soon as CI passes.
- There are no maintenance branches before 1.0.

## Changelog

`CHANGELOG.md` (created by the first release task) lists, per release:
added, changed, fixed, security, and **format and protocol changes** with
their new version numbers.
