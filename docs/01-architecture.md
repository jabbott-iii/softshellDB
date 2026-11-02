# 01 — Architecture

## Principles

1. **Layered crates, one-way dependencies.** Lower layers never know about
   higher ones. `cargo xtask layers` enforces the matrix below (ADR-0005).
2. **One engine, many models.** There is one LSM tree, one WAL and one
   transaction manager. Every data model is a library that maps its records
   onto keys in its own keyspace (ADR-0012).
3. **Data models are independent libraries.**
   - `documents`, `graph`, `series`, `text` and `vector` each depend on
     `txn` and know nothing about each other.
   - The `softshelldb` facade owns the catalog and the glue between models:
     a text or vector index on a document field, hybrid search, and the
     maintenance jobs.
   - This keeps each model testable in isolation against an in-memory
     engine.
4. **The OS is quarantined.** OS declarations live in `softshell-sys`. OS
   behavior (files, durability, locks, randomness) lives in
   `softshell-platform`. Nothing else touches an OS API, contains `unsafe`,
   or has a `target_os` cfg.
5. **Durability is a contract, not a hope.** A commit is acknowledged as
   durable only after its WAL record is synced, and only then becomes
   visible to readers (ADR-0011). Every file the engine writes is
   checksummed, written once, synced, and then installed by a synced
   manifest record.
6. **Bounded everything.** Every cache, buffer, request, result batch and
   traversal has a configured limit. Exceeding one is an error, never an
   unbounded allocation.
7. **Plain threads, no async runtime.** `std::thread`, `Mutex`, `Condvar`,
   `mpsc` and atomics. The server uses a bounded thread pool (ADR-0018).

## Layers

A crate may only depend on crates in lower layers, and only on the specific
crates its row in the dependency matrix allows.

| Layer | Crates |
| :- | :- |
| 7 — applications and tooling | `cli`, `apps/tidepool`, `xtask`, `bench` |
| 6 — network | `server`, `client` |
| 5 — facade | `softshelldb` (Database, catalog, cross-model glue, maintenance) |
| 4 — data models | `documents`, `graph`, `series`, `text`, `vector` |
| 3 — transactions | `txn` |
| 2 — engine, expressions, protocol | `storage`, `expr`, `proto` |
| 1 — OS behavior and formats | `platform`, `codec` |
| 0 — foundations | `core`, `sys` |

## Dependency matrix

A crate may depend on the workspace crates listed in its row and nothing
else. No crate may depend on a third-party crate unless the dependency
policy approves it. The matrix counts normal, dev and build dependencies.

| Crate | Allowed workspace dependencies |
| :- | :- |
| `softshell-core` | — |
| `softshell-sys` | — |
| `softshell-platform` | `core`, `sys` |
| `softshell-codec` | `core` |
| `softshell-storage` | `core`, `platform`, `codec` |
| `softshell-expr` | `core`, `codec` |
| `softshell-proto` | `core`, `codec` |
| `softshell-txn` | `core`, `codec`, `storage` |
| `softshell-documents` | `core`, `codec`, `expr`, `txn` |
| `softshell-graph` | `core`, `codec`, `expr`, `txn` |
| `softshell-series` | `core`, `codec`, `expr`, `txn` |
| `softshell-text` | `core`, `codec`, `txn` |
| `softshell-vector` | `core`, `codec`, `txn` |
| `softshelldb` (facade) | `core`, `platform`, `codec`, `storage`, `expr`, `txn`, `documents`, `graph`, `series`, `text`, `vector` |
| `softshell-server` | `core`, `codec`, `proto`, `softshelldb` |
| `softshell-client` | `core`, `codec`, `proto` |
| `softshell-cli` | `core`, `codec`, `softshelldb`, `client` |
| `tidepool` | `softshelldb` |
| `xtask` | `core`, `codec` |
| `softshell-bench` | any workspace crate |

Models test against a real engine without new edges: `txn` exposes
`TxnDb::open_in_memory()`, which builds `storage` over a `MemVfs`.

## Workspace layout

```
.
├── Cargo.toml                # [workspace], [workspace.package], [workspace.lints], profiles
├── rust-toolchain.toml       # pinned stable toolchain + rustfmt, clippy
├── clippy.toml               # allow-unwrap-in-tests = true, allow-expect-in-tests = true
├── .cargo/config.toml        # alias: xtask
├── crates/
│   ├── core/ sys/ platform/ codec/
│   ├── storage/ expr/ proto/ txn/
│   ├── documents/ graph/ series/ text/ vector/
│   ├── softshelldb/          # the facade
│   └── server/ client/ cli/
├── apps/tidepool/            # the reference application
├── xtask/
├── bench/
├── tests/fixtures/           # committed files of every released on-disk format version
├── fuzz/corpus/              # seed inputs per parser
└── docs/
```

