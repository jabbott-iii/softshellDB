---
paths:
  - "crates/expr/**"
  - "crates/documents/**"
  - "crates/graph/**"
  - "crates/series/**"
  - "crates/text/**"
  - "crates/vector/**"
  - "crates/softshelldb/**"
---

# Rules for the data models and the facade

- **Models are independent libraries** (ADR-0012). `documents`, `graph`,
  `series`, `text` and `vector` depend on `txn`, `codec`, `core` (and
  `expr` where the matrix allows) and never on each other. Glue between
  models belongs in the facade.
- **Stay in your keyspace.** Build every key through the model's `Keyspace`
  handle, from `keycode` or the fixed-width big-endian fields its spec
  defines. Never concatenate unencoded values into keys, and never read or
  write another model's keyspace.
- **One transaction.** Every index, posting, adjacency key, counter and
  statistic a write affects is updated in the same transaction as the write.
  No "fix it up later" paths, except the maintenance jobs the specs define
  (chunking, rollups, rebuilds, sweeps), which run as transactions of their
  own and never change query results.
- **Bounded work.** Every query, traversal, sort, aggregation and search has
  the limits its spec lists. Reaching a limit returns an error or a
  `truncated`/`exhausted` flag as specified; it never allocates without
  bound.
- **Deterministic results.** Ties are broken by a stable key (`_id`,
  `doc_key`, node id), so the same data gives the same order.
- **Each model keeps:**
  - a model-based test against a naive implementation, with reopens;
  - a consistency check that `db.check()` runs (indexes match documents,
    adjacency matches edges, postings match texts, neighbors exist);
  - the bench that measures its budget in `docs/00-overview.md`.
- **The catalog** is the only place names map to keyspace ids. A catalog
  entry change is an on-disk format change (`.claude/rules/on-disk-formats.md`).
- **Public API:** additions are fine within a task; removals and renames
  need the task's authorization and an ADR. The server uses only the
  facade's public API (ADR-0003): never add a private hook for it.
