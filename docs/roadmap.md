# Roadmap

How to use this file:

- Work top to bottom. A task is **unblocked** when everything in its
  `Depends` line is checked, and every ADR its milestone's `Decisions`
  line names is `Accepted`.
- Tick a box only after its acceptance criteria are met **and** its
  verification commands were run and passed (see "Definition of done" in
  `AGENTS.md`).
- If a criterion needs an OS you can't run, leave the box unchecked and add
  an indented note saying exactly what remains:
  `  - ⏳ needs macOS: run the Vfs conformance suite`.
- Add findings as indented `Note:` lines under the task (pinned versions,
  measured numbers, decisions the user made).
- Exit criteria for a milestone are checked by a human.
- OS order within every milestone: **Linux → macOS → Windows**.
- The commands in `Verify` lines come into existence with the tasks that
  build them. Until then, they do not exist: say so rather than guess.

---

## Before M0 — Design review (the user)

- [x] **D0.1 — Review the proposed ADRs**
  - Depends: —
  - Do: the user reads ADR-0004 to ADR-0021 in `docs/decisions.md` and,
    for each, accepts it, asks for an amendment, or rejects it. An agent
    records each answer by changing the ADR's `Status` line to
    `Accepted (user decision, <date>)`, or by `/record-decision` for an
    amendment, and updates the docs that state the decision.
    - The ADRs in M0's `Decisions` line must be decided before M0.1.
    - The others may wait until the milestone whose `Decisions` line names
      them, but deciding early is better: the storage and format ADRs shape
      everything after them.
  - Accept: every ADR in M0's `Decisions` line is `Accepted`; any other ADR
    still `Proposed` is listed in a `Note:` here as deferred.
  - Verify: compare M0's `Decisions` line with the ADR statuses;
    `grep -c '^- \*\*Status:\*\* Proposed$' docs/decisions.md` gives the
    number still open (the template line does not match)
  - Note: decided by the user on 2026-11-01. All 18 accepted, none
    deferred. Accepted as written: ADR-0004, 0006, 0007, 0008, 0009, 0014,
    0015, 0016, 0017, 0018 and 0019. Amended in place before acceptance
    (the user chose this over `/record-decision`, since none had been
    accepted), after an agent review found gaps:
    - ADR-0005, ADR-0020: `xtask` may link only `core` and `codec`, so fuzz
      targets, fixture writers and crash workloads live in the tests of
      the crate they exercise, on a fuzz driver in `softshell_core::testing`;
      `xtask` runs them through `cargo`.
    - ADR-0010, ADR-0011: a write transaction reads the last appended
      commit, which may not be synced yet. Pipelining is kept, and this is
      documented: act on reads from a write transaction only after it
      commits.
    - ADR-0011: a failed SST write or sync in a flush or compaction also
      enters the background-error state, so a full disk fails commits
      instead of stalling them forever.
    - ADR-0012: every secondary index has its own keyspace, so dropping an
      index is an ordinary keyspace drop and never-reused ids rule out
      stale entries.
    - ADR-0013: `Timestamp` stays `i64` nanoseconds; its 1677–2262 range is
      stated, and input outside it is an error.
    - ADR-0021: keycode's version is recorded in the manifest's `format`
      record, and each catalog entry records its key-layout version.

---

## M0 — Foundation and guardrails

**Goal:** an empty but fully guarded workspace; the constraints are
enforced by tooling from day one.
**Decisions:** ADR-0002, ADR-0004, ADR-0005, ADR-0020.

