# Subsystem: `softshell-storage`

The log-structured merge tree (ADR-0007). It stores ordered byte keys and
values with snapshots, atomic write batches and durable commits. It knows
nothing about keyspaces, transactions or models; `txn` builds those on top.

No `unsafe`. Every file operation goes through `Vfs`, so `FaultVfs` can
crash any of them (ADR-0020).

## Components

| Module | Provides |
| :- | :- |
| `batch` | `WriteBatch`: ordered puts and deletes, applied atomically |
| `wal` | WAL segment writer and reader over `codec::record` framing; recovery |
| `memtable` | The persistent copy-on-write B-tree (ADR-0009) and its iterator |
| `sst` | SST writer and reader: data, index, filter and properties blocks, footer |
| `bloom` | Bloom filter build and probe |
| `manifest` | Version edits, the manifest log, `CURRENT` |
| `version` | `Version` (the files of each level, immutable, `Arc`-shared) and `VersionSet` |
| `compaction` | Picking, merging, filters, output splitting, background threads |
| `cache` | Sharded LRU block cache; table cache of open readers |
| `iter` | Merging, level, two-level and bounded iterators, forward and reverse |
| `db` | `Storage`: open, recover, write, get, iterate, snapshot, flush, compact, checkpoint, check, stats, close |

## Internal keys

`internal_key = user_key ++ trailer`, where `trailer` is 8 bytes:
`(sequence << 8) | kind`, stored inverted and big-endian so that, for one
user key, newer versions sort first.

| Kind | Value |
| :- | :- |
| `0x01` | Put |
| `0x00` | Delete (tombstone) |

Sequence numbers are 56 bits and assigned per operation in commit order.
A snapshot is a sequence number: it sees, for each key, the newest version
whose sequence is ≤ the snapshot.

## Write path

1. The caller (in practice `txn`) submits a `WriteBatch` with a sync mode.
2. Under the **commit queue** mutex: assign sequence numbers, encode the
   batch, append it to the active WAL segment, insert it into the active
   memtable, publish the new memtable root. Release the mutex.
3. **Durability wait** (`Sync::Always`): the first waiter whose commit is
   not yet synced becomes the group-commit leader, syncs the WAL once for
   every commit appended so far, and wakes the others. Under
   `Sync::Interval`, a syncer thread syncs on its interval; under
   `Sync::Never`, nothing waits.
4. **Publish**: the visible sequence advances, in commit order, past each
   commit whose durability condition holds (ADR-0011).
5. If the memtable passed `memtable_bytes`, it becomes immutable, a new WAL
   segment and memtable start, and the flush thread is signalled.

## Background errors

A write or sync error on the WAL or the manifest, or on an SST that a
flush or compaction is writing, puts the database into the
background-error state (ADR-0011):

- every commit fails with `Error::Background`, including commits already
  stalled waiting for a flush;
- flush and compaction stop;
- reads continue;
- a reopen is required. A half-written SST is still in `tmp/` and is
  removed on the next open.

A failed sync is never retried.

WAL batch payload (little-endian): `sequence u64`, `count u32`, then per
operation `kind u8`, varint key length, key, and for puts varint value
length, value.

## Recovery

On open:

1. Take `LOCK`. Read `IDENTITY` (create it on first open).
2. Read `CURRENT`, then replay the manifest it names into a `VersionSet`.
   A manifest record that fails its checksum is corruption: open fails.
   A torn last record of the manifest is dropped (it was never synced, so
   nothing referenced it).
3. Replay every WAL segment whose number is ≥ the manifest's log number,
   in order, into a fresh memtable.
   - A `TornTail` in the **last** segment ends recovery there: the tail is
     truncated by starting a new segment, and the offset and byte count are
     logged at `warn`.
   - Corruption anywhere else fails the open with `Error::Corruption`
     naming the segment and offset. There is no "skip corrupted records"
     mode.
4. Flush the recovered memtable to L0, write a version edit, sync it,
   delete the old WAL segments, and remove unreferenced files and `tmp/`.

## SST format (version 1)

```
[data block 0] … [data block n]
[filter block]          # bloom filter over the user keys of the file
[properties block]      # entry count, key range, sequence range, sizes, per-keyspace counts
[metaindex block]       # "filter" → handle, "properties" → handle
[index block]           # last internal key of each data block → handle
[footer: 48 bytes]
```

- **Data block** (target 4 KiB before compression): entries of
  `shared_prefix_len varint, unshared_len varint, value_len varint,
  unshared key bytes, value`; a restart point every 16 entries, where
  `shared_prefix_len = 0`; then the restart offsets (`u32` LE each) and
  their count.
- **Block trailer** (every block): compression type (`0` none, `1` SLZ)
  and the masked CRC-32C of the block bytes and the type.
- **Block handle**: varint offset, varint size.
- **Footer**: metaindex handle and index handle, each padded to 20 bytes;
  format version `u32` LE; magic `SSST` (4 bytes).
- **Bloom filter**: 10 bits per key, 7 probes, by double hashing of
  `h = splitmix64(fnv1a64(user_key))`: probe `i` is `(h_lo + i·h_hi) mod m`.
  The hash is part of the format; changing it is a format change.
- A reader opens a file by reading the footer, then the index and the
  filter, which the table cache keeps. Data blocks go through the block
  cache.
- Files are written to `tmp/NNNNNN.sst`, synced, renamed into the
  directory, the directory synced, and only then named in a version edit.

