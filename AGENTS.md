# softshellDB — an all-in-one NoSQL database engine and library (Rust)

**Status:** early implementation. Every crate in the workspace map exists,
but most are still empty, and no `xtask` command is implemented yet.
`docs/roadmap.md` lists the work and ticks what is done. A task is ticked
once it is done and verified. A task whose code is written but still needs a
check on another OS stays unticked, with a `⏳ needs <OS>` note. The next
task is the first unticked one whose dependencies are ticked and whose
milestone's ADRs are `Accepted`, skipping one whose remaining work needs an
OS you cannot run. This document describes crates and commands that do not
exist yet: check before assuming any of them does.

## What we are building

- One embedded database engine for **six data models** over one
  transactional key-value core: key-value, documents, graph, time-series,
  full-text search and vector search (ADR-0001). One commit can span all
  of them.
- **Library first** (`softshelldb`), then a single-node server, a wire
  protocol, a Rust client and a CLI (ADR-0003).
- **Targets:** Linux x86_64/aarch64 (tier 1); macOS 13+ (Apple Silicon and
  Intel) and Windows 10+ x86_64 (tier 2); local filesystems only
  (ADR-0006).
- **Out of scope:** distribution (sharding, consensus), SQL, WASM,
  `no_std`, mobile, 32-bit, network filesystems, generating embeddings.
- **Built from the ground up:** zero third-party crates. We write our own
  codecs, compression, checksums, caches, storage engine, indexes, server
  and test harnesses (ADR-0002).

## Read before working

| If you are touching… | Read first |
| :- | :- |
| Anything | `docs/00-overview.md`, then `docs/01-architecture.md` |
| `Cargo.toml`, `Cargo.lock`, a new crate | `docs/02-dependency-policy.md` |
| `crates/core`, `crates/codec` | `docs/subsystems/core-and-codec.md` |
| `crates/sys`, `crates/platform` | `docs/subsystems/platform.md`, `docs/03-platforms.md` |
| `crates/storage` | `docs/subsystems/storage-engine.md` |
| `crates/txn` | `docs/subsystems/transactions.md` |
| `crates/expr` | `docs/subsystems/expressions.md` |
| `crates/documents` | `docs/subsystems/documents.md` |
| `crates/graph` | `docs/subsystems/graph.md` |
| `crates/series` | `docs/subsystems/time-series.md` |
| `crates/text` | `docs/subsystems/text-search.md` |
| `crates/vector` | `docs/subsystems/vector-search.md` |
| `crates/softshelldb` | `docs/subsystems/facade.md` |
| `crates/proto`, `crates/server`, `crates/client` | `docs/subsystems/server-and-protocol.md` |
| `crates/cli`, `apps/tidepool`, `xtask`, `bench` | `docs/subsystems/cli-and-tools.md` |
| Tests, CI, fuzzing, crash tests | `docs/04-quality.md` |
| Versions, formats, releases | `docs/05-release.md` |
| Why something is the way it is | `docs/decisions.md` |

## Non-negotiable rules

1. **No third-party dependencies.** Do not add any crate outside this
   workspace to any `[dependencies]`, `[dev-dependencies]` or
   `[build-dependencies]` table, unless `docs/02-dependency-policy.md` lists
   it as `Approved`. If you believe one is needed, stop, write the
   evaluation the policy describes, and ask the user. `cargo xtask deps`
   enforces this.
2. **Layering is one-way.** A crate may depend only on the crates its row of
   the dependency matrix in `docs/01-architecture.md` lists. Data-model
   crates never depend on each other. `cargo xtask layers` enforces this.
3. **`unsafe` lives in two crates only:** `softshell-sys` and
   `softshell-platform`. Every other crate inherits `unsafe_code = "deny"`.
   - Every `unsafe` block carries a `// SAFETY:` comment explaining why it
     is sound.
   - Every `pub unsafe fn` has a `# Safety` doc section.
   - Raw OS declarations live only in `softshell-sys`.
4. **Platform `cfg` lives in those two crates only** (plus `xtask`
   packaging). Every other crate is identical on all OSes, and reaches the
   filesystem only through `Vfs`.
5. **Never lose an acknowledged commit.**
   - A commit is acknowledged as durable, or made visible under
     `Sync::Always`, only after its WAL record is synced.
   - Files are written, synced, renamed into place, the directory synced,
     and only then referenced by a synced manifest record. Nothing is ever
     overwritten in place.
   - A sync or write error on the WAL, the manifest, or an SST being
     flushed or compacted stops writes (ADR-0011); never retry a failed
     sync and report success.
   - Every change to `storage`, `txn` or `platform` keeps the crash-point
     test passing, and new file operations get crash coverage.
