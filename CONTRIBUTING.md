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
6. **CI must pass** on every runner. A maintainer reviews every pull
   request (see `CODEOWNERS`).

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

## Checks

Once they exist (the roadmap builds them in M0), run before every pull
request:

| Command | When |
| :- | :- |
| `cargo xtask ci` | Always |
| `cargo xtask fuzz-smoke --target <t> --secs 60` | A change to a decoder |
| `cargo xtask crash --exhaustive` | A change to `storage`, `txn` or `platform` |
| `cargo xtask bench <filter>` | Performance work (paste before and after) |

## Commit messages

Start the subject with the area it changes, in the imperative, under 72
characters: `storage: sync the directory after renaming an SST`. The body
says why, and names the roadmap task or issue. Coding agents never commit;
they leave changes for a person to review and commit.

## Code of conduct

Be respectful and assume good faith. Criticize code and designs, not
people.
