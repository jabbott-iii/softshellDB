# 04 — Quality: testing, checks, CI

A database earns trust by not losing data. Because we write our own storage
engine, codecs and server, testing does the job that a battle-tested crate
would otherwise do. Every harness below is our own code (dependency policy
items 29–31; ADR-0020).

## Test layers

| Layer | Where | Runs in | What it catches |
| :- | :- | :- | :- |
| Unit tests | `#[cfg(test)] mod tests` beside the code | `cargo test`, all OSes | Logic errors in codecs, iterators, planners, scoring |
| Property tests | `softshell_core::testing::check` | `cargo test` | Round-trips, ordering laws (`keycode`), invariants of every encoder |
| Model-based tests | each crate's `tests/model.rs` | `cargo test` | The engine, a model or the catalog drifting from a simple reference (`BTreeMap`) under random operations, reopens included |
| Crash-point tests | `crates/storage/tests/crash.rs`, `crates/softshelldb/tests/crash.rs` | `cargo test` (bounded); `xtask crash --exhaustive` (all points) | Lost or partial commits after a crash at any sync point |
| Process-kill tests | `xtask crash` | Linux CI (time-boxed); every OS before release | Real crashes: the OS page cache, file handles, locks |
| Fault-injection tests | `FaultVfs` error schedules | `cargo test` | `EIO`, `ENOSPC`, short writes and sync failures turning into panics or silent success |
| Corruption tests | `crates/storage/tests/corruption.rs` | `cargo test` | A flipped bit anywhere going undetected |
| Isolation tests | `crates/txn/tests/history.rs` | `cargo test` | A read transaction seeing anything but one committed state |
| Format fixture tests | `tests/fixtures/`, read by each format's tests | `cargo test` | A new version failing to read files an old version wrote |
| Fuzz smoke | `xtask fuzz-smoke` | CI (time-boxed) | Panics, hangs, huge allocations in every decoder |
| Benchmarks | `softshell-bench` | Manual; nightly once there is a nightly runner | Regressions against stored baselines |
| Soak | `xtask soak --minutes N` (from M12.2) | Before release | Memory growth, file-handle leaks, compaction falling behind |

## Own harnesses

### Property testing — `softshell_core::testing`

```rust
/// Runs `property` on `cases` inputs generated from a seeded PCG32. On
/// failure, panics with the seed and case index so the failure replays.
pub fn check<T: std::fmt::Debug>(
    name: &str,
    cases: u32,
    generate: impl Fn(&mut Pcg32) -> T,
    property: impl Fn(&T) -> Result<(), String>,
);

/// The same, for inputs with a natural size, which grows from 0 to
/// `MAX_SIZE` (100) over the run. On failure, retries the failing case at
/// every smaller size and reports the smallest that still fails.
pub fn check_sized<T: std::fmt::Debug>(
    name: &str,
    cases: u32,
    generate: impl Fn(&mut Pcg32, u32) -> T,
    property: impl Fn(&T) -> Result<(), String>,
);
```

- The seed comes from `SOFTSHELL_SEED` (decimal, or hex after `0x`), or a
  fixed value, so CI runs are reproducible. The seed is printed on
  failure, in decimal, as `replay with SOFTSHELL_SEED=…`.
- Each case has its own generator, seeded through SplitMix64 from the
  run's seed, the property's name and the case number: same-seed PCG32
  streams are correlated, so cases must not share a seed.
- A panicking generator or property is reported like any other failure.
  The report shows the case, the size (and the smallest failing size),
  the property's message and the input's `Debug` text, each cut at 4 KiB.

### Model-based tests

Each stateful component is tested against the simplest correct model:

| Component | Model |
| :- | :- |
| `storage` | `BTreeMap<Vec<u8>, Vec<u8>>` per sequence number, with flush, compaction and reopen as random operations |
| `txn` | A list of committed states; each read transaction must equal the state at its snapshot |
| `documents` | A `Vec<Value>` filtered and sorted by a naive evaluator; every index answer must equal a full scan |
| `graph` | Adjacency lists in `BTreeMap`s; traversals by a naive BFS |
| `series` | A sorted `Vec<(i64, f64)>` per series; windows computed directly |
| `text` | Brute-force BM25 over every document |
| `vector` | The flat index is the model; HNSW must reach the recall budget against it |

### Crash consistency — `FaultVfs`

`FaultVfs` (in `softshell-platform`, feature `fault-injection` or tests)
simulates a machine that can lose power at any moment:

- Each file has a **durable** image and a list of **pending** writes.
  `sync_data` moves pending writes into the durable image.
- Creating, renaming and removing files are pending until `sync_dir` of
  the parent (on the simulated Unix profile) or durable at once (on the
  simulated Windows profile, matching NTFS; `docs/03-platforms.md`).
