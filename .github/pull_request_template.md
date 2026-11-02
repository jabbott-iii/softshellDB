<!--
Thank you for contributing to softshellDB. Read CONTRIBUTING.md before
opening this pull request: the project rules in AGENTS.md apply to every
change. A pull request needs an issue on which a maintainer confirmed the
change, unless it fixes a typo or a broken link.
Replace the comments below, and remove the sections that do not apply.
-->

## Linked issue and roadmap task

Closes #

- Roadmap task: <!-- such as M3.4, or "none" -->
- [ ] A maintainer confirmed the change on the linked issue.

## Summary

<!-- What changed and why. Keep the pull request to the one issue or task. -->

## Type of change

- [ ] Bug fix
- [ ] Roadmap task (every acceptance criterion in `docs/roadmap.md` is met)
- [ ] New feature or change outside the roadmap (recorded in `docs/decisions.md`)
- [ ] Breaking change: a public API, an on-disk format, the protocol, the dependency matrix or a CLI
- [ ] Performance
- [ ] Documentation
- [ ] Build, CI or `xtask`
- [ ] Tests or refactor only, with no change in behavior

## Validation

<!-- Tick only what you ran, and paste the output below. Say which checks
you could not run, and why (a check that needs macOS or Windows, for
example). -->

- [ ] `cargo xtask ci`
- [ ] `cargo xtask fuzz-smoke`, for changes to a decoder (`codec`, `storage` readers, `proto`, `expr` parser, catalog, config)
- [ ] `cargo xtask crash --exhaustive`, for changes to `storage`, `txn` or `platform`
- [ ] `cargo xtask bench <filter>`, for performance work (paste the numbers before and after)
- [ ] `cargo xtask fixtures --new <format>`, for a format change

**Tested on:** <!-- OS and version, CPU, filesystem, and `rustc -V` -->

**Not run, and why:** <!-- or "none" -->

<details>
<summary>Command output</summary>

```text
paste output here
```

</details>

## Checklist

- [ ] No crate from outside the workspace is added, and `Cargo.lock` has no `source =` entries (`docs/02-dependency-policy.md`).
- [ ] Each crate depends only on the crates its row of the dependency matrix in `docs/01-architecture.md` allows.
- [ ] `unsafe` and platform `cfg`s are only in `softshell-sys` and `softshell-platform`; each `unsafe` block has a `// SAFETY:` comment.
- [ ] No `unwrap()` in library code, every `pub` item is documented, and no lint is weakened.
- [ ] Decoders never panic on input, check sizes against `Limits` before allocating, and have fuzz targets.
- [ ] An on-disk format or protocol change bumps its version, reads or clearly rejects old data, and adds fixtures (`/change-on-disk-format`).
- [ ] No test is deleted or ignored, no limit or tolerance is raised, no crash point is skipped and no committed fixture is rewritten to make a check pass, unless the summary says why.
- [ ] Docs are updated where behavior or design changed: the subsystem spec, an ADR in `docs/decisions.md`, and for a roadmap task its tick and its feature in `docs/features.jsonc`.
- [ ] No secrets, tokens, credentials, database files with real data, or build output (`target/`) are included.
- [ ] If a coding agent wrote part of this change, I have reviewed all of it, and the agent followed `AGENTS.md`.

## Storage review checklist

<!-- Required for changes to `storage`, `txn` or `platform`
(docs/04-quality.md). Remove otherwise. -->

1. Files written, in order, and when each is synced:
2. What a crash between each pair of steps leaves, and how recovery handles it:
3. Crash-point, corruption or model test covering the new path:
4. Locks newly held across I/O:
5. On-disk format changes:

## Security and compatibility

<!--
Security: does this change how untrusted files or network input are read,
authentication or authorization, limits, `unsafe` code, or CI workflows and
their permissions? Describe the effect. Never describe an undisclosed
vulnerability here: report it privately as SECURITY.md describes.
Compatibility: does this change a public API, an on-disk format, the
protocol or a command's arguments? Write "None" if neither applies.
-->

## Notes for reviewers

<!-- What to look at first, known limits, and follow-up work. -->
