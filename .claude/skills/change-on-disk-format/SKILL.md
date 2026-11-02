---
name: change-on-disk-format
description: Use whenever a change alters what softshellDB writes to disk or the wire - WAL, SST, manifest, IDENTITY, SDOC, keycode or the value order, SLZ, record framing, series chunks, catalog entries, any model's key layout, or the protocol. Bumps the version, keeps old data readable or clearly rejected, and adds fixtures and fuzz seeds.
argument-hint: "<format> <what changes>"
---

# Change an on-disk format or the protocol

Request: `$ARGUMENTS`

Data written by an older build must never be misread by a newer one
(ADR-0021). Follow every step.

## 1. Decide whether it is a format change

It is one if any byte that a previous build would write, or read, differs:
a new field, a reordered field, a new tag, a different default, a different
hash or checksum, a different key layout, a change to keycode or to the
value comparator (which changes every index). If unsure, treat it as one.

If the change is not backward compatible and was not authorized by the
task, stop and ask the user. Record the decision with `/record-decision`.

## 2. Bump the version

- Find the format's version constant (file magic + version, SDOC version
  byte, chunk version byte, catalog `version` field, protocol version, or
  the model's key-layout version in its catalog entry).
- Increment it. Writers write only the new version (before 1.0) or, from
  1.0, only after `softshell upgrade` (`docs/05-release.md`).
- Record the new version in the manifest's `format` edit if the format is
  one the manifest tracks.

## 3. Keep old versions readable, or reject them clearly

- Prefer a reader for every supported old version.
- Otherwise return `UnsupportedVersion { format, found, supported }`,
  never a misparse, and say in the release notes how to migrate
  (`softshell export` from the old build).
- A database whose manifest names a version newer than this build fails to
  open with that error.

## 4. Fixtures

- `cargo xtask fixtures --new <format>` writes `tests/fixtures/<format>/v<N>/`
  with a `README.md` describing the contents.
- Never edit or regenerate an older `v<M>/` directory.
- Tests read every committed version: the old ones decode to the expected
  values (or fail with `UnsupportedVersion`), the new one round-trips.

## 5. Fuzzing and limits

- Add seeds for the new encoding to `fuzz/corpus/<target>/`.
- Check every new length or count against `Limits` before allocating.
- Run `cargo xtask fuzz-smoke --target <target> --secs 120`.

## 6. Crash safety (storage formats)

- If the WAL, SST, manifest or `CURRENT` change, run
  `cargo xtask crash --exhaustive` and the corruption tests, and add a bit-
  flip test for every new field.

## 7. Docs

- Update the format's spec in `docs/subsystems/` and its row in
  `docs/05-release.md` ("On-disk format compatibility").
- Add a line for the changelog's "Format and protocol changes" section to
  your final message.

## 8. Report

- the format, old and new version numbers
- what old data does on the new build (read, or rejected with which error)
- fixtures added, fuzz and crash runs with their real results
- docs updated