- `crash()` discards every pending write and pending directory operation.
  With `tear = true`, the last pending write of each file is kept up to a
  random 512-byte boundary instead of discarded whole.
- Error schedules: fail the Nth read, write or sync with `EIO`, `ENOSPC`
  or a short write.
- Every operation increments a counter, so a test can crash "at sync point
  k" deterministically.

**The crash-point test** runs a deterministic workload, counts its sync
points N, and then for every k in 0..N (bounded in `cargo test`, all of
them under `xtask crash --exhaustive`):

1. runs the workload on a fresh `FaultVfs`, crashing at sync point k, with
   and without tearing;
2. reopens the database;
3. checks the invariants:
   - the recovered state equals the state after commits `1..=j` for some
     `j`, with no partial commit;
   - `j` is at least the last commit acknowledged as durable before the
     crash (every acknowledged `Sync::Always` commit survives);
   - `softshell check` finds no corruption;
   - the database accepts new writes, and survives a second crash.

### Process-kill tests — `xtask crash`

- `cargo xtask crash --minutes N [--seed S]` builds a test binary that runs
  a deterministic workload with `Sync::Always` and prints the sequence
  number of each acknowledged commit on stdout. The workload is a test
  target of the crate under test (`softshell-storage`, later the facade),
  built through `cargo` (ADR-0020).
- The parent kills the child at a random moment (`SIGKILL`, or
  `TerminateProcess` on Windows), reopens the database itself, and checks
  that every acknowledged commit is present and the state is a prefix of
  the workload. Then it repeats until the time is up.
- This catches what `FaultVfs` cannot: real file handles, the OS page
  cache, lock release on process death. It does not simulate power loss;
  `FaultVfs` does.
- The exit status is 0 for a clean run and 1 for a finding. A finding
  saves the database directory to `target/crash-findings/<seed>/`.

### Corruption tests

