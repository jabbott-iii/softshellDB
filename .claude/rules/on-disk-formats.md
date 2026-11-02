---
paths:
  - "crates/codec/**"
  - "crates/storage/**"
  - "crates/proto/**"
  - "crates/softshelldb/src/catalog*"
  - "crates/softshelldb/src/catalog/**"
  - "tests/fixtures/**"
  - "fuzz/**"
---

# Rules for decoders, on-disk formats and the wire protocol

Every byte we read may be malformed or hostile: a corrupted disk, a
database file from someone else, a malicious client.

- **Never panic on input.**
  - Decoder modules carry
    `#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]`.
  - Read through `softshell_core::bytes::ByteReader`; no slicing with
    input-derived offsets.
  - No `unwrap`/`expect` on input-derived values.
  - Lengths and counts use `checked_add`/`checked_mul` and return an error
    on overflow.
- **Check before allocating.** Compare every declared size against
  `Limits` and against the remaining input before `Vec::with_capacity`,
  `resize` or `String` allocation.
- **Bound recursion** (JSON, SDOC, query documents, keycode arrays and
  objects) by `Limits::max_depth`. Exceeding it is an error, never a stack
  overflow.
- **Errors carry context:** the format, the byte offset (or line and column
  for text), what was expected, and what was found.
- **Checksums before trust.** Verify a block's or record's checksum before
  decoding its contents.
- **Every new decoder:**
  1. registers an `xtask fuzz-smoke` target with a seed corpus in
     `fuzz/corpus/<target>/`, listed in `docs/04-quality.md`;
  2. has hand-made test vectors beside its tests;
  3. is specified in the relevant `docs/subsystems/` file.
- **Format changes:** any change to what a format writes follows
  `/change-on-disk-format`: version bump, a reader for old versions or an
  explicit `UnsupportedVersion` error, new fixtures in
  `tests/fixtures/<format>/v<N>/`, an updated spec. Never rewrite committed
  fixtures; add a new version directory.
- **Order is a format.** keycode's byte order and the value comparator are
  on-disk contracts (every index depends on them). Changing either is a
  format change for every index.
- **Determinism:** encoders produce one encoding per value; SST and
  manifest output contain no timestamps or random data; iteration that
  reaches disk is sorted.
