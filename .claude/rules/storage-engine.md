---
paths:
  - "crates/storage/**"
  - "crates/txn/**"
  - "crates/platform/**"
---

# Rules for the storage engine and transactions

A bug here loses data silently. Read `docs/subsystems/storage-engine.md`
and `docs/subsystems/transactions.md` before changing anything.

- **The ordering rule.** For every file the engine creates:
  1. write it under `tmp/` (or append to the WAL or manifest);
  2. `sync_data` it;
  3. rename it into place (`rename_durable` for `CURRENT` and `IDENTITY`);
  4. `sync_dir` the directory;
  5. only then reference it from a synced manifest record.
  A file is deleted only after a synced manifest record stops referencing it
  and no live `Version` holds it.
- **Never overwrite.** Files are append-only. The WAL and manifest are
  appended to; everything else is written once.
- **Never acknowledge early.** Under `Sync::Always`, a commit is not visible
  to readers and `commit()` does not return until its WAL record is synced
  (ADR-0011).
- **Sync failures are fatal.** A failed `sync_data` or WAL/manifest write
  puts the database in the background-error state. Never retry a sync and
  treat the retry's success as durability.
- **Every file operation goes through `Vfs`.** No `std::fs` calls in
  `storage` or `txn`; otherwise `FaultVfs` cannot crash them.
- **Recovery is strict.** Only a torn tail of the last WAL segment (or of
  the manifest) may be truncated. Corruption anywhere else fails the open
  with the file and offset. Do not add a "skip bad records" mode without an
  ADR.
- **Snapshots are sacred.** Compaction and compaction filters never drop a
  version a live snapshot can see.
- **Locks:** follow the lock order in `docs/01-architecture.md`. Do not
  hold a lock across file I/O unless the spec already does (the WAL append
  under the commit queue). Never hold the writer lock while waiting for a
  sync.
- **No `unsafe`.** Not even for the memtable or the cache (ADR-0005,
  ADR-0009).
- **Tests a change must keep or extend:** the model test, the crash-point
  test (`cargo xtask crash --exhaustive` for changes to the write, flush,
  compaction or recovery paths), the corruption tests, and, in `txn`, the
  isolation history test. A new file operation needs a crash point covering
  it. Never weaken an invariant check to make a test pass.
- **In the final message,** answer the "Review checklist for storage
  changes" in `docs/04-quality.md`.
