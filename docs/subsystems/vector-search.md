# Subsystem: `softshell-vector`

Nearest-neighbor indexes over one keyspace per index (ADR-0017). Like
`text`, it knows documents only as `(doc_key, vector)`; the facade feeds it
from a collection field.

## Index definition

| Setting | Values |
| :- | :- |
| `dims` | 1 to 4,096 |
| `metric` | `L2` (squared), `Cosine` (vectors normalized at insert; a zero vector is rejected), `Dot` |
| `kind` | `Flat`, or `Hnsw { m: 16, ef_construction: 200 }` |
| `seed` | PCG32 seed for level assignment (stored, so rebuilds are reproducible) |

A vector whose length differs from `dims`, or that holds NaN or an
infinity, fails the write.

## Key layout (inside the index's keyspace)

| Key | Value |
| :- | :- |
| `0x01 ++ node u64 BE` | `doc_key`, deleted flag, level, the vector (`f32` LE) |
| `0x02 ++ doc_key` | node `u64` |
| `0x03 ++ level u8 ++ node u64 BE` | neighbor list: varint count, node ids (delta varints, sorted) |
| `0x04 ++ "meta"` | entry point, max level, next node id, live and deleted counts |

Flat indexes use only `0x01`, `0x02` and `0x04`.

## Search

```rust
rx.vector_index("notes", "body_vec")?
    .search(&query_vec, 10)
    .ef(64)
    .filter(query!({ "tags": "ocean" }))   // applied by the facade to fetched documents
    .run()?   // Vec<(doc_key, distance)>
```

- **Flat:** scan every live node, keep the top k in a bounded heap. Exact;
  the oracle for HNSW tests.
- **HNSW:** greedy descent from the entry point through the upper levels,
  then a best-first search with candidate list `ef` (default
  `max(k, 64)`) on level 0. Deleted nodes are traversed but not returned.
- **Filtered search:** the facade asks for `k × expansion` candidates
  (expansion 4, doubling up to `max_ef`, default 1,024) and filters them;
  if fewer than k pass at `max_ef`, it returns what it has and reports
  `exhausted: true`.
- Distances are computed in `f32` with `f64` accumulation for `L2` and
  `Dot` over more than 1,024 dimensions.

## Insert, update and delete

- **Insert (HNSW):** assign a level from the seeded PCG32
  (`floor(−ln(u) · 1/ln(m))`), search for neighbors at each level, connect
  with the neighbor-selection heuristic, and prune neighbors' lists back to
  `m` (`2m` on level 0). All writes are in the writing transaction; the
  single writer means no conflicts (ADR-0010).
- **Update:** delete then insert, in one transaction.
- **Delete:** set the deleted flag and decrement the live count. Neighbors
  are repaired lazily: a search that meets a deleted node continues through
  its neighbors.
- **Rebuild:** when deleted nodes pass 20% of the index, the maintenance
  thread builds a fresh index in a new keyspace in batches, then switches
  the catalog to it in one transaction and drops the old keyspace.

## Tests

- **Recall test:** a seeded dataset of Gaussian clusters (100k × 128 in
  the bench, 5k × 32 in `cargo test`); HNSW recall@10 against `Flat` must
  meet the budget at the default `ef`.
- **Model test:** random inserts, updates, deletes and reopens; `Flat`
  results equal brute force over a `Vec`.
- **Determinism:** two builds with the same seed and insertion order write
  identical keys and values.
- **Consistency check:** every neighbor id exists, every `0x02` entry
  points at its node and back, counts match (part of `softshell check`).
