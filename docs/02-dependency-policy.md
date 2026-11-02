# 02 — Dependency policy

## The rule

> **Build it ourselves unless building it is unreasonable.** A third-party
> crate is used only when it passes the exception test below, the user has
> approved it, and it is recorded in the register in this document
> (ADR-0002).

The current approved set of third-party crates is **empty**. `Cargo.lock`
must contain only workspace packages. `cargo xtask deps` fails otherwise.

## What counts as a dependency

| Counts (needs approval) | Does not count |
| :- | :- |
| Any crate from crates.io or git in any `[dependencies]`, `[dev-dependencies]` or `[build-dependencies]` table | The Rust toolchain, `std`, `core`, `alloc` and `proc_macro` |
| Vendored or copied third-party source code, including "just one file" | OS system libraries we call through FFI (listed below) |
| A build script that downloads anything or runs third-party code | OS tools invoked by CI or `xtask` packaging (`sha256sum`, `tar`, `codesign`, `signtool`) |
| A git submodule containing code | Public specifications we implement ourselves (RFC 8259 JSON, FIPS 180-4 SHA-256, the Snowball Porter2 algorithm, the HNSW paper) |
| A `cargo install`-ed tool that CI depends on | GitHub Actions used by CI, pinned by commit SHA (`docs/04-quality.md`) |
| Test data copied from a third party | Test data we generate ourselves |