## Manifest

- Records are version edits, framed by `codec::record`:

  | Field | Meaning |
  | :- | :- |
  | `log_number` | WAL segments below it are obsolete |
  | `next_file_number` | Next number for any file |
  | `last_sequence` | Highest sequence in any SST |
  | `add_file` | Level, number, size, smallest and largest internal key, sequence range |
  | `delete_file` | Level, number |
  | `format` | The format versions this database writes, keycode's included (ADR-0021) |

- Dropped keyspaces are not recorded here: the facade's catalog holds
  them, and `txn` registers a compaction filter for each on open
  (ADR-0012). Until it does, compaction simply keeps their keys.

- `CURRENT` holds `MANIFEST-NNNNNN\n` and is replaced with
  `rename_durable`.
- When the manifest passes 64 MiB, a new one is written starting with a
  snapshot of the current version, synced, and `CURRENT` switched to it.

## Compaction

- **Levels:** L0 holds flushed memtables, which may overlap. L1–L6 do not
  overlap within a level. L1 targets 256 MiB; each level after is 10×.
- **Score:** L0 by file count over `l0_compaction_trigger` (4); other
  levels by bytes over target. The highest score above 1 runs first.
- **Picking:** an L0 compaction takes every overlapping L0 file and the
  overlapping L1 files. A level-n compaction takes the next file after the
  level's round-robin cursor and the overlapping files of level n+1.
- **Merging:** inputs are merged in internal-key order; outputs split at
  `target_file_bytes` (64 MiB) and at user-key boundaries (a user key's
  versions never span two files of one level).
- **Dropping versions:** a version is dropped if a newer version of the
  same user key exists and no live snapshot lies between the two. A
  tombstone is dropped when no older version can exist below it (the
  output level is the bottom for that key range) and no snapshot needs it.
- **Compaction filters:** a filter registered for a key prefix may drop a
  version older than every live snapshot (dropped keyspaces, TTL, series
  retention). Filters must be deterministic.
- **Install:** outputs are synced and renamed, then one version edit adds
  the outputs and deletes the inputs. Inputs are removed from disk when no
  live `Version` references them.
- **Threads:** one flush thread; `compaction_threads` compaction threads
  (default `clamp(cores / 2, 1, 4)`). Two compactions never share a file.

## Backpressure

| Condition | Effect |
| :- | :- |
| L0 files ≥ `l0_slowdown` (8) | Each commit is delayed 1 ms |
| L0 files ≥ `l0_stop` (12) | Commits wait until compaction brings it below |
| Immutable memtables ≥ `max_immutable_memtables` (2) | Commits wait for a flush |

Each stall's start, end and cause are logged at `info` and counted in
stats. A stall alone never fails a commit; a commit that waits longer than
`stall_timeout` (default none) fails with `Error::Stalled`, and a stalled
commit fails with `Error::Background` if a background error occurs while
it waits.

## Reads

- `get(key, snapshot)`: active memtable, immutable memtables newest first,
  L0 files newest first, then one file per level (binary search on key
  ranges), checking each file's bloom filter first.
- `iter(range, snapshot, direction)`: a merging iterator over the same
  sources, skipping versions newer than the snapshot, shadowed versions and
  tombstones. Bounds and prefixes are pushed down so files outside them are
  not opened.
- A read pins the `Version` and memtables it uses; pinned files are not
  deleted until released.

## Caches

- **Block cache:** 16 shards, LRU, keyed by (file number, block offset),
  charged by block size; `block_cache_bytes` default 64 MiB. Index and
  filter blocks are held by the table cache instead.
- **Table cache:** open file readers, at most `max_open_files` (default
  1,000); evicting one closes its file handle.

## Maintenance APIs

- `flush()` and `compact_range(range)` for tests and tools.
- `checkpoint(dest_dir)`: stop file deletions, flush, copy every live SST,
  a new manifest holding a snapshot of the current version and a fresh
  `CURRENT` into `dest_dir` through the `Vfs`, sync them, resume
  deletions. The result opens as a database.
- `check()`: read and verify every block of every live file, the
  manifest's level invariants (no overlap in L1+), and the WAL; returns a
  report of every problem with file and offset.

## Options

| Option | Default |
| :- | :- |
| `sync` | `Sync::Always` |
| `memtable_bytes` | 64 MiB |
| `max_immutable_memtables` | 2 |
| `block_bytes` | 4 KiB |
| `block_cache_bytes` | 64 MiB |
| `target_file_bytes` | 64 MiB |
| `l1_target_bytes` | 256 MiB |
| `l0_compaction_trigger` / `l0_slowdown` / `l0_stop` | 4 / 8 / 12 |
| `compaction_threads` | `clamp(cores / 2, 1, 4)` |
| `max_open_files` | 1,000 |
| `compression` | SLZ |
| `limits` | `Limits::default()` |

## Stats

Per level: files, bytes, compactions, bytes read and written. Totals:
write amplification, space amplification, stall count and time, cache hit
rates, WAL syncs and the group size histogram, flush and compaction
durations, background errors.

## Invariants (checked by `check()` and, under `dev`, after every install)

- Files in L1–L6 do not overlap within their level, and are sorted.
- Every live file's footer and blocks verify.
- `last_sequence` ≥ every sequence in every SST; WAL replay never produces
  a sequence ≤ `last_sequence` of the manifest it follows.
- No user key's versions span two files of one level.
- Nothing is deleted from disk while a live `Version` references it.
