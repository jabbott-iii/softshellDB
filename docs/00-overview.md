# 00 — Overview

## Purpose

softshellDB is an all-in-one NoSQL database written in Rust. One embedded
engine stores and queries six data models over a single transactional
key-value core:

| Model | What it gives the caller |
| :- | :- |
| Key-value | Ordered keyspaces: get, put, delete, range and prefix scans |
| Documents | Collections of JSON-like documents, secondary indexes, query documents, updates, aggregations |
| Graph | Property graphs: nodes, labeled directed edges, neighbor queries, traversals, shortest paths |
| Time-series | Series identified by measurement and tags, compressed chunks, window aggregations, retention, rollups |
| Full-text search | Analyzed text indexes on document fields, phrase and boolean queries, BM25 ranking |
| Vector search | Nearest-neighbor indexes on document fields (exact and HNSW), filtered and hybrid search |

All six share one storage engine, one write-ahead log and one transaction:
a single commit can insert a document, update its text and vector indexes,
add a graph edge and write a time-series point, atomically (ADR-0001).

It ships first as a Rust library (`softshelldb`) that applications embed,
like SQLite or RocksDB. A single-node network server and a command-line
tool follow, built on the same library (ADR-0003).

It is developed alongside one reference application, `apps/tidepool`: a
notes app with links between notes, search, "similar notes" and activity
charts. The rule is: **the workload pulls features in, the engine never
pushes them.** A data model is not done until Tidepool uses it.

## Goals

1. A correct, crash-safe embedded database: a commit that was acknowledged
   as durable survives a process crash or power loss, on Linux, macOS and
   Windows.
2. Six data models with one transaction across all of them, so an
   application needs one database instead of four.
3. Own the whole stack above the OS and the Rust standard library, so that
   any bug can be understood and fixed in-house (ADR-0002).
4. Keep the engine small enough for one or two engineers to hold in their
   heads. As an estimate, about 45–60k lines of Rust at M12.
5. Every byte read from disk or the network is treated as hostile: no
   panic, no unbounded allocation, no undefined behavior on any input.

## Non-goals

- Distributed operation: sharding, consensus, multi-region. Single-leader
  replication is in the backlog behind a trigger (ADR-0003).
- SQL, or relational joins beyond graph traversals.
- WASM, `no_std`, mobile, and 32-bit targets (ADR-0006).
- Network filesystems (NFS, SMB, CIFS) as the database directory: their
  locking and sync semantics break the durability contract (ADR-0006).
- Generating embeddings. Callers supply vectors; softshellDB stores and
  searches them.
- Geospatial indexes, a GUI, and a hosted service. Each would need an ADR.
- Scripting or stored procedures. Logic stays in the caller's Rust code.

## Performance budgets

These are targets, not measurements. No reference machine is named yet:
record one here before M4.6 measures the storage budgets on it, and
measure every row on that machine in the milestone named.

| Budget | Target | Measured in |
| :- | :- | :- |
| Point read, key in block cache | p50 ≤ 5 µs, ≥ 300k reads/s per thread | M4.6 |
| Random writes, 100-byte values, `Sync::Never` | ≥ 200k writes/s | M4.6 |
| Durable commits, `Sync::Always`, 16 committing threads | ≥ 5k commits/s on NVMe | M5.3 |
| Bulk load (sorted input) | ≥ 100 MB/s | M4.6 |
| Recovery after a crash with a 64 MiB WAL | open in ≤ 2 s | M4.6 |
| Space amplification, steady state | ≤ 1.3× live data | M4.6 |
| Write amplification | measured and reported; ≤ 15× target | M4.6 |
| Indexed equality query, 1 result, 1M documents, hot | p50 ≤ 50 µs | M6.5 |
| Graph 2-hop neighbors, average degree 20, hot | p50 ≤ 1 ms | M7.4 |
| Time-series ingest, batched | ≥ 1M points/s; ≤ 2 bytes/point for regular float series | M8.6 |
| Full-text BM25 top-10, 1M synthetic documents | p50 ≤ 20 ms | M9.5 |
| Vector HNSW, 100k × 128-dim, top-10 | recall@10 ≥ 0.95 at p50 ≤ 5 ms | M10.4 |
| Server overhead per request on loopback | ≤ 50 µs above the embedded call | M11.4 |
| Memory | bounded by configuration: block cache, memtables, request limits | every milestone |
| `softshell` CLI binary | ≤ 10 MB per platform | M12.5 |

