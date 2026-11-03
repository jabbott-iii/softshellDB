# Subsystems: `softshell-core` and `softshell-codec`

## `softshell-core`

Foundation types used by every other crate. No `unsafe`, no OS calls, no
dependencies.

| Module | Provides |
| :- | :- |
| `error` | `CoreError`, a leaf enum whose offsets are absolute `u64`s; `Context<E>` and the `ErrorContext` helper that adds "while doing X" context to any crate's error; `Chain`, which prints an error with its `source()` causes. Each error's `Display` describes its own layer, and `source()` gives the cause |
| `limits` | `Limits` (below), passed to every decoder |
| `bytes` | `ByteReader`: a cursor over `&[u8]` whose every read is checked, never panics, never allocates and leaves the reader unchanged on error; little-endian and big-endian fixed-width reads; `sub_reader` and `split_tail` split off readers that keep absolute offsets (`with_base`); `check_count` for fixed-width counts. `ByteWriter` |
| `varint` | LEB128 `u32`/`u64` and zigzag `i64`. Decoding rejects overlong, overflowing and non-minimal encodings, so each value has one encoding. `read_at_most` checks a declared size against a limit; `read_len` and `read_count` also check it against the input, for bytes or items that start right after the varint. A checked count bounds the items the input holds, not memory: nested decoders cap what they preallocate |
| `crc32c` | CRC-32C (Castagnoli), table-driven slicing-by-8; `extend` continues a CRC over more bytes; `mask`/`unmask` for stored checksums (a checksum of data that contains checksums stays strong) |
| `hash` | `fnv1a64` (stable, may be persisted); `FastHasher` with `FastHashMap`/`FastHashSet` (deterministic, never persisted, not resistant to deliberate collisions) |
| `sha256` | SHA-256 per FIPS 180-4, one-shot, used only for API tokens (ADR-0019) |
| `rng` | `Pcg32` (PCG-XSH-RR, seedable with `new(seed, stream)`: `next_u32`, `next_u64`, `next_f64`, unbiased `range`, `shuffle`). Same-seed streams are correlated: independent generators need different, well-mixed seeds |
| `id` | `Id` (128-bit): 48-bit Unix milliseconds + 80 random bits, the ULID layout, so bytes, value and text sort alike; `IdGenerator` (strictly increasing per generator; the facade keeps one per open database), with its randomness from an `Entropy` source (the OS in the facade, `Pcg32` in tests); text form is 26 characters of Crockford base32, upper case on output and either case on input |
| `clock` | `Clock` trait (`&C`, `Box<C>` and `Arc<C>` are clocks too), `SystemClock`, `ManualClock` for tests; `Timestamp` (i64 nanoseconds since the Unix epoch, UTC: 1677-09-21T00:12:43.145224192Z to 2262-04-11T23:47:16.854775807Z, ADR-0013), whose `Display` and `FromStr` are RFC 3339 |
| `civil` | Unix time ↔ UTC civil date-time (`DateTime`, by Hinnant's days-to-civil algorithms); RFC 3339 formatting and parsing. Output is UTC with exactly nine fraction digits, so each timestamp has one 30-character form and text order is time order. Input may have 0 to 9 fraction digits, any UTC offset, and `T`/`Z` in either case. A leap second (`:60`), more than nine fraction digits, and a time outside the `Timestamp` range are errors, never rounded or clamped |
| `log` | Macros `log::error!`, `warn!`, `info!`, `debug!`, `trace!`, with the module path as target. One process-wide logger, installed by the application, never by a library (with none installed, nothing is written). It has a `Filter` (a level per module-path prefix) and sinks: stderr, rotating file, in-memory ring buffer. `capture` collects what a test's own thread logs. Messages are capped at 16 KiB and escaped to one line. The file sink writes its own log files with `std::fs`: they are not database files, are never synced, and need no `Vfs` crash semantics |
| `metrics` | `Counter`, `Gauge`, `Histogram` (log-linear: 16 buckets per power of two, so a quantile is at most 1/16 above the true value; p50/p95/p99) on atomics; a `Registry` that names them and snapshots them in name order |
| `cli` | Minimal argument parser for `xtask`, the CLI and the bench: subcommands, `--flag`, `--key value` or `--key=value`, positionals (read after every option), `--`. Anything the program did not read is an error, and every `CliError` means exit status 2 |
| `testing` | `check`, `check_sized`, `assert_approx!`, and the fuzz driver that every crate's fuzz targets run on (see `docs/04-quality.md`, ADR-0020) |

### `Limits`

Every decoder takes a `Limits` and checks each size against it, and against
the remaining input, before allocating.

| Field | Default | Used by |
| :- | :- | :- |
| `max_depth` | 64 | JSON, SDOC, query documents |
| `max_document_bytes` | 16 MiB | SDOC, JSON, protocol messages |
| `max_string_bytes` | 16 MiB | JSON, SDOC |
| `max_entries` | 1,000,000 | Array and object entries, per container |
| `max_key_bytes` | 8 KiB | keycode, storage keys |
| `max_vector_dims` | 4,096 | `F32Vec`, vector indexes |
| `max_decompressed_bytes` | 4 MiB | SLZ blocks, series chunks |

### Invariants

- `Pcg32` output is identical on every target for the same seed (fixed
  test vectors).
- `crc32c(b"123456789") == 0xE306_9283`.
- `sha256(b"abc")` matches FIPS 180-4's example; the NIST CAVP short and
  long message vectors pass.
- `IdGenerator` issues strictly increasing ids, even if the wall clock
  steps back: it keeps the larger millisecond and adds one to the last id
  (a carry out of the random part moves the millisecond forward).
- `parse_rfc3339(&format_rfc3339(t)) == Ok(t)` for every `Timestamp` `t`.
- `FastHasher` output is the same on every supported target, but a
  `FastHashMap`'s iteration order also depends on its whole sequence of
  operations (capacity, inserts, removals) and on the target and toolchain
  (`std`'s table layout differs between x86_64 and aarch64). Nothing that
  reaches disk, a result or a test assertion iterates one.

## `softshell-codec`

Byte formats: the value model, text and binary encodings, the key encoding,
compression and record framing. No `unsafe`, no I/O: every function works
on byte slices. Every decoder is a fuzz target and follows
`.claude/rules/on-disk-formats.md`.

### `Value`

```rust
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Timestamp(Timestamp),   // i64 ns since the Unix epoch, UTC
    Id(Id),                 // 128-bit
    Array(Vec<Value>),
    Object(Object),         // insertion-ordered; duplicate keys rejected
    F32Vec(Vec<f32>),       // dense vector
}
```

- **Order** (ADR-0013): `Null < Bool < numbers < String < Bytes < Id <
  Timestamp < Array < Object < F32Vec`.
  - `Int` and `Float` compare by mathematical value, exactly, for every
    pair (`i64::MAX` vs `9.223372036854776e18` included). `5` equals `5.0`.
  - NaN equals NaN and sorts after every other number. `-0.0` equals `0.0`.
  - Strings and bytes compare bytewise; arrays and objects
    lexicographically by element (objects by `(key, value)` pairs in order).
- **Timestamp range:** 1677–2262, the span of an `i64` of nanoseconds
  (ADR-0013). Extended JSON or RFC 3339 text outside it is an error.
- **Field names** in stored documents: 1–1,024 bytes of UTF-8, no `.`
  (the path separator), not starting with `$` (reserved for operators and
  extended JSON).
- `value!` builds values with JSON-like syntax; `ToValue`/`FromValue` and
  `record!` convert Rust types (ADR-0013).

### JSON and JSONC

- `json::parse(&str, Limits)` accepts RFC 8259; `jsonc::parse` also accepts
  `//` and `/* */` comments and trailing commas. Writers:
  `to_string_compact`, `to_string_pretty`.
- A number without a fraction or exponent that fits `i64` is `Int`;
  anything else is `Float`. A number outside `f64` range is an error.
- Duplicate object keys are an error.
- **Extended JSON** maps the types JSON lacks, in both directions:

  | Value | JSON form |
  | :- | :- |
  | `Id` | `{"$id": "01JBX…"}` (26 characters) |
  | `Timestamp` | `{"$ts": "2026-11-01T12:00:00.000000001Z"}` |
  | `Bytes` | `{"$bytes": "<base64, RFC 4648, padded>"}` |
  | `F32Vec` | `{"$f32vec": [0.1, 0.2]}` |
  | Non-finite `Float` | `{"$float": "NaN" \| "Infinity" \| "-Infinity"}` |

- Errors carry the line and column.

### SDOC — binary documents (format version 1)

A stored document is one version byte (`0x01`) followed by one value. A
value is a tag byte and a payload:

| Tag | Type | Payload |
| :- | :- | :- |
| `0x00` | Null | — |
| `0x01` / `0x02` | false / true | — |
| `0x03` | Int | zigzag varint |
| `0x04` | Float | 8 bytes, little-endian IEEE 754 bits, kept exactly |
| `0x05` | String | varint length, UTF-8 bytes (validated) |
| `0x06` | Bytes | varint length, bytes |
| `0x07` | Timestamp | zigzag varint nanoseconds |
| `0x08` | Id | 16 bytes, big-endian |
| `0x09` | Array | varint byte length of the rest, varint count, values |
| `0x0A` | Object | varint byte length of the rest, varint count, then per field: varint key length, key, value |
| `0x0B` | F32Vec | varint count, 4 bytes per element, little-endian |

- Containers carry their byte length so a reader can skip a field without
  decoding it (projections, path lookups).
- One encoding per value: minimal varints, no unused tags. Decoding then
  encoding returns the same bytes (a fuzz assertion).
- The decoder checks depth, total size, and every declared length against
  `Limits` and the remaining input before allocating.

### keycode — order-preserving keys

`keycode::encode(&[Value]) -> Vec<u8>` encodes a tuple so that byte order
of encodings equals tuple order under the value comparator. Used for every
index key, document `_id` key, graph key and series key.

| Element | Encoding |
| :- | :- |
| Null | `0x10` |
| false / true | `0x20` / `0x21` |
| Number | `0x30`, then the 8-byte order key of `r` (the nearest `f64` to the value, `-0.0` as `0.0`: flip all bits if negative, else flip the sign bit; big-endian), then a 2-byte sign-flipped big-endian `i16` holding the exact difference `value − r` (non-zero only for large `Int`s) |
| NaN | `0x31` |
| String | `0x40`, bytes with `0x00` escaped as `0x00 0xFF`, then terminator `0x00 0x01` |
| Bytes | `0x50`, same escaping and terminator |
| Id | `0x60`, 16 bytes big-endian |
| Timestamp | `0x70`, 8 bytes: the `i64` with its sign bit flipped, big-endian |
| Array | `0x80`, each element encoded, then `0x00` |
| Object | `0x90`, for each field the key (as a String element) then the value, then `0x00` |
| F32Vec | not encodable: `Error::NotIndexable` |

- **Descending** fields invert every byte of their element's encoding.
  Every element encoding is prefix-free, so inversion reverses the order.
- Decoding returns `Int` for an integral number that fits `i64` and `Float`
  otherwise: keycode keeps value order, not the numeric type. Callers that
  need the original type read the record.
- Property tests: for random `a` and `b`,
  `cmp(a, b) == bytes_cmp(encode(a), encode(b))`, for single values,
  tuples, mixed `Int`/`Float` near `±2^53` and `±2^63`, and descending
  fields; `encode(decode(encode(v))) == encode(v)`.

### SLZ — block compression (format version 1)

An LZ77 format for SST blocks, designed for fast decoding:

- Stream: varint decompressed length, then sequences until that length is
  produced.
- Sequence: a token byte (high nibble literal length, low nibble match
  length − 4; 15 means "more length follows as varint"), the literals, a
  2-byte little-endian offset (1–65,535), then the match. The last sequence
  has literals only.
- The encoder is greedy with a 4-byte hash table of 4,096 entries:
  deterministic, no allocation per call beyond its reused table.
- The decoder checks the declared length against
  `Limits::max_decompressed_bytes`, every offset against the bytes already
  produced, and every length against the remaining output.
- A block stores SLZ only if it saves at least 12.5%; otherwise it stores
  the raw bytes (a compression-type byte in the block trailer says which).

### Record framing (format version 1)

The WAL and the manifest are sequences of records framed in 32 KiB blocks,
following the LevelDB log format's design, so a torn write damages at most
the records it touched:

- Each fragment: masked CRC-32C (4 bytes LE) of type and payload, length
  (2 bytes LE), type (1 byte: `FULL`, `FIRST`, `MIDDLE`, `LAST`), payload.
- A record that does not fit in the rest of a block is split into
  fragments. A block's last 1–6 bytes, too short for a header, are zeros.
- The reader reports, for the last block of the last file only, a bad
  checksum or a truncated fragment as `TornTail { offset }`; anywhere else
  it is `Corruption { offset }`. The storage engine decides what to do
  (`docs/subsystems/storage-engine.md`).
- Records are at most 64 MiB (a write batch is bounded by `txn`).

### Series chunk codec (format version 1)

One chunk holds the points of one (series, field) in one time bucket,
in timestamp order (ADR-0015):

- Header: version byte, value kind (`f64` or `i64`), varint point count,
  zigzag varint first timestamp.
- Timestamps: the second as a zigzag varint delta; then delta-of-deltas in
  a bit stream with prefix codes: `0` (zero), `10` + 7 bits, `110` + 9 bits,
  `1110` + 12 bits, `11110` + 32 bits, `11111` + 64 bits.
- `f64` values: the first as 64 raw bits; then each XORed with the previous:
  `0` if equal; `10` + the meaningful bits if they fit in the previous
  window; `11` + 6 bits leading zeros + 6 bits length + the meaningful bits.
- `i64` values: zigzag delta varints.
- The bit reader is bounds-checked; a count that the remaining bits cannot
  hold is an error before any allocation.