`.cargo/config.toml`:

```toml
[alias]
xtask = "run --profile xtask --package xtask --"
```

Workspace lints (`Cargo.toml`); every member uses `[lints] workspace = true`:

```toml
[workspace.lints.rust]
unsafe_code = "deny"              # sys and platform override with #![allow(unsafe_code)]
unsafe_op_in_unsafe_fn = "deny"
missing_docs = "warn"
unreachable_pub = "warn"

[workspace.lints.clippy]
undocumented_unsafe_blocks = "deny"
multiple_unsafe_ops_per_block = "deny"
missing_safety_doc = "deny"
unwrap_used = "deny"
print_stdout = "deny"
print_stderr = "deny"
dbg_macro = "deny"
todo = "deny"
```

`unsafe_code` is `deny` rather than `forbid` on purpose: `forbid` cannot be
overridden, and the two OS crates need `#![allow(unsafe_code)]` at their
root. `cargo xtask unsafe-audit` checks that the attribute appears nowhere
else. `softshell-cli`, `xtask` and `softshell-bench` allow `print_stdout`
and `print_stderr` at their crate root, with a comment, because printing is
their job.

Modules that decode bytes from disk or the network (every decoder in
`codec`, the SST, WAL and manifest readers in `storage`, and `proto`) add
`#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]` at
module level, so an out-of-bounds index or an overflowing length is a
compile error there rather than a panic on hostile input. These lints are
not workspace-wide: CI runs clippy with `-D warnings`, and ordinary code
would drown in them.

## How the models share one tree

Every key in the LSM tree starts with a 4-byte big-endian **keyspace id**.
The rest of the key belongs to the model that owns the keyspace.

| Keyspace | Owner | Holds |
| :- | :- | :- |
| `0` | facade catalog | Definitions of every collection, index, graph and series set, and the next keyspace id |
| `1` | facade | Auth tokens (hashed) and server settings |
| `2..` | one model each | Allocated by the catalog; never reused after a drop |

A model's own key layout is in its subsystem spec. For example, a document
collection uses `ks ++ 0x01 ++ keycode(_id)` for documents and
`ks ++ 0x02 ++ index_id ++ keycode(fields…) ++ keycode(_id)` for index
entries. Keys within a model are built with `keycode`, the order-preserving
encoding in `softshell-codec`, so range scans follow value order.

Dropping a collection is one catalog write: its keyspace id goes on the
dropped list, reads stop at once, and compaction removes its keys later
(ADR-0012).

## The write path

```
caller ─► db.write(|tx| …) ─► takes the writer lock (one write txn at a time)
              │ reads: snapshot + own buffered writes (read-your-writes)
              │ writes: buffered in the txn's WriteBatch; models add their index entries
              ▼
          commit: assign sequence numbers ─► append the batch to the WAL ─► apply to memtable
              │ release the writer lock (the next writer may start)
              ▼
          durability wait (per Sync policy): group commit syncs the WAL for every waiting commit
              ▼
          publish visible sequence ─► readers see it ─► return Ok to caller
```

- The writer lock is released before the sync, so the next transaction can
  run while the previous one waits for its sync (commit pipelining). The
  visible sequence advances in commit order, only past synced commits.
- Background threads flush full memtables to level-0 SST files and compact
  levels. Each finished file is synced, then installed by a synced manifest
  record.

## The read path

```
db.read(|rx| …) ─► snapshot = visible sequence
   get(key)  ─► active memtable ─► immutable memtables ─► L0 files (newest first) ─► L1..Ln
                  (bloom filter per file, index block, data block via block cache)
   scan(range) ─► merging iterator over the same sources, newest version ≤ snapshot wins
```

Readers never take the writer lock and never block a writer. A snapshot
pins the versions it can see until it is dropped; compaction keeps them.

## Threading model

| Thread | Owns | Talks to others via |
| :- | :- | :- |
| Caller threads | Read transactions (any number), at most one write transaction at a time | The writer lock; the commit queue |
| Group-commit leader (a waiting caller, not a dedicated thread) | The WAL sync for every waiting commit | `Mutex` + `Condvar` |
| WAL syncer (only with `Sync::Interval`) | Periodic WAL sync | `Condvar` with timeout |
| Flush (one) | Turning immutable memtables into L0 files | Version-set mutex; job channel |
| Compaction (N = clamp(cores / 2, 1, 4)) | Merging files between levels | Version-set mutex; job channel |
| Maintenance (one, facade) | Series chunking, retention, rollups, vector rebuilds, dropped-keyspace cleanup | Runs short write transactions in batches |
| Server acceptor + connection pool (server only) | Sockets and sessions, bounded by `max_connections` | Calls the facade like any caller |