## Assumptions

Revisit these if any turns out to be wrong.

- One or two engineers, with an AI coding agent doing much of the
  implementation under review.
- The data fits on one machine's local disk, and the hot working set fits
  in memory most of the time.
- Storage is an SSD. Spinning disks work, but the budgets ignore them.
- Documents are at most 16 MiB, keys at most 8 KiB.
- Single-writer transactions are fast enough for the target workloads,
  because commits are short and group commit batches the syncs (ADR-0010).
- All supported targets are 64-bit and little-endian. File formats still
  state their byte order explicitly.

## Scope estimate

Sizes are rough line-count estimates for planning, not commitments.
S ≈ <1k lines, M ≈ 1–3k, L ≈ 3–6k, XL ≈ 6k+.

| Area | Size | Why |
| :- | :- | :- |
| `core` | M | Errors, varints, CRC32C, hashes, SHA-256, PCG32, IDs, logging, CLI, test harness |
| `sys` + `platform` | M | `Vfs` trait, three OS implementations, memory and fault-injecting VFS, OS randomness |
| `codec` | L | `Value`, JSON/JSONC, SDOC, keycode, SLZ compression, record framing, series chunk codec |
| `storage` | XL | WAL, memtable, SST format, manifest, compaction, caches, iterators, backup, checks |
| `txn` | M | Keyspaces, snapshots, single-writer transactions, commit pipeline |
| `expr` | M | Paths, filter/projection/update AST, query-document parser, evaluator |
| `documents` | L | Collections, secondary indexes, planner, updates, aggregations |
| `graph` | M | Adjacency layout, traversals, shortest paths |
| `series` | M | Series registry, tag index, chunking, windows, retention, rollups |
| `text` | M | Analyzer, stemmer, postings, BM25, query evaluation |
| `vector` | M | Metrics, flat index, HNSW in the key-value store, deletes and rebuilds |
| `softshelldb` facade | M | Database API, catalog, cross-model index glue, maintenance, stats |
| `proto` + `server` + `client` | L | Framing, handshake, sessions, auth, handlers, cursors, client |
| `cli` | M | REPL, import/export, check, dump, serve, admin |
| `xtask` + `bench` + harnesses | L | Guardrails, fuzzer, crash harness, benchmark workloads |
| `apps/tidepool` | S | The reference application |

The biggest risk is not any one model. It is **silent data loss**: a crash
at the wrong moment, an OS that does not sync what we think it syncs, or a
compaction that drops a live version. The roadmap reflects this. The
storage engine and its crash tests come before any data model, and every
model is a library over the same transaction API.

## Top risks and mitigations

| Risk | Mitigation |
| :- | :- |
| Crash-consistency bugs lose acknowledged commits | Checksums on every record and block; a fault-injecting VFS that drops unsynced writes and tears the last write at every sync point; process-kill tests; `softshell check`; `.claude/rules/storage-engine.md` |
| OS durability semantics differ (macOS `fsync`, Windows rename, directory sync) | All OS file behavior lives in `softshell-platform`, specified per OS in `docs/03-platforms.md` and tested on each OS in CI |
| Six data models dilute the effort | Each model is a library over `txn` and knows nothing of the others; milestones finish one model at a time; Tidepool pulls each one in |
| Compaction stalls and write amplification | Budgets above, measured in M4.6; write stalls are explicit and logged; compaction statistics in `db.stats()` |
| Build-first cost for compression, hashing, crypto and TLS | Simple formats (own LZ block format, Gorilla-style series chunks); crypto and TLS go through the dependency exception test; the server stays loopback-only until TLS is decided (ADR-0018) |
| Hostile files and network input | No-panic rule on all input, `Limits` on every allocation, every parser fuzzed (`docs/04-quality.md`) |
| On-disk formats change before 1.0 and strand data | Every file type carries a magic and a version; fixtures of every released version are committed and read in tests (ADR-0021) |
| The single writer becomes the bottleneck | Group commit and commit pipelining; writer-lock wait is measured; multi-writer optimistic transactions are in the backlog with a trigger (ADR-0010) |
