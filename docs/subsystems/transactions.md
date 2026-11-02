# Subsystem: `softshell-txn`

Keyspaces and transactions over `softshell-storage` (ADR-0010, ADR-0011,
ADR-0012). Every data model reads and writes through this crate and nothing
else.

## Contents

| Module | Provides |
| :- | :- |
| `db` | `TxnDb`: owns a `Storage`, the writer lock, the snapshot registry; `open`, `open_in_memory` |
| `keyspace` | `KeyspaceId(u32)`, `Keyspace` handles; the prefix rules |
| `read` | `ReadTxn`: snapshot reads |
| `write` | `WriteTxn`: buffered writes with read-your-writes, commit and rollback |
| `filter` | Registry mapping keyspace prefixes to compaction filters |
| `error` | `TxnError` |

## Keyspaces

- A keyspace is a 4-byte big-endian prefix. `Keyspace` handles add and
  strip the prefix, so a model never sees another model's keys and never
  builds a key outside its own range.
- `TxnDb` does not allocate keyspace ids; the facade's catalog does
  (keyspace 0). `TxnDb` only refuses access to a keyspace marked dropped.
- Range and prefix scans are clamped to the keyspace: a scan of keyspace
  `k` never yields a key of `k + 1`.

## Read transactions

```rust
let rx = db.begin_read();                    // snapshot = visible sequence now
let v: Option<Bytes> = rx.get(ks, key)?;
for entry in rx.range(ks, start..end, Direction::Forward)? { /* … */ }
for entry in rx.prefix(ks, prefix)? { /* … */ }
```

- A `ReadTxn` sees exactly one committed state: the one at its snapshot.
- Any number of read transactions run at once, from any thread. They never
  take the writer lock.
- A read transaction registers its snapshot; compaction keeps the versions
  it can see until it is dropped. Long-lived read transactions therefore
  hold space; `stats()` reports the oldest snapshot's age.

## Write transactions

```rust
let mut tx = db.begin_write()?;              // blocks until the writer lock is free
let old = tx.get(ks, key)?;                  // last appended commit + own writes
tx.put(ks, key, value)?;
tx.delete(ks, other_key)?;
tx.commit_with(Sync::Always)?;               // or tx.commit(); dropping it rolls back
```

- One write transaction at a time. `begin_write` waits for the lock;
  `try_begin_write` and `begin_write_timeout(d)` do not wait forever.
- Reads inside the transaction see the last appended commit plus the
  transaction's own buffered writes, merged in one iterator, so range
  scans reflect pending puts and deletes.
- Under commit pipelining, the last appended commit may still be waiting
  for its sync. Reading the visible (synced) state instead would let this
  transaction overwrite changes it never saw. A crash can therefore erase
  a commit this transaction read, and then this transaction too, because
  the WAL is synced in order. Callers must not act on values read inside a
  write transaction until its commit returns `Ok` (ADR-0010).
- Writes are buffered in a `WriteBatch`. A transaction whose buffer passes
  `max_txn_bytes` (default 64 MiB) fails the write with
  `TxnError::TooLarge`; the transaction is then rolled back.
- `commit` submits the batch to `Storage` with the sync mode, releases the
  writer lock after the WAL append (commit pipelining), then waits for
  durability and visibility per ADR-0011 before returning.
- Rollback discards the buffer. Nothing reached the WAL, so there is
  nothing to undo.
- A panic inside a `db.write(|tx| …)` closure in the facade rolls the
  transaction back and releases the lock (the guard's `Drop`).

## Isolation guarantees

| Transaction | Guarantee |
| :- | :- |
| Write | Serializable: write transactions run one after another in commit order |
| Read | Consistent snapshot: one committed state, as of its start |
| Together | The history is serializable: each read transaction can be placed after the last commit visible at its start |

There are no conflicts and no retries (ADR-0010).

## Compaction filters

`TxnDb::register_filter(prefix, filter)` installs a `CompactionFilter`
for a keyspace or a prefix within one. The facade registers:

- dropped keyspaces (drop every key);
- series retention (drop raw points and chunks older than the cutoff);
- document TTL (backlog).

Filters only drop versions older than every live snapshot, so no reader
sees a key vanish (`docs/subsystems/storage-engine.md`).

## Errors

`TxnError` wraps `StorageError` and adds `TooLarge`, `KeyspaceDropped`,
`LockTimeout`, `ReadOnly` (the database is in a background-error state)
and `Closed`.

## Tests

- **Isolation history test** (`tests/history.rs`): one writer and eight
  readers for a few thousand random transactions; the checker replays the
  commit log and proves each reader's observations match the state at its
  snapshot (`docs/04-quality.md`).
- **Read-your-writes model test:** random gets and scans inside a write
  transaction against `BTreeMap` overlays.
- **Pipelining test:** while one commit waits for its sync (a `FaultVfs`
  that delays syncs), the next writer starts, and its commit becomes
  visible only after the first.
- **Crash test:** the storage crash-point test, run through `txn`, with
  multi-keyspace transactions: after recovery a transaction is all there
  or not at all, in every keyspace.