- Lock order, outermost first: writer lock → commit queue → version set →
  memtable publish → block cache shard. No lock is held across a file
  read or write except the WAL append under the commit queue.
- The engine never spawns a thread per request and never blocks a reader
  on I/O done for another reader.

## Public API sketch (embedded)

```rust
use softshelldb::prelude::*;

fn main() -> Result<(), softshelldb::Error> {
    let db = Database::open("./data", Options::default())?;

    db.write(|tx| {
        tx.create_collection("notes", CollectionOptions::default())?;
        tx.create_index("notes", IndexSpec::new("by_tag").field("tags", Order::Asc))?;
        tx.create_text_index("notes", TextIndexSpec::new("body_text").field("body"))?;
        tx.create_vector_index("notes", VectorIndexSpec::new("body_vec", "embedding", 384, Metric::Cosine))?;
        tx.create_graph("links")?;
        tx.create_series_set("activity", SeriesOptions::default().retention(days(90)))?;
        Ok(())
    })?;

    db.write(|tx| {
        let id = tx.collection("notes")?.insert(value!({
            "title": "Tide tables",
            "tags": ["ocean", "notes"],
            "body": "Low tide is the best time to explore tide pools.",
            "embedding": embedding_from_somewhere(),
        }))?;
        tx.graph("links")?.add_edge(&id, &other_id, "links_to", value!({}))?;
        tx.series("activity")?.write("edits", &[("note", id.to_string())], now(), value!({ "count": 1 }))?;
        Ok(())
    })?;

    let hits = db.read(|rx| {
        rx.collection("notes")?
            .find(query!({ "tags": { "$in": ["ocean"] } }))?
            .sort("title", Order::Asc)
            .limit(20)
            .collect_values()
    })?;
    Ok(())
}
```

- `db.write` commits when the closure returns `Ok` and rolls back on `Err`.
  `db.begin_write()` and `db.begin_read()` give explicit handles.
- `value!` and `query!` are `macro_rules!` macros that build `Value`s with
  JSON-like syntax. `ToValue` and `FromValue` convert Rust types; the
  `record!` macro implements them for plain structs (no proc macros, no
  serde; ADR-0013).
- The pure key-value API is `tx.keyspace("name")?` with `get`, `put`,
  `delete`, `range` and `prefix`.
- `Database::open_in_memory()` runs the same engine over a `MemVfs`.

## Feature flags

| Feature | Crates | Enables |
| :- | :- | :- |
| `dev` | `core`, `storage`, `softshelldb` | Extra invariant checks after every flush and compaction, allocation counters, verbose engine events |
| `fault-injection` | `platform` | Exposes `FaultVfs` outside tests (for `xtask crash` and fuzzing) |
| (none) | — | Release behavior |

A member turns a feature on only from its `[features]` table or a
dev-dependency, never from a normal dependency, so a release build of
`softshelldb` never includes `dev` or `fault-injection`.
`cargo xtask release-no-dev` checks this.

## The database directory

```
<db>/
├── LOCK                 # held with an exclusive OS file lock while open
├── IDENTITY             # database id (128-bit), written once
├── CURRENT              # the name of the live manifest, replaced by durable rename
├── MANIFEST-000005      # log of version edits (files per level, sequence, file numbers)
├── 000009.wal           # write-ahead log segments, numbered
├── 000012.sst           # sorted string tables, immutable once installed
└── tmp/                 # files being written; removed on open
```

- Only one process may open a database directory at a time; a second open
  fails with `Error::Locked`.
- On open, files in `tmp/` and SST files the manifest does not list are
  deleted (after a log line naming each). Nothing else is deleted without
  the manifest saying so.

## Configuration

- Embedded: `Options` (builder) — cache size, memtable size, sync policy,
  compaction threads, limits, clock, logger sink. Defaults are documented
  on each field and in `docs/subsystems/storage-engine.md`.
- Server: `softshell.jsonc`, parsed with our own JSONC parser. Unknown keys
  are an error (a typo must not silently disable a setting); every value is
  validated before the server binds.
- The options a database was created with that affect its files (the
  comparator, format versions) are stored in the manifest. Opening with an
  incompatible setting is an error, not a silent reinterpretation.