- For every file type (WAL, SST, manifest, `CURRENT`, `IDENTITY`), flip
  every bit of a small fixture, one at a time, and check that opening or
  reading returns `Error::Corruption` naming the file and offset, or (for
  the WAL's last record only) that recovery truncates the tail and logs it.
- No bit flip may cause a panic, a hang, an allocation over `Limits`, or a
  wrong answer returned as `Ok`.

### Fuzz smoke (stable Rust, no dependencies)

- `cargo xtask fuzz-smoke [--secs N] [--target NAME]` loads
  `fuzz/corpus/<target>/` seeds. `--secs` is the time per target
  (default 60; CI uses 30).
- **Where the code lives** (ADR-0020): the driver (corpus loading,
  mutations, `catch_unwind`, the watchdog, findings) is in
  `softshell_core::testing`. Each target is a test in the crate that owns
  its parser, built on that driver. `xtask` keeps a table of target names
  and their crates and runs each target through `cargo`, passing the time
  budget and seed in environment variables; it links only `core` and
  `codec` (ADR-0005).
- Every seed runs unchanged first. After that each input is a random seed
  with 1 to 16 random mutations: bit flips, byte insert/delete, splice,
  length-field tweaks, "interesting" integers (0, 1, 255, 65535,
  `u32::MAX`, `u64::MAX`).
- Each input runs inside `catch_unwind`, with a watchdog: a target that
  does not finish within 2 s is a hang and ends the run.
- Findings go to `fuzz/crashes/<target>/{panic,hang}-<hash>.bin` (ignored
  by Git). Copy one into `fuzz/corpus/<target>/` to keep it as a regression.
- A target that can check its own answer asserts it, so a wrong answer is
  a finding too: decoders re-encode and compare, `keycode` checks order
  against the value comparator, `slz` decompresses its own output.
- Targets (each added by the task that adds the parser):

  | Target | Parser |
  | :- | :- |
  | `json` | `codec::json::parse` |
  | `sdoc` | `codec::sdoc::decode` |
  | `keycode` | `codec::keycode::decode` |
  | `slz` | `codec::slz::decompress` |
  | `record` | `codec::record::Reader` (WAL and manifest framing) |
  | `series_chunk` | `codec::series_chunk::decode` |
  | `sst_block`, `sst_footer` | `storage` SST readers |
  | `manifest_edit` | `storage` version-edit decoder |
  | `query_doc` | `expr::parse_query` |
  | `catalog_entry` | facade catalog decoder |
  | `proto_frame` | `proto` frame and message decoder |
  | `config` | server `softshell.jsonc` loader |

### Format fixtures

- `tests/fixtures/<format>/v<N>/` holds small files written by version `N`
  of each on-disk format, plus a `README.md` saying what they contain.
- Each format's tests read every committed version and compare with the
  expected values; unsupported old versions must fail with
  `Error::UnsupportedVersion { format, found, supported }`.
- `cargo xtask fixtures --new <format>` writes fixtures for the current
  version into a new `v<N>/` directory. It never rewrites a committed one.
  The writer for each format is in the tests of the crate that owns the
  format; `xtask` runs it through `cargo` (ADR-0020).

### Isolation tests

- Writers run a random workload; meanwhile reader threads open snapshots
  and record everything they read with the snapshot's sequence number.
- The checker replays the committed history and asserts that every reader's
  observations equal the model state at its sequence number. With one
  writer, this proves snapshot reads are consistent and commits atomic.

### Determinism

- Two flushes or compactions of the same input produce byte-identical SST
  files: no timestamps, sorted iteration, deterministic hashers only.
- HNSW construction is deterministic for a given seed and insertion order.
- No `std::collections::HashMap` with `RandomState` in any code whose
  iteration order reaches disk, results or tests.

### Benchmarks — `softshell-bench`

- `cargo xtask bench [filter]` runs in release mode: warm-up, then N
  samples, reporting median, p95 and p99, throughput, and for storage
  workloads write and space amplification.
- Workloads: YCSB-style A (50/50 read/update), B (95/5), C (read-only),
  E (short ranges), F (read-modify-write); sorted bulk load; recovery time;
  document indexed queries; graph k-hop; series ingest and windows; text
  BM25 top-k; vector recall and latency against the flat index.
- Datasets are generated from a seed, never downloaded.
- Baselines live in `bench/baselines/<machine>.json`; a run compares with
  the baseline for its machine name and flags changes beyond 10%. Not a CI
  gate until runner timing is stable.

## Guardrail checks

| Command | Fails when |
| :- | :- |
| `cargo xtask ci` | Any of the checks below (`workflows` from M0.8), plus `fmt --check`, `clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`. It runs every check even after one fails, the quick ones first, and names the failed ones at the end |
| `cargo xtask deps` | `Cargo.lock` has a package from outside the workspace that is not approved in `xtask/deps-allowlist.txt`: from crates.io, at exactly that name and version. That covers registry and git packages, approved names from anywhere but crates.io, optional dependencies, and path packages outside the workspace (vendored code, a `[patch]` to a local copy). Also: a member's manifest declares such a dependency, or an approved one not required at exactly its version (`= x.y.z`); a member is not a crate of the dependency matrix; a `Cargo.toml` that is not the root's or a member's, or a `Cargo.lock` but the root's; a submodule; `patch` or `replace` in `Cargo.toml`, or `source`, `patch`, `registries`, `paths` or `include` in `.cargo/config.toml`, however spelled (headers, dotted keys, inline tables): these are not allowed at all; an allowlist line that is malformed or cites an ADR not `Accepted` |
| `cargo xtask layers` | A crate depends on a crate its row of the dependency matrix does not list (normal, dev and build dependencies, every platform; a dev-dependency on itself is no edge), a workspace crate has no row, or a row names no workspace crate |
| `cargo xtask unsafe-audit` | Outside `softshell-sys` and `softshell-platform`: the `unsafe` keyword, in code or in a doc example (fenced or indented, in doc comments or `#[doc = "…"]`); a `doc` attribute that is not a plain string literal (`include_str!`); `global_asm!`, `naked_asm!` or `asm!`; or the `unsafe_code` lint named anywhere but in a `deny` or `forbid` attribute (so `allow`, `expect`, `warn`, `cfg_attr`, raw identifiers and macros are covered). Inside them: an `unsafe` block or `unsafe impl` with no comment starting `SAFETY:` and saying something, after the statement before it ends; an exported macro containing `unsafe`; or an extern block in `softshell-platform`, since raw declarations belong in `softshell-sys`. Everywhere: `#[path]` or `include!` naming anything but a relative `.rs` file inside the crate, `include` renamed, a macro that gives a module attributes from a fragment or calls a macro it is given; a symbolic link; a target whose root is not a `.rs` file inside its package; a crate or target not on edition 2024; a member without `[lints] workspace = true`, or a workspace whose `unsafe_code` is not `deny`. Files are tracked or untracked-but-not-ignored, and belong to the member whose directory holds them most closely. The lexer takes the characters Rust treats as whitespace, invisible ones included, as whitespace |
| `cargo xtask headers` | A source file lacks the Apache-2.0 header, or has it altered or out of place (ADR-0004): it must be the first thing in the file, or follow a shebang (in a Rust file, a `#!` line that rustc takes as one). Source files are Rust files (`*.rs`), scripts (`*.sh`, `*.ps1`, `*.py`), GitHub workflows (`.github/workflows/*.yml`, `*.yaml`) and the server's sample `softshell.jsonc`. Exempt: Markdown, Cargo manifests and other TOML files, other JSON and JSONC data (`docs/features.jsonc`), other `.github` files, `LICENSE`, `NOTICE`, format fixtures and fuzz corpora. A byte-order mark is a finding; `\r\n` line endings are not |
| `cargo xtask workflows` | A GitHub workflow uses an action not allowed, pins one by anything but a full commit SHA, or grants its token more than `contents: read` without a comment saying why |
| `cargo xtask release-no-dev` | A release build of `softshelldb` or `softshell-cli` (each built alone, `-p`) includes the `dev` or `fault-injection` feature of a workspace crate; any member's normal or build dependencies turn one on (resolved by cargo with the member's own features off, so a feature that turns it on, or a crate whose `default` does, counts too); in a member with a build script or a library, a string (of any kind, a `concat!` of strings, or a file read with `include_str!` or `include_bytes!`) has a `rustc-cfg` directive setting anything but a plain name of our own (not a feature, not a name rustc gives a meaning such as `test`, `doc` or `target_…`); or `.cargo/config.toml` holds anything but the `xtask` alias as written (`rustflags`, `[env]`, `runner`, `build.rustc` and the rest act behind cargo's back), or a `.cargo/config` file exists |
| `cargo xtask features` | A ticked `M` task of the roadmap belongs to no feature in `docs/features.jsonc`, a feature's `ticked` list disagrees with the roadmap, a feature lists a task the roadmap does not have or is missing a field, or a roadmap line looks like a task but does not parse |

Every cargo command `xtask` runs is offline (`--offline` and
`CARGO_NET_OFFLINE`), and those that resolve the graph are `--locked`; the
`cargo xtask` alias itself is an ordinary `cargo run`. `deps`,
`unsafe-audit`, `headers` and (for build scripts) `release-no-dev` list
files with `git ls-files`, so they run in a Git checkout. Exit status: 0
when every check passed, 1 when one found a problem or could not run, 2
for a command line not understood. Cargo's own 101 means it could not build
or resolve `xtask` itself, which includes a new dependency it cannot find
offline, or that `xtask` panicked.

Not covered, and left to review: code copied into an existing file; build
scripts that download or generate code, or that build a `rustc-cfg`
directive at run time (`.claude/rules/cargo-manifests.md`); a
crates.io package replaced by source replacement outside the repository's
own `.cargo/config.toml` (a parent directory's, `$CARGO_HOME`'s), since the
allowlist pins no checksum; `RUSTFLAGS` in a CI job's environment (the
`workflows` check, M0.8); and platform `cfg` outside the two OS crates
(ADR-0005).