Test vectors published with a specification (NIST SHA-256 vectors, the
Snowball stemmer's vocabulary lists) are not code, but they keep their own
license and notice beside them in the test directory (ADR-0004).

### OS libraries we rely on (not dependencies, but listed for audit)

| OS | Libraries | How |
| :- | :- | :- |
| Linux | glibc | Through `std` only. `/dev/urandom` read through `std::fs` |
| macOS | libSystem | Through `std` only. `/dev/urandom` read through `std::fs` |
| Windows | `kernel32` (`MoveFileExW`), `bcryptprimitives` (`ProcessPrng`) | Declared in `softshell-sys`, linked with `#[link]` |

Anything added to this table needs the `/add-os-binding` procedure and a
line here.

## The exception test

Before proposing any crate, first try to **change the design so the need
disappears**:

- choose a simpler format (an LZ77 block format instead of zstd)
- cut scope (English stemming only, tokens split on Unicode alphanumerics)
- avoid the need (API tokens of 256 random bits need no password KDF)
- use what `std` already has (`File::lock`, `sync_data`, `read_at`)

If the need remains, a crate may be proposed only when at least one of these
is true:

1. **Disproportionate cost.** Building it is estimated above 4 engineer-weeks,
   the capability is not central to the database, and no design change
   avoids it.
2. **Specialist correctness with severe failure modes.** The domain demands
   expert knowledge, bugs are hard to detect by testing, and the consequences
   are severe. Examples: TLS, authenticated encryption, password hashing.
3. **Dev-only tooling with no reasonable equivalent.** It never ships in a
   released binary or library, and our own version would be far weaker.
   Example: coverage-guided fuzzing.

"It's popular" and "it would be faster to start" are not reasons.

## Exception process

1. Run `/evaluate-dependency` (or follow
   `.claude/skills/evaluate-dependency/` by hand) and write the evaluation
   to `docs/dependency-evaluations/<crate>.md`. It must cover:
   - the need, and the design changes considered
   - an estimate for building it ourselves
   - the candidate's license, maintainers and release cadence
   - its transitive dependency count and `unsafe` usage, including C or
     assembly it builds
   - its binary-size impact, and whether it ships in `softshelldb`
2. The **user** approves or rejects it. Agents never approve dependencies.
3. If approved:
   - Add a row to the register below with status `Approved` and an ADR
     reference.
   - Add the exact pin to `xtask/deps-allowlist.txt` (`name version ADR-NNNN`),
     one line per package the crate brings in, transitive ones included.
   - Use an exact version requirement (`= x.y.z`).
   - Wrap the crate behind a single module or crate boundary of ours, so it
     can be replaced without touching callers.
   - Put it behind a cargo feature if it is needed only by the server or
     the CLI, so the embedded library does not carry it.
4. Review approved dependencies on every upgrade. Upgrades are their own
   change, never bundled with feature work.

## Build-vs-buy register

**Status** is one of:

- **Build:** we write it.
- **Open:** no decision yet; a roadmap task schedules the evaluation.
- **Proposed:** an evaluation exists and awaits the user's decision.
- **Approved:** the user said yes, ADR recorded.
- **Rejected.**

The **fallback** column is a crate we would evaluate first if the **switch
trigger** fires. It is not approved.

| # | Capability | Status | Our approach | Fallback (not approved) | Switch trigger |
| :- | :- | :- | :- | :- | :- |
| 1 | File I/O, sync, locks, rename | Build | `std::fs` (`read_at`/`seek_read`, `sync_data`, `File::lock`) plus a few hand-written Windows declarations in `softshell-sys` | `libc`, `windows-sys` | More than about 100 declarations, or 2 binding bugs reach `main` in one milestone |
| 2 | OS randomness | Build | `/dev/urandom` through `std::fs` on Unix; `ProcessPrng` on Windows | `getrandom` | `std::random` stabilizes on the pinned toolchain: switch to it and drop ours (ADR) |
| 3 | Networking and concurrency for the server | Build | `std::net`, a bounded thread pool, one thread per connection (ADR-0018) | `tokio` | A deployment needs more than about 2,000 concurrent connections, or per-connection thread memory exceeds budget |
| 4 | Serialization of Rust types | Build | `Value`, `ToValue`/`FromValue`, the `record!` macro (ADR-0013) | `serde` | Macro limits block more than 3 needed type shapes |
| 5 | JSON / JSONC | Build | Own parser and writer in `softshell-codec::json` | `serde_json` | Not expected |
| 6 | Binary document format | Build | SDOC (ADR-0013) | `bson` | Not expected; SDOC is not BSON-compatible by design |
| 7 | Order-preserving key encoding | Build | `keycode` in `softshell-codec` | `storekey` | Not expected |
| 8 | Block compression | Build | SLZ, our own LZ77 block format | `lz4_flex` | SLZ decode below 500 MB/s, or below 2× on the text benchmark, after optimization |
| 9 | CRC32C checksums | Build | Table-driven, slicing-by-8, in `softshell-core` | `crc32c` (hardware) | Checksumming above 5% of read-path CPU in a profile; first consider hardware CRC in `softshell-platform` (ADR) |
| 10 | Non-cryptographic hashing | Build | FNV-1a 64 (stable on disk), own fast hasher for in-memory maps | `ahash`, `xxhash-rust` | Not expected |
| 11 | SHA-256 | Build | FIPS 180-4 in `softshell-core`, NIST CAVP vectors; used only to hash high-entropy API tokens (ADR-0019) | `sha2` | Any second use of SHA-256, or any need for another cryptographic primitive |
| 12 | Password hashing / KDF | Build (not needed) | Avoided: only random API tokens, no passwords (ADR-0019) | `argon2` | A requirement for user passwords: criterion 2 |
| 13 | TLS for the server | **Open** | Evaluation in M11.6. Until approved, the server binds loopback only (ADR-0018) | `rustls` with a crypto provider | M11.6 |
| 14 | Encryption at rest (AEAD) | **Open** | Not scheduled; backlog | `chacha20poly1305`, `aes-gcm`, `aws-lc-rs` | A user requirement for encrypted database files: criterion 2 |
| 15 | Memtable structure | Build | Persistent copy-on-write B-tree in safe Rust (ADR-0009) | `crossbeam-skiplist` | Memtable inserts above 30% of commit CPU in a profile |
| 16 | Block cache | Build | Sharded LRU with byte accounting | `moka` | Not expected |
| 17 | Bloom filters | Build | Own, 10 bits per key, double hashing | — | — |
| 18 | Thread pool and channels | Build | `std::thread`, `std::sync::mpsc`, `Mutex`/`Condvar` | `crossbeam`, `rayon` | Not expected |
| 19 | Regular expressions (`$regex`) | Build | Backlog: a small backtracking-free engine (Thompson NFA) | `regex` | A user need before the backlog item lands |
| 20 | Unicode segmentation and normalization | Build | `char::is_alphanumeric` and `char::to_lowercase` from `std`; tables generated by `xtask` later | `unicode-segmentation`, `unicode-normalization` | Indexing a language without spaces between words |
| 21 | Stemming | Build | Porter2 (English) per the Snowball description | `rust-stemmers` | More than two languages needed |
| 22 | Approximate nearest neighbors | Build | HNSW over the key-value store, flat index as ground truth (ADR-0017) | `hnsw_rs` | Recall or latency budget missed after tuning |
| 23 | SIMD distance kernels | Build | Scalar loops written for auto-vectorization | `simsimd` | Distance computation above 50% of vector query time in a profile; first consider `std::arch` kernels in a crate an ADR allows `unsafe` in |
| 24 | Time-series compression | Build | Delta-of-delta timestamps, XOR floats (Gorilla-style) (ADR-0015) | `tsz` | Not expected |
| 25 | Logging | Build | `softshell_core::log` | `log`, `tracing` | An embedding application needs to route our logs into its own logger: add an adapter trait first |
| 26 | CLI argument parsing | Build | `softshell_core::cli` | `clap` | Not expected |
| 27 | REPL line editing | Build | Plain line reads from stdin | `rustyline` | Repeated user requests for history and editing |
| 28 | Error plumbing | Build | Per-crate error enums | `thiserror`, `anyhow` | Not expected |
| 29 | Property testing | Build | Seeded harness in `softshell_core::testing` | `proptest` | Not expected |
| 30 | Benchmarking | Build | `softshell-bench`: warm-up, N samples, median/p95/p99, JSON baselines | `criterion` | Not expected |
| 31 | Fuzzing (stable, CI) | Build | Mutation smoke fuzzer in `xtask` | — | — |
| 32 | Coverage-guided fuzzing (optional, nightly) | **Open** | Decide in M12.2 (criterion 3) | `cargo fuzz` with `libfuzzer-sys`, in an excluded `fuzz/` workspace, never linked into a release | M12.2 |
| 33 | Timestamps and RFC 3339 | Build | Civil-date conversion in `softshell-core::civil` | `chrono`, `time` | Time-zone database support becomes a requirement |
| 34 | Random numbers | Build | PCG32, seedable, deterministic | `rand` | Not expected |
| 35 | Unique ids | Build | Time-ordered 128-bit `Id` (48-bit milliseconds + 80 random bits) | `uuid`, `ulid` | Not expected |

## Enforcement

- **`cargo xtask deps`** reads `cargo metadata --format-version 1` (parsed
  with our own JSON parser). It fails if any package has a non-null
  `source` and is not in `xtask/deps-allowlist.txt` at that exact version.
- **`cargo xtask layers`** checks the workspace dependency graph against the
  matrix in `docs/01-architecture.md`.
- **Path-scoped rule** `.claude/rules/dependencies.md` loads whenever an
  agent opens a Cargo manifest.
- **Claude Code permissions** ask before `cargo add`, `cargo update` and
  `cargo install`.
