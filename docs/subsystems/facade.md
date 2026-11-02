# Subsystem: `softshelldb` (the facade)

The crate applications depend on. It owns the database lifecycle, the
catalog, the glue between models, maintenance and stats, and re-exports
what callers need (ADR-0012). The server and the CLI use only its public
API (ADR-0003).

## Lifecycle

| Call | Does |
| :- | :- |
| `Database::open(dir, Options)` | Locks the directory, recovers storage, loads the catalog, resumes interrupted index builds, starts background threads |
| `Database::open_in_memory(Options)` | The same over a `MemVfs`; nothing touches the disk |
| `db.close()` | Stops maintenance, waits for running flushes and compactions, syncs the WAL, releases the lock. `Drop` does the same, logging any error |
| `db.checkpoint(dest)` | A consistent copy that opens as a database (`storage` checkpoint) |
| `db.check()` | Storage checks plus each model's consistency check; returns a report |
| `db.stats()` | Storage stats, per-collection counts, maintenance backlog, oldest snapshot age |

`Database` is `Send + Sync`; share it with `Arc`.

## Transactions

- `db.read(|rx| …)` and `db.write(|tx| …)` run a closure in a transaction;
  `write` commits on `Ok` and rolls back on `Err` or panic.
- `db.begin_read()` / `db.begin_write()` return explicit handles.
- Inside either, model handles are looked up by name:
  `tx.keyspace("kv")?`, `tx.collection("notes")?`, `tx.graph("links")?`,
  `tx.series("activity")?`, `rx.text_index("notes", "body_text")?`,
  `rx.vector_index("notes", "body_vec")?`. An unknown name is
  `Error::NotFound { kind, name }`.

## The catalog (keyspace 0)

| Entry key (`keycode`) | Value (SDOC, with a `version` field) |
| :- | :- |
| `("meta")` | Catalog version, next keyspace id, dropped keyspaces |
| `("keyspace", name)` | Keyspace id |
| `("collection", name)` | Keyspace id, `_id` policy, options |
| `("index", collection, name)` | Index id, spec, state (`building`, `ready`) |
| `("text_index", collection, name)` | Keyspace id, fields, analyzer version and options, state |
| `("vector_index", collection, name)` | Keyspace id, field, dims, metric, kind, seed, state |
| `("graph", name)` | Keyspace id |
| `("series_set", name)` | Keyspace id, bucket width, retention, rollup rules |

- Every definition change (create, drop, rename) happens inside a write
  transaction, so it is atomic with any data written alongside it.
- Names: 1–128 bytes of UTF-8, no control characters (`docs/03-platforms.md`).
- Keyspace ids are never reused. A drop records the id as dropped; the
  compaction filter and a maintenance sweep remove its keys.
- The catalog is cached in memory and replaced on each committed change.

## Cross-model glue

- **Text and vector indexes on collections.** On every insert, replace,
  update and delete in a collection, the facade, in the same transaction:
  1. computes the old and new values of each indexed field;
  2. calls `text::Index::update(doc_key, old_text, new_text)` and
     `vector::Index::upsert/delete(doc_key, vector)`.
  A document whose indexed field has the wrong type (a number in a text
  field, a vector of the wrong length) fails the write, naming the index.
- **Index builds** for text and vector indexes created on non-empty
  collections run like secondary-index builds: `building` state, batched
  transactions, resumed after a crash.
- **Hybrid search** (M10.5): run a text query and a vector query on the
  same collection, fuse with reciprocal rank fusion (k = 60), apply an
  optional document filter, return the top k with both component ranks.

## Maintenance thread

One thread runs jobs in short write transactions, each bounded by a time
budget (default 50 ms), yielding the writer lock between batches:

| Job | Trigger |
| :- | :- |
| Resume or continue index builds | An index in `building` state |
| Series chunking | Closed buckets with raw points |
| Series retention and rollups | Expired buckets; closed rollup windows |
| Vector rebuild | Deleted share above 20% |
| Dropped-keyspace sweep | Dropped keyspaces with keys left |

`Options::maintenance(Manual)` disables the thread; `db.run_maintenance()`
then runs pending jobs on the caller's thread (tests, tools).

## Errors

`softshelldb::Error` is one enum that wraps each crate's error and adds
`NotFound`, `AlreadyExists`, `Locked`, `Background`, `InvalidName`. Every
variant implements `Display` with the operation and the object it was
about, and `source()` for the cause.

## Re-exports and prelude

`softshelldb::prelude` brings in `Database`, `Options`, `Value`, `value!`,
`query!`, `record!`, `Id`, `Timestamp`, the spec builders, `Order`,
`Direction`, `Metric`, `Sync` and the result types.

## Tests

- **Cross-model crash test:** transactions that insert a document, update
  its text and vector indexes, add an edge and write a point, crashed at
  every sync point; after recovery all five are present or none are.
- **Catalog model test:** random creates, drops and renames against a map,
  with reopens.
- **Glue consistency:** after random document operations, `db.check()`
  finds every text and vector index consistent with its collection.
