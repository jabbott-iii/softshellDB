@AGENTS.md

## Claude Code specifics

The project rules are in `AGENTS.md` (imported above), shared with any other
coding agent. This section covers only what is specific to Claude Code.

**Project skills** (in `.claude/skills`):

| Skill | Use it to… |
| :- | :- |
| `/next-task [task-id]` | Pick the next unblocked roadmap task, or the one named, and carry it through the definition of done |
| `/evaluate-dependency <crate> <need>` | Run the build-vs-buy evaluation before anyone proposes a third-party crate |
| `/change-on-disk-format <format> <change>` | Change the WAL, SST, manifest, SDOC, keycode, SLZ, series chunk, catalog or protocol format safely: version bump, readers, fixtures, fuzz corpus |
| `/add-os-binding <os> <api>` | Add a raw OS declaration to `softshell-sys` and wrap it safely in `softshell-platform` |
| `/record-decision <title>` | Append an ADR to `docs/decisions.md` |
| `/fix-doc-drift [scope]` | Find statements in the docs that no longer match the code or the records, and correct them |
| `/draw-diagrams [file or view]` | Draw or rebuild architecture diagrams of the project as Mermaid in Markdown |
| `/phase-gates [plan \| phase ID]` | Group roadmap tasks into phases with checkable gates, or check a phase's gate |
| `/sweep-code [scope] [bugs \| dead code]` | Find and prove bugs and dead code, then fix the ones you approve, with regression tests |
| `/simplify-code [scope]` | Simplify the long or convoluted code you approve, keeping every behavior |
| `/security-sweep [scope]` | Find and prove security vulnerabilities, then patch the ones you approve |
| `/set-up-github [pieces]` | Write or merge the files GitHub reads: CodeQL, Dependabot, `SECURITY.md`, `CONTRIBUTING.md`, templates, `CODEOWNERS`, `.gitignore` |
| `/build-language-packs [languages] [scope]` | Translate user-facing text into nine languages, as draft language packs for review |
| `/build-course <description>` | Build a course from a description (it relies on `teaching-materials.md`, which this repository hasn't adopted) |

**Rules** in `.claude/rules` load automatically: a rule with `paths` when
you open a matching file, and a rule without them in every session:

| Rule file | Applies to |
| :- | :- |
| `unsafe-and-os.md` | `sys`, `platform` |
| `storage-engine.md` | `storage`, `txn`, `platform` |
| `on-disk-formats.md` | `codec`, `storage`, `proto`, the facade's catalog, `tests/fixtures`, `fuzz` |
| `data-models.md` | `expr`, `documents`, `graph`, `series`, `text`, `vector`, the facade |
| `network.md` | `proto`, `server`, `client`, `cli` |
| `cargo-manifests.md` | Cargo manifests and build scripts |
| `rust.md` | Every Rust source, Cargo manifest, `Cargo.lock`, and toolchain or lint file |
| `roadmap-and-docs.md` | `docs/`, `AGENTS.md`, `CONTRIBUTING.md`, `README.md` |
| `writing.md` | Every session: all text written for people to read |
| `commits.md` | Every session: commit messages, how much Git Claude may run, and pull requests |
| `dependencies.md` | Every session: anything the project uses that it didn't write |
| `testing.md` | Every session: tests, fixtures, coverage, and the checks CI runs |

**Shared with Doctrina:** `commits.md`, `dependencies.md`,
`roadmap-and-docs.md`, `rust.md`, `testing.md`, and `writing.md`, and every
skill above except `/evaluate-dependency`, `/change-on-disk-format`,
`/add-os-binding`, and `/record-decision`, are copies from
[Doctrina](https://github.com/jabbott-iii/Doctrina) at commit `3afa240`.
Change a copied rule here only in its "Project details" section, and a
copied skill not at all. Change their general text in Doctrina, then copy
it over.

**Permissions:**

- `.claude/settings.json` pre-approves routine `cargo` and read-only `git`
  commands.
- It denies Git operations that publish or finalize changes, plus
  `cargo publish` and `cargo login`.
- It asks before `cargo add`, `cargo remove`, `cargo update` and
  `cargo install`.
- These are guardrails, not a security boundary. The rules in `AGENTS.md`
  still apply when a command slips past a pattern.

**Working style:**

- Use the task list for any roadmap task with more than two steps. Make the
  last item "Verify and report", and run the task's `Verify` commands for
  it.
- Most tasks touch one crate. If a change spreads across more than three
  crates, stop and say so: the task is probably mis-scoped.
- Before starting a task, check its milestone's `Decisions` line. If any ADR
  listed there is still `Proposed`, stop and ask the user to decide it.
- For any change to `storage`, `txn` or `platform`, answer the review
  checklist in `docs/04-quality.md` ("Review checklist for storage
  changes") in your final message.
- When an acceptance criterion needs an OS this session can't run (Claude
  Code sessions are usually Linux), implement it and say plainly which
  checks still need that OS.
- When a meaningful unit of work is done, offer a commit message for the
  user to use. Never run `git commit` yourself.