## CI

- GitHub Actions, one workflow (`.github/workflows/ci.yml`) on pull
  requests and pushes to `main`. Every job starts `xtask` from a directory
  outside the checkout, as `cargo +<pinned> --locked run --manifest-path
  <checkout>/Cargo.toml --profile xtask -p xtask -- ci`, naming the
  toolchain `rust-toolchain.toml` pins (`cargo +1.99.1 …`). Cargo reads
  `.cargo/config.toml`, and rustup `rust-toolchain.toml`, from the current
  directory, so the repository's own configuration cannot redirect how
  `xtask` is built or run (a `runner` would stop it before it starts), and
  `xtask` then checks that file.
  Without `--locked`, `cargo run` would quietly rewrite a stale or edited
  `Cargo.lock` before `deps` reads it. Once a third-party crate is
  approved, `cargo fetch --locked` runs first, since `xtask` is offline.
  - Linux x86_64: `cargo xtask ci`, `cargo xtask fuzz-smoke --secs 30`,
    `cargo xtask crash --minutes 5`.
  - Linux aarch64, macOS aarch64, Windows x86_64: `cargo xtask ci`.
- Every action is pinned by full commit SHA with a version comment;
  Dependabot proposes updates to those pins weekly, with a 7-day cooldown.
- The workflow token is `contents: read`. No workflow on pull requests has
  access to secrets.
- CodeQL scans the Rust code and the workflows (once Rust support in
  CodeQL is confirmed for the pinned toolchain; until then, workflows only).

## Review checklist for storage changes

A pull request that touches `storage`, `txn` or `platform` answers these in
its description:

1. Which files are written, in what order, and when is each synced?
2. What does a crash between each pair of steps leave on disk, and how does
   recovery handle it?
3. Which crash-point, corruption or model test covers the new path?
4. Does any lock get held across I/O that it was not held across before?
5. Does any on-disk format change? If so, `/change-on-disk-format` was
   followed.
