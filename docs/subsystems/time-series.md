# Subsystem: `softshell-series`

Time-series over one keyspace per series set (ADR-0015).

## Model

- A **series set** is a named container with options: bucket width
  (default 2 h), retention (default none), rollup rules.
- A **series** is a measurement name plus a tag set (string keys and
  values, sorted by key, at most 32 tags). It maps to a 64-bit series id,
  allocated by the series set.
- A **point** is a timestamp (`Timestamp`, ns) and fields: `f64` or `i64`
  numbers. A field's kind is fixed by its first point; writing the other
  kind to it is an error.
- Writing the same series, field and timestamp twice keeps the last write.

## Key layout (inside the series set's keyspace)

| Key | Value |
| :- | :- |
| `0x01 ++ keycode(measurement, tags…)` | series id `u64` BE |
| `0x02 ++ series_id u64 BE` | SDOC `{measurement, tags, fields: {name: kind}}` |
| `0x03 ++ keycode(tag_key, tag_value) ++ series_id` | empty (tag index) |
| `0x04 ++ series_id ++ field_id u16 ++ timestamp i64 (sign-flipped BE)` | raw point value (8 bytes) |
| `0x05 ++ series_id ++ field_id u16 ++ bucket_start i64 (sign-flipped BE)` | series chunk (`docs/subsystems/core-and-codec.md`) |
| `0x06 ++ "meta"` | Next series id, last chunked bucket per series |

## Write path

- `write(measurement, tags, timestamp, fields)` and `write_batch(points)`
  resolve (or allocate) the series id and write one raw-point key per
  field. No read-modify-write of chunks happens on the write path.
- Points may arrive in any order. A point in a bucket that is already
  chunked is still written as a raw key; the next chunking pass of that
  bucket merges it.

## Chunking (maintenance)

- The facade's maintenance thread calls `chunk_closed_buckets(budget)`: for
  each series and field, every bucket that ended more than one bucket width
  ago and has raw points is merged with its existing chunk (if any), encoded
  as one chunk, and its raw keys deleted, in a short write transaction.
- A bucket holds at most 65,536 points per field; a fuller bucket is split
  into several chunks keyed by their first timestamp.
- Chunking never changes query results: a test runs every query before and
  after chunking and compares.

## Queries

```rust
rx.series("activity")?
    .query("edits")
    .tags(query!({ "note": { "$in": [a, b] } }))     // expr filter over the tag object
    .range(t0..t1)
    .window(minutes(5), [Agg::Sum, Agg::Count])
    .group_by(["note"])
    .fill(Fill::None)
    .run()?
```

- Tag filters are evaluated over the tag index: equality and `$in` use it;
  other operators filter the candidate series.
- Raw range reads merge chunks and raw points in timestamp order; a raw
  point overrides a chunk point with the same timestamp.
- Aggregates: `min`, `max`, `sum`, `count`, `mean`, `first`, `last`.
  Windows align to the Unix epoch. `Fill`: none, null, previous.
- Bounds: `max_series_per_query` (default 10,000) and `max_points_scanned`
  (default 100 million); exceeding one is an error naming it.

## Retention and rollups

- **Retention:** points older than `now − retention` are hidden from
  queries at once (by the injected `Clock`) and removed by the maintenance
  thread, which deletes whole chunks and raw keys of expired buckets. A
  compaction filter (`txn` registry) drops any that remain.
- **Rollups:** a rule names a source series set, a window and aggregates,
  and a target series set. The maintenance thread computes each closed
  window once, writes it to the target, and records progress in `0x06`.
  Late points in a rolled-up window trigger a recompute of that window.

## Tests

- **Codec:** chunk round-trips on regular, jittered, constant, random and
  extreme series; the fuzz target `series_chunk`; bytes per point measured
  on a regular series (budget ≤ 2).
- **Model test:** random writes (in and out of order), chunking passes,
  queries and reopens against sorted vectors.
- **Retention:** a `ManualClock` moved forward hides then removes buckets.
