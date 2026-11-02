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

**Path-scoped rules** in `.claude/rules` load automatically when you open
matching files:

| Rule file | Applies to |
| :- | :- |
| `unsafe-and-os.md` | `sys`, `platform` |
| `storage-engine.md` | `storage`, `txn`, `platform` |
| `on-disk-formats.md` | `codec`, `storage`, `proto`, the facade's catalog, `tests/fixtures`, `fuzz` |
| `data-models.md` | `expr`, `documents`, `graph`, `series`, `text`, `vector`, the facade |
| `network.md` | `proto`, `server`, `client`, `cli` |
| `dependencies.md` | Cargo manifests, `.cargo`, the toolchain file, `xtask` |

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