6. **Hostile input never panics.** Every byte read from disk or the network
   is checked: no out-of-bounds indexing, no unchecked arithmetic on
   lengths, no allocation before the size is checked against `Limits`, no
   unbounded recursion. Every decoder is a fuzz target.
7. **Git is read-only for agents.** Do not commit, amend, rebase, merge,
   tag, push, force-push or rewrite history. Leave all changes uncommitted
   for human review.
8. **Honest verification.**
   - Never state that a build, test, benchmark, fuzz run or crash run
     passed unless you ran it in this session and saw it pass.
   - If a command does not exist yet, say so instead of guessing.
   - Report what you did not run, and why.
9. **Smallest complete change.**
   - No unrelated refactors, renames, reformatting or "while I'm here"
     fixes.
   - Preserve public APIs, on-disk formats and the protocol unless the task
     authorizes a break; record breaks in `docs/decisions.md`.
10. **Decisions belong to the user.** Never mark an ADR `Accepted`, approve
    a dependency, or weaken a security control (auth, the loopback-only
    rule, limits, checksums, lints, tests) unless the user explicitly
    agreed in this conversation.
11. **No secrets.** Never write tokens, credentials, keys or connection
    strings into the repository, logs or test fixtures. Release secrets
    exist only in CI secret storage (`docs/05-release.md`).

## Workspace map (created by M0.1; each crate is empty until its tasks)

| Path | Package | Purpose | `unsafe` |
| :- | :- | :- | :- |
| `crates/core` | `softshell-core` | Bytes, varints, errors, `Limits`, CRC-32C, hashes, SHA-256, PCG32, ids, clock, civil time, logging, metrics, CLI parser, test harness | deny |
| `crates/sys` | `softshell-sys` | Hand-written raw OS declarations that `std` lacks (Windows `MoveFileExW`, `ProcessPrng`) | allowed |
| `crates/platform` | `softshell-platform` | `Vfs` trait; `OsVfs` per OS, `MemVfs`, `FaultVfs`; OS randomness | allowed |
| `crates/codec` | `softshell-codec` | `Value`, JSON/JSONC, SDOC, keycode, SLZ, record framing, series chunks, conversions | deny |
| `crates/storage` | `softshell-storage` | The LSM engine: WAL, memtable, SSTs, manifest, compaction, caches, checkpoint, check | deny |
| `crates/expr` | `softshell-expr` | Paths, filters, projections, sorts, updates, aggregation; query-document parser and evaluator | deny |
| `crates/proto` | `softshell-proto` | Wire protocol: framing, handshake, messages | deny |
| `crates/txn` | `softshell-txn` | Keyspaces, snapshot reads, single-writer transactions, compaction-filter registry | deny |
| `crates/documents` | `softshell-documents` | Collections, secondary indexes, planner, queries, updates | deny |
| `crates/graph` | `softshell-graph` | Property graphs, adjacency, traversals, shortest paths | deny |
| `crates/series` | `softshell-series` | Time-series: registry, tag index, chunking, windows, retention, rollups | deny |
| `crates/text` | `softshell-text` | Full-text: analyzer, postings, BM25 | deny |
| `crates/vector` | `softshell-vector` | Vector search: metrics, flat index, HNSW | deny |
| `crates/softshelldb` | `softshelldb` | Facade: `Database`, catalog, cross-model glue, maintenance, stats | deny |
| `crates/server` | `softshell-server` | The server: listener, sessions, auth, handlers | deny |
| `crates/client` | `softshell-client` | Rust client | deny |
| `crates/cli` | `softshell-cli` | The `softshell` binary | deny |
| `apps/tidepool` | `tidepool` | Reference application that drives requirements | deny |
| `xtask` | `xtask` | Build automation: guardrails, fuzzing, crash runs, benches, packaging | deny |
| `bench` | `softshell-bench` | Own benchmark harness and workloads | deny |

## Commands (planned: verify one exists before relying on it)