- [x] **M0.1 — Workspace skeleton**
  - Depends: D0.1
  - Do:
    - Create every crate in the workspace map (`AGENTS.md`) as an empty
      library or binary with a crate-level doc comment and the license
      header. `softshell-cli`, `tidepool`, `xtask` and `softshell-bench`
      are binaries; the rest are libraries. Internal crates set
      `publish = false`.
    - Root `Cargo.toml`: `[workspace]` (resolver 3), `[workspace.package]`
      (edition 2024, version 0.1.0, license Apache-2.0, repository,
      `rust-version` = the pinned toolchain), `[workspace.dependencies]`
      for every workspace crate by path and version, and
      `[workspace.lints]` exactly as in `docs/01-architecture.md`.
    - `[profile.xtask]`: inherits `dev`, `opt-level = 3`.
    - `rust-toolchain.toml`: pin the current stable, with `rustfmt` and
      `clippy`. Record the version in a task note. It must be ≥ 1.89
      (`File::lock`).
    - `clippy.toml`: `allow-unwrap-in-tests = true`,
      `allow-expect-in-tests = true`.
    - `.cargo/config.toml` with the `xtask` alias.
    - `.gitignore`: add `fuzz/crashes/` (`target` is already covered);
      check that the existing `debug` pattern matches no crate path.
    - `#![allow(unsafe_code)]` in the `softshell-sys` and
      `softshell-platform` roots only.
    - `NOTICE` copied into each published crate's folder, with `LICENSE`.
  - Accept:
    - `cargo build --workspace` and
      `cargo clippy --workspace --all-targets -- -D warnings` succeed.
    - `Cargo.lock` lists only workspace packages.
  - Verify: `cargo build --workspace`;
    `cargo clippy --workspace --all-targets -- -D warnings`;
    `grep -c 'source = ' Cargo.lock` (expect 0)
  - Note: pinned Rust 1.99.1, the current stable (the user's choice,
    2026-11-01), in `rust-toolchain.toml` and as `rust-version`.
  - Note: `[workspace.dependencies]` lists the 16 library crates. The four
    binaries have no library target, so they cannot be dependencies.
  - Note: `publish = false` is set on `tidepool`, `xtask` and
    `softshell-bench`, the internal list in `docs/05-release.md`. The 17
    crates under `crates/` carry `LICENSE` and `NOTICE`. The release table
    in that file names only `softshelldb` and its dependencies for
    crates.io, so whether `softshell-proto`, `-server`, `-client` and `-cli`
    are published stays open until M12.5.
  - Note: `LICENSE` is copied as committed, with its appendix copyright
    line filled in (the user's choice). ADR-0004 says `LICENSE` stays the
    unmodified text; the two still need reconciling.
  - Note: the `.gitignore` pattern `debug` matches no crate path, but it
    would also hide any future directory named `debug`, such as a
    `src/debug/` module.
  - Note: in a scratch copy, a missing crate doc, `println!`, `unwrap()`,
    `unsafe` in `softshell-txn` and an `unsafe` block without `// SAFETY:`
    each failed `cargo clippy --workspace --all-targets -- -D warnings`; a
    commented `unsafe` block in `softshell-sys` compiled.

- [x] **M0.2 — core: bytes, varints, errors, limits**
  - Depends: M0.1
  - Do: `bytes::{ByteReader, ByteWriter}`, `varint` (canonical decoding),
    `error::{CoreError, ErrorContext}`, `limits::Limits` with the defaults
    in `docs/subsystems/core-and-codec.md`.
  - Accept:
    - Every `ByteReader` read past the end returns an error; none panics.
    - Varint round-trips for edge values; overlong and non-minimal
      encodings are rejected.
    - Every public item has docs.
  - Verify: `cargo test -p softshell-core`
  - Note: the API came from a design panel (three proposals, three judges,
    one synthesis) and was then checked by an adversarial review (five
    reviewers, each finding checked by a skeptic). Beyond the Do line it
    adds `Context<E>` (made by `ErrorContext`), `Chain`,
    `ByteReader::{with_base, sub_reader, split_tail, check_count}` and
    `varint::{read_at_most, read_len, read_count}`.
  - Note: the review agents reported about 138 million varint checks
    against an independent reference decoder, and a model-based harness
    over every `ByteReader` method (nested sub-readers, extreme offsets and
    sizes) in debug and release builds, with no panic, overflow or
    disagreement. Five weak spots in the tests they found are fixed; each
    planted bug is now caught.
  - Note: a checked count bounds how many items the input holds, not how
    much memory a decoder may reserve. Nested decoders (SDOC, JSON) must
    cap what they preallocate; `bytes` and `varint` say so.

- [x] **M0.3 — core: CRC32C, hashes, SHA-256, PCG32**
  - Depends: M0.2
  - Do: `crc32c` (slicing-by-8, `mask`/`unmask`), `hash::{fnv1a64,
    FastHasher, FastHashMap, FastHashSet}`, `sha256` (FIPS 180-4), `rng::Pcg32`.
  - Accept:
    - `crc32c(b"123456789") == 0xE3069283`; slicing-by-8 equals the
      bytewise reference on random inputs.
    - SHA-256 passes the NIST CAVP short- and long-message vectors
      (committed with their source noted).
    - `Pcg32` matches the PCG reference output for a fixed seed.
    - `FastHasher` output is pinned by a test.
  - Verify: `cargo test -p softshell-core`
  - Note: the NIST vectors (`SHA256ShortMsg.rsp`, `SHA256LongMsg.rsp`, 129
    vectors) were downloaded with the user's approval on 2026-11-02 and
    are committed with their source, checksums and terms in
    `crates/core/tests/vectors/sha256/README.md`; `.gitattributes` keeps
    their bytes exact.
  - Note: every pinned value (RFC 3720 CRC-32C vectors, FNV-1a, the
    `pcg32-demo` output, `FastHasher`, `Pcg32`'s derived outputs, digests
    of every length 0 to 256) was computed by an independent Python
    implementation, not taken from the code under test. The SHA-256
    constants are recomputed from the primes in a test.
  - Note: an adversarial review (five reviewers, each finding checked by
    a skeptic) found no wrong output. It found that `str` keys differing
    only in trailing NUL bytes collided in `FastHasher`, now fixed; that
    iteration-order and stream-independence claims were too strong, now
    corrected here and in the code; and seven untested paths, each now
    caught by a test.
  - Note: `Pcg32` streams with the same seed are correlated. The M0.4
    property harness should give each case its own well-mixed seed.

- [x] **M0.4 — core: ids, clock, civil time, logging, metrics, CLI, test harness**
  - Depends: M0.3
  - Do: `id::{Id, IdGenerator}` (entropy injected), `clock::{Clock,
    SystemClock, ManualClock, Timestamp}`, `civil` with RFC 3339, the `log`
    macros and sinks, `metrics`, `cli`, `testing::{check, check_sized,
    assert_approx!}`.
  - Accept:
    - `IdGenerator` is monotonic when the clock steps back (tested with
      `ManualClock`); text form round-trips.
    - `civil` matches known timestamps, including leap years, 1970,
      2038 and negative times.
    - RFC 3339 parsing accepts both ends of the `Timestamp` range and
      rejects a time one nanosecond outside either (ADR-0013).
    - Log file rotation works in a temporary directory under `target/`.
    - A failing `check` prints a reproducible seed; `check_sized` shrinks
      by size.
  - Verify: `cargo test -p softshell-core --all-features`
  - Note: user decisions (2026-11-03): the rotating log `FileSink` writes
    with `std::fs` inside core, since log files are not database files and
    need no `Vfs` crash semantics (recorded in the core spec; AGENTS.md
    rule 4 is unchanged); RFC 3339 output always has nine fraction digits.
  - Note: three reviewers critiqued the design before any code. Adopted:
    a concrete `EntropyError` (documents and graph cannot name the
    platform's error); the log macros under hidden names, re-exported from
    `log` (a crate with its own `mod error` could not otherwise import
    `error!`); a rotation that fills gaps and retries only after another
    `max_bytes`; messages rendered once, outside locks, capped at 16 KiB;
    a thread-local `log::capture` for tests; `try_set_logger`; `cli`
    errors for a missing option or command; wrapping per-case seeds.
  - Note: pinned values come from independent Python: the RFC 3339 text
    of one instant on every day from 1677 to 2262 (an aggregate), the
    Crockford forms of 1,000 ids, the per-case seeds, and which case fails
    and how far it shrinks in the harness tests.
  - Note: an adversarial review (four reviewers, each finding checked by
    a skeptic) confirmed 39 findings. Bugs fixed: `checked_add` and
    `ManualClock::advance` with steps of 2^63 ns or more; an abort when a
    thread-local destructor logs; a deadlock when a replaced logger's sink
    logs as it is dropped; single-dash options breaking order
    independence in `cli`; quantile ranks one too high for decimals such
    as 0.55; `approx_eq` true on overflow with a tolerance above 1; a cut
    message gaining text after the cut. Untested paths got tests, and 24
    of the bugs and mutants were re-planted afterwards: every one fails
    the suite.
  - Note: `cli`'s `NotUnicode` paths need an `OsString` that is not UTF-8,
    which core cannot build without platform code; a scratch program
    checked them by hand on Linux (a separate-token value and `_os`
    positionals pass the bytes through; `--out=<non-UTF-8>`, a subcommand
    and a text value give `NotUnicode`; a flag given `=<non-UTF-8>` gives
    `UnexpectedValue`). macOS and Windows run the suite from M0.8.
  - Note: no `dev` feature was added to core, so `--all-features` builds
    the default. docs/01-architecture.md lists allocation counters under
    core's `dev`, but a counting allocator needs `unsafe impl GlobalAlloc`,
    which core denies.

- [x] **M0.5 — codec: `Value`, JSON and JSONC**
  - Depends: M0.2, M0.4
  - Do: `Value`, the value comparator (ADR-0013), `json::parse`,
    `jsonc::parse`, the writers, extended JSON, base64 (RFC 4648), and the
    `value!` macro, per `docs/subsystems/core-and-codec.md`.
  - Accept:
    - RFC 8259's examples parse; the JSONC extensions, duplicate-key and
      depth-limit errors behave as specified, with line and column.
    - Property: the comparator is a total order (antisymmetric,
      transitive), and `Int`/`Float` comparison is exact near `±2^53` and
      `±2^63`.
    - Extended JSON round-trips every `Value` variant.
    - Malformed input returns errors; nothing panics.
  - Verify: `cargo test -p softshell-codec`
  - Note: user decision (2026-11-03): one canonical NaN (ADR-0022). NaN's
    sign and payload are not data; SDOC (M2.1) stores only the canonical
    quiet NaN, and M2.1's acceptance says so.
  - Note: three reviewers critiqued the design before the code. Adopted:
    no `From<Vec<u8>>`, since inference turned integer vectors into `Bytes`
    (callers write `Value::Bytes`); `value!` exported as `value::value`,
    with parenthesized keys unwrapped (they warned at the caller) and
    lists split one element per recursion level; `DuplicateKeyError`, and
    no key in any error message, so errors are safe to log; a reader of
    its own for each extended JSON form, so wrappers cannot nest, with an
    object's depth charged once its first key shows it is not a wrapper;
    shared scratch stacks that close each container into a vector of exact
    length; long number tokens rewritten before `from_str`, which returns
    infinity for `1` followed by 655,360 zeros and `e-655360`.
  - Note: pinned values come from an independent Python program that
    mirrors each generator call for call: the shortest f64 and f32 text of
    random bit patterns and of short decimals (exact fraction search, ties
    rounded up as Rust does, otherwise checked against `repr`), string
    escaping (`json.dumps(ensure_ascii=False)`), number parsing of short
    and long tokens (`float()`, and exact fractions for `f32`), the
    `Int`/`Float` order (Python's exact comparison, 20,468 pairs) and
    base64. RFC 8259's §13 examples and RFC 4648's §10 vectors are tested
    as published.
  - Note: measured: a `max_depth` of 256 works on a 2 MiB stack in a debug
    build for parsing, cloning, comparing, hashing, writing and dropping
    (512 works too, 1,024 overflows); a parsed value takes at most about 25
    times its text's peak memory (398 MiB for 16 MiB of nested one-element
    arrays, glibc).
  - Note: an adversarial review (four reviewers, each finding checked by a
    skeptic) confirmed 17 findings. Fixed: text ending inside an extended
    JSON form gave `ExtendedJson` instead of `UnexpectedEnd`; seven claims
    in docs or comments (the exponent `from_str` miscounts, the 768-digit
    halfway bound, the memory and pretty-output bounds, the order errors
    are reported in, the macro's recursion limit, a test's coverage); nine
    untested paths. 39 bugs and mutants, the review's among them, were
    re-planted one at a time afterwards: every one fails the suite.
  - Note: the `json` fuzz target and its corpus come with M0.7's driver.
    Until then a property test parses 20,000 mutated texts in both
    dialects, under default and tight limits, and rewrites every success.

- [ ] **M0.6 — xtask: guardrail checks**
  - Depends: M0.4, M0.5
  - Do: subcommands `fmt`, `clippy`, `test`, `deps` (+ an empty
    `xtask/deps-allowlist.txt`), `layers` (the matrix as a `const`),
    `unsafe-audit`, `headers`, `release-no-dev`, `features`, and `ci`
    running them all. Use `softshell_core::cli` and `softshell_codec::json`
    (to read `cargo metadata`).
  - Accept:
    - `cargo xtask ci` passes on a clean tree.
    - Each check fails when deliberately violated in a scratch copy, then
      reverted: a path dependency that breaks the matrix, an `unsafe`
      block in `softshell-txn`, a registry dependency, a file without the
      header. Record each demonstration in a task note.
  - Verify: `cargo xtask ci`, plus the demonstrations

- [ ] **M0.7 — core + xtask: fuzz smoke harness**
  - Depends: M0.6
  - Do: per `docs/04-quality.md` and ADR-0020:
    - the fuzz driver in `softshell_core::testing`: corpus loading,
      mutations, `catch_unwind`, the 2 s watchdog, findings directory,
      seeded and reproducible;
    - the `json` target as a test in `softshell-codec`, with a seed corpus
      in `fuzz/corpus/json/`;
    - `cargo xtask fuzz-smoke`, which runs registered targets through
      `cargo`;
    - `cargo xtask fixtures --new <format>` (`docs/04-quality.md`, "Format
      fixtures"), which runs the owning crate's fixture writer through
      `cargo`. M2.1 first uses it.
  - Accept:
    - `--secs 60 --target json` runs clean.
    - A planted panic in a scratch copy is found, saved once, and the run
      exits 1. Reverted afterwards.
  - Verify: `cargo xtask fuzz-smoke --secs 60`

- [ ] **M0.8 — CI on Linux, macOS and Windows**
  - Depends: M0.6, M0.7
  - Do: `.github/workflows/ci.yml` per `docs/04-quality.md` (the crash job
    joins in M3.8); actions pinned by full commit SHA with version
    comments; `permissions: contents: read`; `.github/dependabot.yml` for
    GitHub Actions only, weekly, 7-day cooldown; `cargo xtask workflows`;
    a CodeQL workflow for the workflows, and for Rust if CodeQL supports
    the pinned toolchain (record which in a task note).
  - Accept:
    - CI passes on every listed runner. Record the run URL in a task note.
    - `cargo xtask workflows` fails on a tag-pinned action in a scratch
      copy.
  - Verify: `cargo xtask ci`; the CI run on GitHub

**Exit criteria (M0):** `cargo xtask ci` is green on every CI runner; no
third-party package in `Cargo.lock`.

---

## M1 — Files and durability

**Goal:** a `Vfs` with exact, tested durability semantics on all three
OSes, and a fault-injecting one for crash tests.
**Decisions:** ADR-0006, ADR-0011.

- [ ] **M1.1 — platform: the `Vfs` trait, `MemVfs`, the conformance suite**
  - Depends: M0.4
  - Do: the traits in `docs/subsystems/platform.md`, `VfsError`,
    `MemVfs`, and a conformance suite that any implementation runs.
  - Accept: `MemVfs` passes the suite, including concurrent `read_exact_at`
    from 8 threads.
  - Verify: `cargo test -p softshell-platform`

- [ ] **M1.2 — platform (Linux): `OsVfs` and randomness**
  - Depends: M1.1
  - Do: `OsVfs` per `docs/03-platforms.md` ("Linux"); `fill_random` from
    `/dev/urandom`.
  - Accept: the conformance suite passes in a temporary directory under
    `target/`; two processes cannot lock one `LOCK` file (a test spawning
    a child process).
  - Verify: `cargo test -p softshell-platform`

- [ ] **M1.3 — platform (macOS): `OsVfs`, and `F_FULLFSYNC` verified**
  - Depends: M1.2
  - Do: read the pinned toolchain's `std` source for `sync_data` and
    `sync_all` on Apple targets and record what it calls in a task note.
    If it is not `F_FULLFSYNC`, declare `fcntl` in `softshell-sys`
    (`/add-os-binding`) and use it. Update `docs/03-platforms.md` with the
    finding.
  - Accept: the conformance suite passes on macOS.
  - Verify: on macOS, `cargo test -p softshell-platform`

- [ ] **M1.4 — sys + platform (Windows): `MoveFileExW`, share modes, `ProcessPrng`**
  - Depends: M1.2
  - Do: declare `MoveFileExW` and `ProcessPrng` in `softshell-sys`
    (`/add-os-binding`); `OsVfs` per `docs/03-platforms.md` ("Windows");
    `fill_random` via `ProcessPrng`.
  - Accept: the conformance suite passes on Windows, including removing a
    file that another handle holds open.
  - Verify: on Windows, `cargo test -p softshell-platform`;
    `cargo xtask unsafe-audit`

- [ ] **M1.5 — platform: `FaultVfs`**
  - Depends: M1.1
  - Do: `FaultVfs` per `docs/04-quality.md`: durable image and pending
    writes, `crash()`, tearing at 512-byte boundaries, `Unix` and `Ntfs`
    directory profiles, error schedules, the operation counter; behind
    `cfg(test)` and the `fault-injection` feature.
  - Accept:
    - It passes the conformance suite when no crash or error is scheduled.
    - Unit tests show exactly which writes survive a crash, with and
      without tearing, in both profiles.
  - Verify: `cargo test -p softshell-platform --features fault-injection`

**Exit criteria (M1):** the conformance suite passes on Linux, macOS and
Windows in CI; `docs/03-platforms.md` states what macOS `std` does.

---

## M2 — Binary formats

**Goal:** every byte format the engine writes, each fuzzed.
**Decisions:** ADR-0013, ADR-0021, ADR-0022.

- [ ] **M2.1 — codec: SDOC**
  - Depends: M0.5, M0.7
  - Do: SDOC encode and decode per `docs/subsystems/core-and-codec.md`;
    path lookup over SDOC bytes without full decoding; fuzz target `sdoc`.
  - Accept:
    - Property: `decode(encode(v)) == v` (floats bit-exact, except that
      every NaN is written as the canonical NaN, ADR-0022) and
      `encode(decode(b)) == b` for every `b` that decodes; a non-canonical
      NaN is rejected.
    - Limits are enforced before allocation (a declared 4 GiB string in a
      10-byte input is an error, not an allocation).
    - Fixtures in `tests/fixtures/sdoc/v1/`.
  - Verify: `cargo test -p softshell-codec`;
    `cargo xtask fuzz-smoke --target sdoc --secs 60`

- [ ] **M2.2 — codec: keycode**
  - Depends: M0.5, M0.7
  - Do: `keycode::{encode, encode_desc, decode}` per the spec; fuzz target
    `keycode`.
  - Accept: the property tests in the spec pass, including mixed
    `Int`/`Float` near `±2^53` and `±2^63`, NaN, `-0.0`, descending
    fields, and tuples of every type.
  - Verify: `cargo test -p softshell-codec`;
    `cargo xtask fuzz-smoke --target keycode --secs 60`

- [ ] **M2.3 — codec: SLZ**
  - Depends: M0.4, M0.7
  - Do: SLZ compressor and decompressor per the spec; fuzz target `slz`
    (which also compresses each input and checks the round trip).
  - Accept:
    - Round-trips on random, repetitive, empty and incompressible inputs.
    - Ratio and speed measured on a generated text corpus and recorded in a
      task note (no bench harness yet: a timed test in release mode).
  - Verify: `cargo test -p softshell-codec`;
    `cargo xtask fuzz-smoke --target slz --secs 60`

- [ ] **M2.4 — codec: record framing**
  - Depends: M0.3, M0.7
  - Do: `record::{Writer, Reader}` per the spec; fuzz target `record`.
  - Accept:
    - Records of every size from 0 to 3 blocks round-trip, at every block
      offset.
    - Truncation and bit flips in the last block yield `TornTail`; elsewhere
      `Corruption`, with the offset.
  - Verify: `cargo test -p softshell-codec`;
    `cargo xtask fuzz-smoke --target record --secs 60`

- [ ] **M2.5 — codec: `ToValue`, `FromValue`, `record!`**
  - Depends: M0.5
  - Do: the conversion traits for `std` types (integers, floats, `String`,
    `Vec<T>`, `Option<T>`, `BTreeMap<String, T>`, tuples up to 8), and the
    `record!` `macro_rules!` macro for plain structs with named fields.
  - Accept: structs with nested structs, options and vectors round-trip;
    a missing or mistyped field is an error naming the field path.
  - Verify: `cargo test -p softshell-codec`

**Exit criteria (M2):** every format has fixtures, fuzz targets and
documented limits.

---

## M3 — Storage engine I: durable writes and reads

**Goal:** a crash-safe LSM with WAL, memtable, SSTs and flush, proven by
crash tests before any compaction exists.
**Decisions:** ADR-0007, ADR-0008, ADR-0009, ADR-0011.

- [ ] **M3.1 — storage: write batches and the WAL**
  - Depends: M1.1, M2.4
  - Do: `WriteBatch`, the WAL batch payload, segment writer and reader,
    and the recovery rules in `docs/subsystems/storage-engine.md`.
  - Accept: batches written, synced and re-read across segment rolls;
    a torn tail in the last segment is truncated and logged; damage
    elsewhere is `Corruption`.
  - Verify: `cargo test -p softshell-storage`

- [ ] **M3.2 — storage: the memtable**
  - Depends: M0.4
  - Do: the persistent copy-on-write B-tree (ADR-0009): insert, get,
    ordered iteration in both directions from any bound, size accounting,
    root publication.
  - Accept:
    - Model test against `BTreeMap` under random operations.
    - A reader iterating an old root sees no change while the writer
      publishes 10,000 new roots.
  - Verify: `cargo test -p softshell-storage`
  - Note (D0.1 review, ADR-0009): a reader holding an old root keeps every
    node replaced since then alive, so counting bytes per commit
    understates memory while long readers exist. Decide how that memory
    is bounded or accounted. Series ingest (M8.6, ≥ 1M points/s, about
    1 µs per insert) will be this design's hardest load.

- [ ] **M3.3 — storage: the write path, group commit and snapshots**
  - Depends: M3.1, M3.2, M1.5
  - Do: `Storage` over WAL + memtable only: the commit queue, sequence
    assignment, group commit, `Sync::{Always, Interval, Never}`,
    visible-after-durable publication, background-error state, `get` at a
    snapshot, reopen by WAL replay.
  - Accept:
    - 16 threads committing with `Sync::Always` share syncs (group sizes
      > 1 recorded in stats).
    - A `FaultVfs` sync failure puts the database in the background-error
      state: writes fail, reads continue.
    - A commit is not visible before its sync completes (a `FaultVfs`
      that blocks syncs).
  - Verify: `cargo test -p softshell-storage`

- [ ] **M3.4 — storage: SST files and bloom filters**
  - Depends: M1.1, M2.3, M0.3
  - Do: the SST writer and reader per the spec; bloom filters; fuzz targets
    `sst_block` and `sst_footer`.
  - Accept:
    - Files of 0, 1 and 100,000 entries round-trip; seeks to every key and
      between keys are correct.
    - Measured bloom false-positive rate within 0.5 points of 1% at 10
      bits per key.
    - Two writes of the same input are byte-identical.
    - Fixtures in `tests/fixtures/sst/v1/`.
  - Verify: `cargo test -p softshell-storage`;
    `cargo xtask fuzz-smoke --target sst_block --secs 60`;
    `cargo xtask fuzz-smoke --target sst_footer --secs 60`

- [ ] **M3.5 — storage: manifest, flush and recovery**
  - Depends: M3.3, M3.4
  - Do: version edits, the manifest log, `CURRENT`, `IDENTITY`,
    `VersionSet`; flush of immutable memtables to L0 by the flush thread;
    the full open sequence; removal of unreferenced files and `tmp/`;
    fuzz target `manifest_edit`.
  - Accept:
    - Reopen after flushes and WAL replay returns every committed key.
    - Files are written, synced, renamed, the directory synced, and only
      then referenced (a test asserts the `FaultVfs` operation order).
    - Manifest rollover at a small configured size works.
    - A failed SST write or sync during a flush puts the database in the
      background-error state, and a commit stalled behind the flush fails
      with `Error::Background` instead of waiting (ADR-0011).
    - Fixtures in `tests/fixtures/manifest/v1/` and `tests/fixtures/wal/v1/`.
  - Verify: `cargo test -p softshell-storage`;
    `cargo xtask fuzz-smoke --target manifest_edit --secs 60`

- [ ] **M3.6 — storage: iterators and range scans**
  - Depends: M3.5
  - Do: merging, level and bounded iterators; forward and reverse; prefix
    scans; version pinning.
  - Accept: model test (random puts, deletes, flushes, snapshots, scans in
    both directions, reopens) against `BTreeMap` snapshots.
  - Verify: `cargo test -p softshell-storage`

- [ ] **M3.7 — storage: crash-point, model and corruption tests**
  - Depends: M3.6, M1.5
  - Do: the crash-point test (both directory profiles, with and without
    tearing), the corruption tests, and `xtask crash --exhaustive`, per
    `docs/04-quality.md`.
  - Accept: every crash point of the standard workload passes the
    invariants; every single-bit flip in the fixtures is detected.
  - Verify: `cargo test -p softshell-storage`;
    `cargo xtask crash --exhaustive`

- [ ] **M3.8 — xtask: process-kill crash runs**
  - Depends: M3.7, M1.2
  - Do: `cargo xtask crash --minutes N` per `docs/04-quality.md`; add the
    5-minute run to the Linux CI job.
  - Accept: a 10-minute run is clean; a planted bug (skipping the WAL
    sync in a scratch copy) is found. Reverted afterwards.
  - Verify: `cargo xtask crash --minutes 10`

**Exit criteria (M3):** crash-point and process-kill runs are clean on
Linux; no acknowledged commit has ever been lost in testing.

---

## M4 — Storage engine II: compaction and caches

**Goal:** a storage engine that holds its budgets over time.
**Decisions:** ADR-0007, ADR-0008.

- [ ] **M4.1 — storage: leveled compaction**
  - Depends: M3.7
  - Do: scoring, picking, merging with the version-drop rules, output
    splitting, installation, input removal, compaction threads.
  - Accept:
    - The M3.6 model test and the M3.7 crash-point test pass with
      compaction running (crash points now include compaction).
    - Long random workloads keep L1+ non-overlapping (checked after every
      install under `dev`).
    - Snapshots keep their versions through compaction.
    - A failed output write or sync during a compaction puts the database
      in the background-error state (ADR-0011).
  - Verify: `cargo test -p softshell-storage --features dev`;
    `cargo xtask crash --exhaustive`

- [ ] **M4.2 — storage: backpressure**
  - Depends: M4.1
  - Do: the stall rules in the spec, with logging and stats.
  - Accept: a write burst against a throttled `FaultVfs` slows, stops and
    resumes; no commit fails; stall time is reported.
  - Verify: `cargo test -p softshell-storage`

- [ ] **M4.3 — storage: block cache and table cache**
  - Depends: M3.6
  - Do: the sharded LRU block cache and the table cache per the spec.
  - Accept: the cache never exceeds capacity plus one block; hit rates are
    reported; the table cache closes evicted files.
  - Verify: `cargo test -p softshell-storage`

- [ ] **M4.4 — storage: compaction filters**
  - Depends: M4.1
  - Do: the `CompactionFilter` hook by key prefix, registered at run time
    (not persisted; `txn` re-registers filters on open).
  - Accept: a prefix whose filter drops everything disappears after
    compaction; a live snapshot still sees filtered keys until released;
    filters are applied identically on every level.
  - Verify: `cargo test -p softshell-storage`

- [ ] **M4.5 — storage: checkpoint, check and stats**
  - Depends: M4.1, M4.3
  - Do: `checkpoint`, `check`, and the stats in the spec.
  - Accept: a checkpoint taken during writes opens with exactly the state
    at its sequence; `check` reports each corruption-test fault with file
    and offset.
  - Verify: `cargo test -p softshell-storage`

- [ ] **M4.6 — bench: the harness and storage workloads**
  - Depends: M4.2, M4.3, M4.5
  - Do: `softshell-bench` and `cargo xtask bench` per
    `docs/subsystems/cli-and-tools.md`; the YCSB-style workloads, bulk load
    and recovery time; name the reference machine in
    `docs/00-overview.md` and store its baseline.
  - Accept: every storage budget row in `docs/00-overview.md` measured on
    the reference machine; each met, or a note saying by how much it is
    missed and a backlog item or ADR proposal for it.
  - Verify: `cargo xtask bench storage`

- [ ] **M4.7 — cli: `softshell check` and `softshell dump`**
  - Depends: M4.5
  - Do: the `check` and `dump` commands over `Storage` directly (the
    facade does not exist yet).
  - Accept: `check` exits 1 on each corruption fixture; `dump` decodes every
    file type in the fixtures.
  - Verify: `cargo test -p softshell-cli`

**Exit criteria (M4):** the storage budgets are measured and met or have a
recorded plan; a 1-hour mixed workload with compaction keeps space
amplification under budget.

---

## M5 — Transactions and the facade

**Goal:** the API every model and every user builds on.
**Decisions:** ADR-0010, ADR-0012.

- [ ] **M5.1 — txn: keyspaces and read transactions**
  - Depends: M4.4, M3.6
  - Do: `TxnDb`, `Keyspace`, `ReadTxn`, the snapshot registry,
    `open_in_memory`, the filter registry.
  - Accept: scans never cross a keyspace boundary (property test on
    adjacent ids, including `u32::MAX`).
  - Verify: `cargo test -p softshell-txn`
  - Note (D0.1 review, ADR-0005): `txn` may not depend on
    `softshell-platform`, so `open_in_memory` and the M5.3 tests that use
    `FaultVfs` must reach both through `softshell-storage` (for example a
    re-export, with `storage` passing the `fault-injection` feature
    through). Settle this before writing `open_in_memory`.

- [ ] **M5.2 — txn: write transactions**
  - Depends: M5.1
  - Do: `WriteTxn` per `docs/subsystems/transactions.md`: the writer lock
    with timeouts, read-your-writes, `max_txn_bytes`, commit, rollback,
    rollback on drop and panic.
  - Accept: the read-your-writes model test passes; a panicking closure
    releases the lock.
  - Verify: `cargo test -p softshell-txn`

- [ ] **M5.3 — txn: commit pipelining, isolation and the durable-commit budget**
  - Depends: M5.2, M4.6
  - Do: release the writer lock after the WAL append; the isolation
    history test; the multi-keyspace crash test; a bench of durable commits
    from 16 threads.
  - Accept: the pipelining and history tests pass; the durable-commit
    budget is measured on the reference machine.
  - Verify: `cargo test -p softshell-txn`; `cargo xtask bench txn`

- [ ] **M5.4 — facade: `Database`, the catalog and the key-value API**
  - Depends: M5.2, M2.1, M2.2
  - Do: `Database::{open, open_in_memory, close}`, the catalog in
    keyspace 0 (keyspace entries only for now), `tx.keyspace(name)`,
    create and drop, `softshelldb::Error`, the prelude, `examples/kv.rs`.
  - Accept: catalog model test with reopens; a dropped keyspace's name is
    reusable and its old keys are invisible at once.
  - Verify: `cargo test -p softshelldb`;
    `cargo run -p softshelldb --example kv`

- [ ] **M5.5 — facade: maintenance thread and stats**
  - Depends: M5.4
  - Do: the maintenance thread with time-budgeted jobs,
    `Options::maintenance(Manual)`, `run_maintenance`, the dropped-keyspace
    sweep, `db.stats()`, `db.check()` (storage checks for now).
  - Accept: a dropped keyspace's keys are swept; `close()` stops the
    thread within one job budget.
  - Verify: `cargo test -p softshelldb`

**Exit criteria (M5):** an embedded key-value database with serializable
transactions, documented with a runnable example.

---

## M6 — Documents

**Goal:** collections, indexes and queries, used by Tidepool.
**Decisions:** ADR-0013, ADR-0014.

- [ ] **M6.1 — expr: paths, filters, parser and evaluator**
  - Depends: M2.1, M0.7
  - Do: paths, the filter operators and `index_bounds` per
    `docs/subsystems/expressions.md`; fuzz target `query_doc`.
  - Accept: the evaluator table tests and the `index_bounds` soundness
    property pass.
  - Verify: `cargo test -p softshell-expr`;
    `cargo xtask fuzz-smoke --target query_doc --secs 60`

- [ ] **M6.2 — documents: collections and point operations**
  - Depends: M5.4, M2.2
  - Do: the key layout, `_id` policy, `insert`, `insert_many`, `get`,
    `replace`, `delete_by_id`, the stats key.
  - Accept: model test with reopens; documents over the size limit and
    invalid field names are rejected.
  - Verify: `cargo test -p softshell-documents`

- [ ] **M6.3 — documents: `find` by scan**
  - Depends: M6.1, M6.2
  - Do: filter, projection, sort (in memory, bounded), skip, limit,
    cursor tokens, `count`.
  - Accept: the documents model test's `find` equals the naive evaluator.
  - Verify: `cargo test -p softshell-documents`

- [ ] **M6.4 — expr + documents: updates**
  - Depends: M6.3
  - Do: the update operators; `update` with `upsert` and `multi`; `delete`
    by filter.
  - Accept: operator table tests; `$inc` overflow is an error; `_id`
    cannot change.
  - Verify: `cargo test -p softshell-expr -p softshell-documents`

- [ ] **M6.5 — documents: secondary indexes and the planner**
  - Depends: M6.4, M4.6
  - Do: compound, unique, sparse and multikey indexes; maintenance on every
    write; index builds; the planner and `explain()`; the indexed-query
    bench.
  - Accept:
    - Every `find` in the model test returns the same results with each
      index and with none.
    - Planner table tests pass.
    - The indexed-equality budget is measured.
  - Verify: `cargo test -p softshell-documents`;
    `cargo xtask bench documents`

- [ ] **M6.6 — facade: collections and indexes**
  - Depends: M6.5, M5.5
  - Do: catalog entries for collections and indexes; `tx.collection`;
    create and drop; index builds in `building` state, resumed after a
    crash; collection checks in `db.check()`; fuzz target `catalog_entry`.
  - Accept: a crash during an index build resumes and completes; `check`
    finds a deliberately removed index entry; an index dropped and
    re-created under the same name never returns the old entries, and
    dropping a collection drops its index keyspaces (ADR-0012).
  - Verify: `cargo test -p softshelldb`;
    `cargo xtask fuzz-smoke --target catalog_entry --secs 60`

- [ ] **M6.7 — expr + documents: aggregation**
  - Depends: M6.3
  - Do: the pipeline in `docs/subsystems/expressions.md`, bounded by
    `max_groups`.
  - Accept: results equal a naive implementation on random collections.
  - Verify: `cargo test -p softshell-expr -p softshell-documents`

- [ ] **M6.8 — cli: REPL, import and export**
  - Depends: M6.6, M4.7
  - Do: `softshell open`, `import`, `export`; move `check` and `dump` onto
    the facade.
  - Accept: export then import into an empty database reproduces every
    document byte for byte (SDOC).
  - Verify: `cargo test -p softshell-cli`

- [ ] **M6.9 — tidepool: notes**
  - Depends: M6.6
  - Do: Tidepool's notes features (`docs/subsystems/cli-and-tools.md`) and
    `tidepool seed`.
  - Accept: an integration test runs the notes commands on a seeded corpus.
  - Verify: `cargo test -p tidepool`; `cargo run -p tidepool -- seed --notes 1000`

**Exit criteria (M6):** documents with indexes, used by Tidepool; the
documents budget measured.

---

## M7 — Graph

**Goal:** property graphs with traversals.
**Decisions:** ADR-0012.

- [ ] **M7.1 — graph: nodes, edges and adjacency**
  - Depends: M5.4, M2.2
  - Do: the key layout, node and edge operations, cascade removal, stats.
  - Accept: model test with reopens; the adjacency consistency check.
  - Verify: `cargo test -p softshell-graph`

- [ ] **M7.2 — graph: neighbors and traversals**
  - Depends: M7.1, M6.1
  - Do: `neighbors`, `nodes_with_label`, BFS and DFS with limits, filters
    and paths.
  - Accept: traversals equal a naive BFS; limits truncate as specified.
  - Verify: `cargo test -p softshell-graph`

- [ ] **M7.3 — graph: shortest paths**
  - Depends: M7.2
  - Do: unweighted BFS and Dijkstra.
  - Accept: results equal a reference implementation on random graphs;
    negative weights are errors.
  - Verify: `cargo test -p softshell-graph`

- [ ] **M7.4 — facade: graphs**
  - Depends: M7.3, M5.5
  - Do: catalog entries, `tx.graph`, graph checks in `db.check()`, the
    k-hop bench.
  - Accept: the 2-hop budget is measured.
  - Verify: `cargo test -p softshelldb`; `cargo xtask bench graph`

- [ ] **M7.5 — tidepool: links**
  - Depends: M7.4, M6.9
  - Do: `[[links]]`, backlinks, "notes within 2 links".
  - Accept: integration test on a seeded corpus.
  - Verify: `cargo test -p tidepool`

---

## M8 — Time-series

**Goal:** compressed series with windows, retention and rollups.
**Decisions:** ADR-0015.

- [ ] **M8.1 — codec: series chunks**
  - Depends: M0.4, M0.7
  - Do: the chunk codec per `docs/subsystems/core-and-codec.md`; fuzz
    target `series_chunk`; fixtures in `tests/fixtures/series_chunk/v1/`.
  - Accept: round-trips on regular, jittered, constant, random and
    extreme series; bytes per point on a regular series recorded.
  - Verify: `cargo test -p softshell-codec`;
    `cargo xtask fuzz-smoke --target series_chunk --secs 60`

- [ ] **M8.2 — series: registry, tag index and writes**
  - Depends: M5.4, M2.2
  - Do: series ids, the tag index, raw-point writes, field kinds.
  - Accept: model test of writes and point reads with reopens.
  - Verify: `cargo test -p softshell-series`

- [ ] **M8.3 — series: queries**
  - Depends: M8.2, M8.1, M6.1
  - Do: range reads merging chunks and raw points, windows, aggregates,
    group by, fill, bounds.
  - Accept: results equal a naive computation over sorted vectors.
  - Verify: `cargo test -p softshell-series`

- [ ] **M8.4 — series: chunking**
  - Depends: M8.3, M5.5
  - Do: `chunk_closed_buckets`, including late points and full buckets.
  - Accept: every query returns the same results before and after
    chunking.
  - Verify: `cargo test -p softshell-series`

- [ ] **M8.5 — series: retention and rollups**
  - Depends: M8.4, M4.4
  - Do: retention hiding and removal (with the compaction filter);
    rollup rules, progress and recompute of late windows.
  - Accept: `ManualClock` tests for both.
  - Verify: `cargo test -p softshell-series`

- [ ] **M8.6 — facade: series sets**
  - Depends: M8.5
  - Do: catalog entries, `tx.series`, maintenance jobs, the ingest bench.
  - Accept: the ingest and bytes-per-point budgets are measured.
  - Verify: `cargo test -p softshelldb`; `cargo xtask bench series`

- [ ] **M8.7 — tidepool: activity**
  - Depends: M8.6, M6.9
  - Do: edit activity per note and tag; the weekly terminal chart.
  - Accept: integration test on a seeded corpus.
  - Verify: `cargo test -p tidepool`

---

## M9 — Full-text search

**Goal:** analyzed text indexes on document fields with BM25.
**Decisions:** ADR-0016.

- [ ] **M9.1 — text: the analyzer**
  - Depends: M0.4
  - Do: tokenizing, lowercasing, folding and stopwords per
    `docs/subsystems/text-search.md`.
  - Accept: token tables, including non-Latin scripts and positions after
    stopwords.
  - Verify: `cargo test -p softshell-text`

- [ ] **M9.2 — text: the Porter2 stemmer**
  - Depends: M9.1
  - Do: the English stemmer; copy the Snowball vocabulary and expected
    output into the tests with their license and notice, after confirming
    the license.
  - Accept: every vocabulary word stems to its expected output.
  - Verify: `cargo test -p softshell-text`

- [ ] **M9.3 — text: index storage and maintenance**
  - Depends: M9.2, M5.4
  - Do: the key layout, insert, update and delete by re-analysis, stats.
  - Accept: the consistency check passes after random operations and
    reopens.
  - Verify: `cargo test -p softshell-text`

- [ ] **M9.4 — text: queries and BM25**
  - Depends: M9.3
  - Do: term, phrase, prefix and boolean queries; BM25; top-k.
  - Accept: the scoring model test equals brute-force BM25.
  - Verify: `cargo test -p softshell-text`

- [ ] **M9.5 — facade: text indexes on collections**
  - Depends: M9.4, M6.6
  - Do: catalog entries, glue on every document write, builds, checks,
    the BM25 bench.
  - Accept: the cross-model crash test includes text; the text budget is
    measured.
  - Verify: `cargo test -p softshelldb`; `cargo xtask bench text`

- [ ] **M9.6 — tidepool: search**
  - Depends: M9.5, M6.9
  - Do: `tidepool search`.
  - Accept: integration test on a seeded corpus.
  - Verify: `cargo test -p tidepool`

---

## M10 — Vector search

**Goal:** exact and HNSW nearest-neighbor indexes, filtered and hybrid
search.
**Decisions:** ADR-0017.

- [ ] **M10.1 — vector: metrics and the flat index**
  - Depends: M5.4, M0.3
  - Do: the metrics, validation and the flat index per
    `docs/subsystems/vector-search.md`.
  - Accept: model test against brute force over a `Vec`.
  - Verify: `cargo test -p softshell-vector`

- [ ] **M10.2 — vector: HNSW**
  - Depends: M10.1
  - Do: level assignment, insertion with the selection heuristic, search.
  - Accept: recall@10 ≥ 0.95 on the 5k × 32 test set at the default `ef`;
    deterministic builds.
  - Verify: `cargo test -p softshell-vector`
  - Note (D0.1 review, ADR-0017): every node a search visits is a
    key-value read. A rough estimate at 100k × 128 is about 2,000 reads
    per query, or 4–10 ms against the 5 ms budget measured in M10.4, before
    any distance math. Plan a bounded cache of decoded nodes here rather
    than discovering the miss in M10.4.

- [ ] **M10.3 — vector: deletes and rebuilds**
  - Depends: M10.2, M5.5
  - Do: deleted flags, lazy repair, the rebuild job with the catalog switch.
  - Accept: recall holds after deleting 30% and rebuilding; a crash during
    a rebuild leaves the old index usable.
  - Verify: `cargo test -p softshell-vector -p softshelldb`

- [ ] **M10.4 — facade: vector indexes on collections**
  - Depends: M10.3, M6.6
  - Do: catalog entries, glue, builds, filtered search, checks, the vector
    bench.
  - Accept: the cross-model crash test includes vectors; the vector budget
    is measured at 100k × 128.
  - Verify: `cargo test -p softshelldb`; `cargo xtask bench vector`

- [ ] **M10.5 — facade: hybrid search**
  - Depends: M10.4, M9.5
  - Do: reciprocal rank fusion of text and vector results with a filter.
  - Accept: fusion table tests; results are deterministic.
  - Verify: `cargo test -p softshelldb`

- [ ] **M10.6 — tidepool: similar notes**
  - Depends: M10.5, M6.9
  - Do: the toy embedder, `tidepool similar`, hybrid `tidepool search`.
  - Accept: integration test on a seeded corpus.
  - Verify: `cargo test -p tidepool`

**Exit criteria (M10):** all six data models are in one database, one
transaction, and Tidepool.

---

## M11 — Server, protocol, client

**Goal:** the same database over the network, loopback-only until TLS.
**Decisions:** ADR-0018, ADR-0019.

- [ ] **M11.1 — proto: framing, handshake and messages**
  - Depends: M2.1, M0.7
  - Do: the protocol per `docs/subsystems/server-and-protocol.md`; fuzz
    target `proto_frame`; fixtures in `tests/fixtures/proto/v1/`.
  - Accept: every message round-trips; oversize and malformed frames are
    errors.
  - Verify: `cargo test -p softshell-proto`;
    `cargo xtask fuzz-smoke --target proto_frame --secs 60`

- [ ] **M11.2 — facade + cli: API tokens**
  - Depends: M5.4, M1.2, M0.3
  - Do: tokens in keyspace 1 per ADR-0019; `softshell admin token`.
  - Accept: a created token authenticates, a revoked one does not; the
    stored form is the SHA-256 hash; comparison is constant-time (code
    review, and a test that the comparison reads every byte).
  - Verify: `cargo test -p softshelldb -p softshell-cli`

- [ ] **M11.3 — server: listener, sessions and limits**
  - Depends: M11.1, M11.2
  - Do: the listener with the loopback-only rule, the connection pool,
    handshake and auth, timeouts, limits, roles, config loading (fuzz
    target `config`), logging.
  - Accept: the bind-refusal, auth, role, limit and timeout tests pass.
  - Verify: `cargo test -p softshell-server`;
    `cargo xtask fuzz-smoke --target config --secs 60`
  - Note (D0.1 review, ADR-0018): with the loopback-only rule, the server
    cannot be reached from outside a container, because a 127.0.0.1 bind
    is not reachable through a published port. Say so in the server docs;
    it lasts until M11.7.

- [ ] **M11.4 — server: every operation, cursors and interactive transactions**
  - Depends: M11.3, M7.4, M8.6, M10.5
  - Do: handlers for every message; cursors with expiry; `begin`/`commit`
    with the duration limit; the overhead bench.
  - Accept: a dropped connection aborts its transaction and frees the
    writer lock; the overhead budget is measured.
  - Verify: `cargo test -p softshell-server`; `cargo xtask bench server`

- [ ] **M11.5 — client and CLI**
  - Depends: M11.4, M6.8
  - Do: `softshell-client`; `softshell serve` and `softshell connect`.
  - Accept: the server integration tests run through the client.
  - Verify: `cargo test -p softshell-client -p softshell-cli`

- [ ] **M11.6 — TLS: the dependency decision**
  - Depends: M11.3
  - Do: run `/evaluate-dependency` for TLS (candidates in register item 13)
    and present it. The user decides. Record the decision in the register
    and an ADR.
  - Accept: the register row for TLS is `Approved` or `Rejected`.
  - Verify: the evaluation file exists and the register is updated

- [ ] **M11.7 — server + client: TLS and remote binding**
  - Depends: M11.6 (only if approved)
  - Do: TLS on the server and client behind a `tls` feature, per the ADR;
    lift the loopback-only rule when TLS is configured, and only then.
  - Accept: a non-loopback bind without TLS is still refused; with TLS, the
    integration tests pass over a non-loopback address.
  - Verify: `cargo test -p softshell-server -p softshell-client --features tls`

---

## M12 — Hardening and the first release

**Goal:** `v0.1.0`.

- [ ] **M12.1 — fuzz every decoder**
  - Depends: M11.5
  - Do: run every fuzz target for 300 s; fix every finding with a
    regression seed in its corpus.
  - Accept: a clean 300-s run of every target, recorded in a task note.
  - Verify: `cargo xtask fuzz-smoke --secs 300`

- [ ] **M12.2 — soak, and the coverage-guided fuzzing decision**
  - Depends: M11.5
  - Do: `cargo xtask soak`; a 1-hour soak with Tidepool's seeded workload;
    present register item 32 for a decision.
  - Accept: no memory growth beyond configured caches, no file-handle leak,
    compaction keeps up; register item 32 decided.
  - Verify: `cargo xtask soak --minutes 60`

- [ ] **M12.3 — the per-OS release checklist**
  - Depends: M12.1, M12.2
  - Do: automate what `docs/03-platforms.md` ("Per-OS verification
    checklist") allows; run the whole checklist on each OS.
  - Accept: the checklist passes on Linux, macOS and Windows for one commit.
  - Verify: the checklist, on each OS

- [ ] **M12.4 — user documentation**
  - Depends: M11.5
  - Do: README as a user guide; one example per model under
    `crates/softshelldb/examples/`; rustdoc complete with examples on the
    main types.
  - Accept: `cargo doc --workspace --no-deps` has no warnings; every
    example runs.
  - Verify: `cargo doc --workspace --no-deps`;
    `cargo run -p softshelldb --example <each>`

- [ ] **M12.5 — the release pipeline**
  - Depends: M12.3, M12.4
  - Do: confirm and reserve the crate names (`docs/05-release.md`);
    `cargo xtask crates` and `cargo xtask dist`; the release workflow;
    `CHANGELOG.md`; the CLI binary-size budget.
  - Accept: a dry run of the release workflow on a fork or a test tag
    produces every artifact without publishing.
  - Verify: `cargo xtask crates`; `cargo xtask dist`

**Exit criteria (M12):** the maintainer tags `v0.1.0` (never an agent).

---

## Backlog (M13+, each needs a trigger or a decision)

| Item | Trigger / reason |
| :- | :- |
| Single-leader replication (WAL shipping to read replicas) | A user needs high availability or read scaling (ADR-0003) |
| Multi-writer optimistic transactions | Writer-lock wait above 50% of commit latency at the M5.3 load (ADR-0010) |
| Range tombstones | Compaction filters cannot express a needed bulk delete |
| Separate trees per keyspace (column families) | Series retention or ingest dominates compaction I/O (ADR-0012) |
| Time-windowed compaction for series keyspaces | Same as above |
| Key-value separation for large values | Values over 64 KiB are common in a real workload |
| Partitioned index and filter blocks | Table-cache memory for indexes exceeds 10% of the block cache |
| `O_DIRECT` / unbuffered I/O | Page-cache copies above 20% of read CPU (ADR-0008) |
| `io_uring` on Linux | I/O submission overhead shows in profiles |
| Hardware CRC32C | Checksumming above 5% of read CPU (register item 9) |
| SIMD distance kernels | Distance computation above 50% of vector query time (register item 23) |
| Arena skiplist memtable | Memtable inserts above 30% of commit CPU (ADR-0009) |
| Covered queries (index-only) | Document fetches dominate indexed query time |
| Cost-based planner with statistics | Plan regressions that hints cannot fix (ADR-0014) |
| `$regex` | User request (register item 19) |
| Document TTL | User request |
| Schema validation per collection | User request |
| Change streams (CDC) | A user needs to react to writes; also the basis for replication |
| Cypher-like graph pattern queries | Traversal builders become unwieldy for Tidepool or users |
| Graph nodes bound to collections (enforced) | Dangling node ids reported as bugs |
| WAND / block-max pruning for text | Text top-k misses its budget at 1M documents |
| More analyzers and languages; UAX #29 segmentation | Indexing a non-English or non-whitespace language |
| Fuzzy text queries and highlighting | User request |
| Deferred vector indexing (queued, exact search of pending vectors) | HNSW inserts hold the writer lock too long at the M10.4 load |
| Filtered HNSW (filter-aware traversal) | Post-filtering exhausts `max_ef` on selective filters |
| Vector quantization (SQ8, PQ) | Vector storage or cache size beyond budget |
| Geospatial indexes | User request (needs an ADR; non-goal today) |
| Encryption at rest | User requirement (register item 14, criterion 2) |
| Graceful signal handling in the server | Operators need clean shutdown on `SIGTERM` |
| Unix domain sockets for the server | Local deployments want filesystem permissions instead of tokens |
| HTTP/JSON API and a metrics endpoint | Integration with tools that cannot speak the binary protocol |
| Client connection pool; async client | Applications with many threads per client |
| WASM build over `MemVfs` | A browser or edge use case (ADR-0006) |
| Bindings for other languages (C ABI, Python) | User demand; needs an ADR (would bring `unsafe` to a new crate) |
| Online backup to a remote destination | Users need off-machine backups beyond `checkpoint` |
