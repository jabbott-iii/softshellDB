# Contributing to softshellDB

Thanks for your interest in softshellDB, an all-in-one NoSQL database
engine and library written in Rust with no third-party crates. This guide
covers how to propose a change, set up a machine, check your work and open
a pull request.

The project's rules are in [`AGENTS.md`](AGENTS.md), which every
contributor follows, person or coding agent. Where this guide and
`AGENTS.md` disagree, `AGENTS.md` wins. The design is in [`docs/`](docs):
start with [`docs/00-overview.md`](docs/00-overview.md), then the
subsystem spec of the crate you are changing (the table "Read before
working" in `AGENTS.md` says which).

## Contribution workflow

1. **Open an issue first,** with the issue form: a bug, a proposal, a
   question, or a roadmap task you would like to take. A pull request with
   no issue will be declined, unless it fixes a typo or a broken link.
2. **If an issue already exists,** comment on it instead of opening
   another.
3. **Wait for a maintainer to confirm** the change on the issue before you
   open a pull request.
4. **Branch from `main` and make the change,** keeping to the one issue.
5. **Check your work** (see [Checks](#checks)), then open a pull request
   that links the issue. The template asks what you ran.
6. **The checks on GitHub must pass.** Today the only one is CodeQL.
   Roadmap task M0.8 will add a CI workflow that runs `cargo xtask ci` on
   Linux (x86_64 and aarch64), macOS (aarch64), and Windows (x86_64), and
   from then on `cargo xtask ci` must pass on every runner. A maintainer
   reviews every pull request (see `CODEOWNERS`).

### Roadmap tasks

Most work is a task in [`docs/roadmap.md`](docs/roadmap.md). Each task says
what to do (`Do`), what must be true when it is done (`Accept`) and how to
check it (`Verify`), and lists the tasks it depends on. Each milestone also
lists the decisions (ADRs) it rests on; a task cannot start while one of
them is still `Proposed`. A task is done when:

1. every acceptance criterion is met;
2. its `Verify` commands ran and passed, and the pull request reports each
   with its real result;
3. its checkbox is ticked, and only then;
4. the docs are up to date: the subsystem spec where behavior or design
   changed, an ADR in [`docs/decisions.md`](docs/decisions.md) where a
   decision changed, and the task's feature in
   [`docs/features.jsonc`](docs/features.jsonc).

A criterion that needs an OS you do not have stays unticked, with a
`⏳ needs <OS>: <the remaining check>` line under the task.

## Ground rules

These are the rules in `AGENTS.md` that a pull request is most often held
up on:

- **No third-party crates.** Do not add a crate from outside this
  workspace to any dependency table. If you think one is needed, say so on
  the issue, with the evaluation that
  [`docs/02-dependency-policy.md`](docs/02-dependency-policy.md)
  describes.
- **Layering is one-way.** A crate may depend only on the crates its row of
  the dependency matrix in
  [`docs/01-architecture.md`](docs/01-architecture.md) lists. Data-model
  crates never depend on each other.
- **`unsafe` and platform `cfg` live in `softshell-sys` and
  `softshell-platform` only,** with a `// SAFETY:` comment on every
  `unsafe` block.
- **Never lose an acknowledged commit.** Changes to `storage`, `txn` or
  `platform` keep the crash tests passing and answer the storage review
  checklist in [`docs/04-quality.md`](docs/04-quality.md).
- **Hostile input never panics.** Every decoder checks bounds and sizes
  and has a fuzz target.
- **Format changes are versioned.** A change to what is written to disk or
  the wire bumps the format's version, keeps old data readable or clearly
  rejected, and adds fixtures.
- **Smallest complete change.** No unrelated refactors, renames or
  reformatting. Keep public APIs unless the issue agreed to break one.
- **Report what you ran, honestly.** Never say a check passed unless you ran
  it and saw it pass, and say which checks you did not run.
- **No secrets.** Never commit tokens, credentials or keys, including in
  test fixtures.

## Development setup

- **Rust through [rustup](https://rustup.rs).** `rust-toolchain.toml` (from
  M0.1) pins the version and the `rustfmt` and `clippy` components; rustup
  installs them the first time you run `cargo` in the repository.
- **Git.**
- **Per system:** nothing else. The database uses only the standard library
  and the OS. On Windows, the MSVC build tools and the Windows SDK; on
  macOS, the Xcode command-line tools.

## Where code and tests go

- **Code** goes in the crate that the workspace map in `AGENTS.md` names
  for it, and depends only on what that crate's row of the dependency
  matrix allows.
- **Unit tests** go in a `#[cfg(test)] mod tests` beside the code, and
  **integration and model tests** in the crate's `tests/` folder.
- **Property tests,** which check a property against many generated
  inputs, use `softshell_core::testing::check`.
- **Format fixtures** go in `tests/fixtures/<format>/v<N>/`, **fuzz seeds**
  in `fuzz/corpus/<target>/`, and **benchmarks** in `bench/`.

"Test layers" in [`docs/04-quality.md`](docs/04-quality.md) lists every
kind of test, and [`.claude/rules/testing.md`](.claude/rules/testing.md)
says how strict each one must be.

## Checks

Run these before every pull request. `cargo xtask ci` exists; the others
come with their roadmap tasks (`cargo xtask help` lists what there is):

| Command | When |
| :- | :- |
| `cargo xtask ci` | Always |
| `cargo xtask fuzz-smoke --target <t> --secs 60` | A change to a decoder |
| `cargo xtask crash --exhaustive` | A change to `storage`, `txn` or `platform` |
| `cargo xtask bench <filter>` | Performance work (paste before and after) |

`cargo xtask ci` prints each check's result, and a passing run ends with
a line like this one. The count grows as roadmap tasks add checks.

```text
== ci: all 9 checks passed
```

## Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org/): a
subject line of the form `<type>(<scope>): <summary>`, then a blank line
and a body. This section summarizes
[`.claude/rules/commits.md`](.claude/rules/commits.md), which has the full
rules. The text of messages, docs, and comments follows
[`.claude/rules/writing.md`](.claude/rules/writing.md).

- **Type:** `feat` for new behavior, `fix` for something broken, `docs`,
  `refactor`, `perf`, `test`, `style` (formatting only), `build` (Cargo
  manifests and the toolchain), `ci`, `chore` (repository upkeep), or
  `revert`.
- **Scope:** the crate's directory name, such as `core`, `storage`, or
  `xtask`, and `claude` for `.claude/`. Leave it out when the change spans
  several areas.
- **Summary:** in the imperative ("add", not "added"), starting with a
  lowercase letter, with no final period, and 72 characters or fewer,
  prefix included. When the commit completes a roadmap task, end the
  summary with the task's ID in parentheses.
- **Body:** what changed and why, not how, and the roadmap task or issue
  it belongs to, wrapped at 72 characters. End with `Closes #12` or
  `Refs #12`, and add `BREAKING CHANGE: …` when the commit breaks a public
  API, an on-disk format, or the wire protocol (a break also needs an ADR
  in [`docs/decisions.md`](docs/decisions.md)).

Each commit holds one logical change. A task's code, its tests, and the
docs and roadmap tick it needs go in the same commit; unrelated work goes
in separate ones. For example:

```text
fix(storage): sync the directory after renaming an SST

A crash right after the rename could lose the directory entry, so
the manifest would reference a file that is gone.
```

Coding agents never commit; they leave changes for a person to review and
commit.

## Reporting a problem

Use the [issue form](https://github.com/jabbott-iii/softshellDB/issues/new/choose)
for bugs, proposals, and questions. Report a security vulnerability
privately, as [`SECURITY.md`](SECURITY.md) describes, never in a public
issue, discussion, or pull request.

## Coding agents

Coding agents follow [`AGENTS.md`](AGENTS.md), like everyone else. Claude
Code also loads [`CLAUDE.md`](CLAUDE.md), which lists the rules in
[`.claude/rules/`](.claude/rules) and the skills in
[`.claude/skills/`](.claude/skills). If an agent wrote part of your change,
review all of it before you open the pull request.

## License of contributions

softshellDB is licensed under the Apache License, Version 2.0
([`LICENSE`](LICENSE)). As section 5 of the license says, a contribution
you submit is under the same license, with no further terms: there is no
sign-off and no contributor license agreement. Every new source file starts
with the license header that ADR-0004 (an Apache-2.0 header on every source
file) specifies, and `cargo xtask headers` checks it.

## Code of conduct

Be respectful and assume good faith. Criticize code and designs, not
people.