| Command | What it does | Built in |
| :- | :- | :- |
| `cargo xtask ci` | fmt check, clippy (`-D warnings`), tests, `deps`, `layers`, `unsafe-audit`, `headers`, `workflows`, `release-no-dev`, `features`: the full PR gate. Each check joins `ci` when its task lands | M0.6 |
| `cargo xtask deps` | Fails if `Cargo.lock` has a non-workspace package not approved in the policy | M0.6 |
| `cargo xtask layers` | Fails if a crate depends on a crate the matrix forbids | M0.6 |
| `cargo xtask unsafe-audit` | Fails if `unsafe` is outside `sys`/`platform` or lacks a `SAFETY:` comment | M0.6 |
| `cargo xtask headers` | Fails if a source file lacks the Apache-2.0 header (ADR-0004) | M0.6 |
| `cargo xtask workflows` | Fails if a workflow pins an action by anything but a commit SHA or over-grants its token | M0.8 |
| `cargo xtask features` | Fails if a ticked task belongs to no feature in `docs/features.jsonc` | M0.6 |
| `cargo xtask fuzz-smoke [--secs N] [--target T]` | Mutation fuzzer over every decoder | M0.7 |
| `cargo xtask crash [--minutes N] [--exhaustive]` | Process-kill runs; every crash point of the workload | M3.7, M3.8 |
| `cargo xtask fixtures --new <format>` | Writes fixtures of the current format version | M0.7 |
| `cargo xtask bench [filter]` | Benchmarks in release mode against stored baselines | M4.6 |
| `cargo xtask soak --minutes N` | Long mixed workload with resource tracking | M12.2 |
| `cargo run -p softshell-cli -- <command>` | The `softshell` CLI (`check`, `dump`, `open`, `serve`, …) | M4.7 onward |
| `cargo run -p tidepool -- <command>` | The reference app | M6.9 onward |

`xtask` never uploads, publishes, signs with real credentials, reads
secrets or calls the network. Publishing happens only in the release
workflow, on a tag pushed by the maintainer.

## Conventions

- **Toolchain and edition:** Rust edition 2024, stable toolchain pinned in
  `rust-toolchain.toml` (≥ 1.89, for `File::lock`). In edition 2024,
  `extern` blocks are written `unsafe extern "C" { ... }`.
- **Byte order:** on-disk integers are little-endian, except inside keys,
  which are big-endian (or keycode) so that byte order is numeric order.
- **Checksums:** CRC-32C, masked, on every WAL fragment, manifest record
  and SST block.
- **Keys:** model keys are built inside the model's own keyspace prefix,
  from `keycode` or the fixed-width big-endian fields its spec defines
  (ids, timestamps); never concatenate unencoded values into keys.
- **Errors:**
  - Each crate defines its own error enum implementing `Display` and
    `std::error::Error`, with context (file, offset, collection, index).
  - No `unwrap()` in library code (`clippy::unwrap_used` is denied outside
    tests). `expect("why this cannot fail")` only for real invariants, never
    on input-derived values.
- **Logging:** `softshell_core::log` macros only; `print!`/`eprint!` are
  denied outside the CLI, `xtask` and the bench. Never log document
  contents, keys of user data at `info` or above, or tokens.
- **Time:** inject `Clock` wherever wall time matters (ids, TTL,
  retention); use `Instant` for durations.
- **Determinism:** no `std::collections::HashMap` with `RandomState` in any
  code whose iteration order reaches disk, results or tests. Same input,
  same files.
- **Bounded work:** every cache, buffer, result set, traversal and request
  has a configured limit, and exceeding it is an error.
- **Docs:** every `pub` item has a doc comment (`missing_docs` = warn,
  `-D warnings` in CI).
- **License headers:** every source file starts with the Apache-2.0 header
  from `LICENSE`'s appendix, in its comment syntax (ADR-0004).
- **Tests:**
  - Unit tests in `#[cfg(test)] mod tests` beside the code.
  - Integration and model tests in each crate's `tests/`.
  - Format fixtures in `tests/fixtures/<format>/v<N>/`; fuzz seeds in
    `fuzz/corpus/<target>/`.

## Definition of done for a roadmap task

1. Every acceptance criterion for the task in `docs/roadmap.md` is met.
2. The task's verification commands were run. Report each command and its
   real result.
3. The checkbox is ticked only after steps 1 and 2.
4. Docs are updated if behavior or design changed. If a decision changed,
   add an ADR in `docs/decisions.md`. A ticked task belongs to a feature in
   `docs/features.jsonc`: rewrite that feature's summary for what is built
   now and update its `ticked` (`cargo xtask features` checks).
5. The final message summarizes:
   - changes and files affected
   - validation performed
   - checks not run, and why
   - assumptions
   - unresolved issues and residual risks
