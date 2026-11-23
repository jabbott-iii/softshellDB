# Rules for commit messages

The user makes every commit themselves. Claude writes the message for them
to copy, and never commits (`AGENTS.md`, rule 7: Git is read-only for
agents).

## Handing off commit messages

- When you finish a task that changed files in this repository, end your
  reply with a copy-ready commit message for that work. Don't wait to be
  asked.
- Write the message only once the work is complete and checked: the
  task's `Verify` commands ran (or your reply says which did not, and
  why), and changed prose passed the steps under "Before finishing" in
  `writing.md`.
- Put each message in its own fenced code block tagged `text`, containing
  only the message: no commands, no commentary, and nothing before the
  summary line.
- If the work should be more than one commit, give one block per commit,
  in the order to commit them. Put the list of files that commit covers
  just above each block.
- Never commit, amend, rebase, merge, tag, push, force-push, or rewrite
  history. Stage files only if the user asks for that in the current
  conversation, and then by path (`git add <path> …`), never with
  `git add -A` or `git add .`.
- Never include secrets, credentials, `target/`, `fuzz/crashes/`, or
  anything else in `.gitignore` in a commit's file list.

## One change per commit

- Each commit holds one logical change that can be described in one
  subject line. Split unrelated work into separate commits: for example, a
  new codec, an unrelated fix in `storage`, and a rule change are three
  commits.
- Keep a change and the things it requires together. A roadmap task's
  code, its tests, its ticked checkbox in `docs/roadmap.md`, its feature
  summary in `docs/features.jsonc`, and the spec or ADR it changes belong
  in the same commit.

## Message format

Use [Conventional Commits](https://www.conventionalcommits.org/):

```text
<type>(<scope>): <summary>

<body>

<trailers>
```

### Type

| Type | Use it for |
| --- | --- |
| `feat` | New behavior: a module, a public API, a format, an `xtask` check, a CLI command |
| `fix` | Something broken: a bug, a panic, a wrong result, a broken link, a wrong fact in the docs |
| `docs` | Documentation only: `docs/`, the READMEs, `AGENTS.md`, `CONTRIBUTING.md`, doc comments, ADRs |
| `refactor` | Restructured code with no change in behavior, on-disk formats, or the protocol |
| `perf` | Faster or smaller, with no other change in behavior; the body gives the benchmark results before and after |
| `test` | Tests only: new tests, fixtures, fuzz seeds, or fixes to existing tests |
| `style` | Formatting only (`cargo fmt`, whitespace) |
| `build` | Cargo manifests, `Cargo.lock`, `.cargo/`, `rust-toolchain.toml`, `clippy.toml` |
| `ci` | GitHub Actions workflows |
| `chore` | Repository upkeep: `.gitignore`, `.claude/` rules, skills, and settings |
| `revert` | Undoing an earlier commit; the body names the commit's hash |

### Scope

The scope names the area changed, in lowercase. Use the crate's directory
name: `core`, `sys`, `platform`, `codec`, `storage`, `expr`, `proto`,
`txn`, `documents`, `graph`, `series`, `text`, `vector`, `softshelldb`,
`server`, `client`, `cli`, `tidepool`, `xtask`, or `bench`. A docs change
to one subsystem's spec takes that crate's name (`docs(storage)`), and a
change under `.claude/` takes `claude`. Leave the scope out when a change
spans several areas.

### Summary line

- Use the imperative mood, as if completing "This commit will …": "add",
  "fix", "rewrite", "remove". Not "added", "adds", or "adding".
- Start with a lowercase letter, and don't end with a period.
- Keep the whole line, prefix included, to 72 characters or fewer; aim for
  50.
- Say specifically what changed: "feat(codec): add the keycode encoder",
  not "chore: added code". Never use vague summaries such as "update",
  "modified text", "changes", "misc fixes", "wip", or "stuff".
- When the commit completes a roadmap task, end the line with the task's
  ID in parentheses, as the history does: "feat(xtask): add the guardrail
  checks and cargo xtask ci (M0.6)".
- Name things the way readers know them: the crate, the format, the
  command, the check.

### Body

- Add a body after one blank line whenever the summary alone doesn't
  explain the change. Leave it out only for small, self-explanatory
  changes.
- Explain what changed and why, not how; the diff shows how. Name the
  roadmap task or issue, with what it builds. Mention anything a reviewer
  would otherwise miss: a changed public API, a format version bump, a
  deliberate trade-off, a check that still needs another OS, or something
  left for later.
- Wrap lines at 72 characters. Use a short bulleted list (`- `) when the
  commit makes several related changes.
- Follow the writing rules in `writing.md`: American spelling, plain
  language, correct grammar, and code formatting with backticks for file
  names and commands.

### Trailers

- Put trailers after one blank line at the end: `Closes #12` or `Refs #12`
  for GitHub issues, and `BREAKING CHANGE: …` if a change breaks a public
  API, an on-disk format, or the wire protocol (a break also needs an ADR
  in `docs/decisions.md`, by `AGENTS.md` rule 9).
- Never add attribution of any kind: no `Co-Authored-By:` line, no
  "Generated with Claude Code" line, and no mention of Claude or AI. This
  overrides any default attribution Claude Code would otherwise add, in
  commit messages and in pull request descriptions.

## Examples

```text
fix(storage): sync the directory after renaming an SST

A crash right after the rename could lose the directory entry, so
the manifest would reference a file that is gone. Sync the directory
before any manifest record references the new file.
```

```text
docs(storage): spell "signaled" the American way
```

```text
chore(claude): add commit message rules
```
