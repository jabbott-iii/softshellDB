---
paths:
  - "crates/storage/**"
  - "crates/txn/**"
  - "crates/platform/**"
---

# Rules for the storage engine and transactions

A bug here loses data silently. `AGENTS.md`, rule 5, states the durability
rules every change keeps. These rules add the engine's details.

- **The ordering rule.** For every file the engine creates:
  1. write it under `tmp/` (or append to the WAL or manifest);
  2. `sync_data` it;
  3. rename it into place (`rename_durable` for `CURRENT` and `IDENTITY`);
  4. `sync_dir` the directory;
  5. only then reference it from a synced manifest record.
  A file is deleted only after a synced manifest record stops referencing it
  and no live `Version` holds it.
- **Never acknowledge early.** Under `Sync::Always`, a commit is not visible
  to read transactions and `commit()` does not return until its WAL record
  is synced (ADR-0011). Write transactions read the last appended commit,
  synced or not (ADR-0010).
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
- **Tests a change must keep or extend,** besides the crash-point test:
  the model test, the corruption tests, and, in `txn`, the isolation
  history test.
