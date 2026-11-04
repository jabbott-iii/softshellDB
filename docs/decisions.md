# Architecture decision records

New ADRs are appended at the end using the template below; `/record-decision`
does this for you. Never edit an accepted ADR's decision. Supersede it with a
new ADR and set the old one's status to `Superseded by ADR-NNNN`.

An ADR is `Accepted` only when the user made or approved the decision. The
initial design (ADR-0004 to ADR-0021) was proposed with the roadmap and
reviewed by the user in task D0.1 (`docs/roadmap.md`) on 2026-11-01. All
eighteen were accepted. ADR-0005, ADR-0010, ADR-0011, ADR-0012, ADR-0013,
ADR-0020 and ADR-0021 were amended in that review before acceptance, in
place, because none had been accepted yet; the D0.1 note lists what
changed.

```markdown
## ADR-NNNN — <title>

- **Status:** Proposed | Accepted | Superseded by ADR-NNNN
- **Date:** YYYY-MM-DD
- **Context:** <forces at play, constraints, what prompted this>
- **Decision:** <what we will do>
- **Consequences:** <good and bad results, follow-up work>
- **Alternatives considered:** <option — why not>
- **Revisit when:** <a concrete trigger>
```

---

## ADR-0001 — One embedded engine for six data models

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** "All-in-one NoSQL" could mean anything from a key-value store
  to a multi-model platform. Applications that need documents, search,
  vectors, a graph and metrics today run four or five systems and keep them
  in sync by hand.
- **Decision:**
  - softshellDB covers key-value, documents, graph, time-series, full-text
    search and vector search.
  - All six are built over one transactional key-value core, so one commit
    can span every model.
- **Consequences:**
  - Breadth is the main schedule risk. Each model is a library over the
    same transaction API (ADR-0012), finished one milestone at a time.
  - Each model gets a smaller feature set than a dedicated system would
    (no Cypher, no SQL, English-only stemming at first). The backlog holds
    the rest, each item with a trigger.
  - Cross-model atomicity comes for free from the shared WAL.
- **Alternatives considered:**
  - Key-value plus documents only: smaller, but not "all in one", and the
    user chose all six.
  - Separate engines per model behind one API: no shared transactions, and
    several storage engines to keep crash-safe.
- **Revisit when:** a model's milestone runs past twice its estimate, or a
  model's needs force a change to the storage engine that would hurt the
  others.

## ADR-0002 — Build it ourselves; zero third-party crates

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** The user's rule, carried over from Nadir: a dependency is
  acceptable only if building the capability in-house is unreasonable. For
  a database, external code is also supply-chain and durability risk that
  we cannot audit cheaply.
- **Decision:**
  - The approved third-party crate set starts empty.
  - Exceptions follow the test and process in
    `docs/02-dependency-policy.md`, and require user approval.
  - Enforced by `cargo xtask deps`.
- **Consequences:**
  - Much more code to write and test: codecs, compression, checksums,
    caches, the server, the test harnesses.
  - TLS, authenticated encryption and password hashing are the likely
    exceptions (criterion 2). They stay out until evaluated: the server is
    loopback-only until TLS is decided (ADR-0018).
  - Full understanding of the stack; no transitive supply-chain risk; small
    binaries and fast clean builds.
- **Alternatives considered:**
  - A std-only storage core with curated crates (tokio, rustls) at the
    edges. Faster for the server, but the user chose build-first.
  - Curated dependencies throughout, as most Rust databases do.
- **Revisit when:** a switch trigger in the register fires.

## ADR-0003 — Library first, then a single-node server; replication deferred

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** An embedded library is the core of every deployment shape:
  a server is a library plus a protocol, and replication is a server plus
  log shipping. Building the server first would put a network between
  every test and the engine.
- **Decision:**
  - The embedded crate `softshelldb` is the primary product.
  - A single-node server (`softshell-server`), a wire protocol and a Rust
    client follow in M11, built on the public facade API only.
  - Replication is in the backlog with a trigger. Sharding and consensus
    are non-goals.
- **Consequences:**
  - The facade's API must be complete enough for the server to use with no
    private hooks: anything the server needs, an embedder can have too.
  - Interactive server transactions hold the single writer lock across
    network round trips, so the server bounds their duration (ADR-0018).
- **Alternatives considered:** embedded only; all three on the roadmap.
- **Revisit when:** a user needs high availability or read scaling beyond
  one machine (replication), or the server becomes the main use.

## ADR-0004 — Apache-2.0 header on every source file; NOTICE at the root

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** The repository's `LICENSE` is Apache-2.0. Its appendix
  recommends a header in each file, and its section 4(d) carries `NOTICE`
  files forward to redistributions. Nadir, the maintainer's other
  project, does the same.
- **Decision:**
  - Every source file (Rust, scripts, workflows, the server config sample)
    starts with the Apache-2.0 header from `LICENSE`'s appendix, in its
    comment syntax, after a shebang if there is one, with the placeholder
    filled in as `Copyright 2026 Joseph Anthony Abbott III` (as in Nadir).
    `LICENSE` itself stays the unmodified license text.
  - `NOTICE` at the root names the project and copyright holder, and each
    published crate carries copies of `LICENSE` and `NOTICE`.
  - Third-party material (test vectors) keeps its own license and notice
    beside it.
  - `cargo xtask headers` enforces the header.
- **Consequences:** a few lines per file; a check to maintain.
- **Alternatives considered:** a license only at the root (simpler, but
  files copied out of the repository lose their terms).
- **Revisit when:** the license changes.

## ADR-0005 — Layered crates; `unsafe` and OS code in two crates only

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** A database spans byte formats, OS durability, a storage
  engine, five models and a server. Without enforced layering, models reach
  into the engine's internals and every crate grows platform `cfg`s.
- **Decision:**
  - The crate map and dependency matrix in `docs/01-architecture.md`,
    enforced by `cargo xtask layers`.
  - `unsafe` is allowed in `softshell-sys` (raw declarations) and
    `softshell-platform` (their safe wrappers) only. Every other crate
    inherits `unsafe_code = "deny"`.
  - `#[cfg(target_os = …)]`, `cfg(unix)` and `cfg(windows)` appear only in
    those two crates and in `xtask` packaging code.
- **Consequences:**
  - The storage engine is safe Rust. Structures that usually use `unsafe`
    (arena skiplists, lock-free queues) are replaced by safe ones (ADR-0009),
    at some cost in speed.
  - Hardware CRC and SIMD distance kernels need a superseding ADR that
    names the crate allowed to hold them.
  - `xtask` links only `core` and `codec`. Tooling that must exercise a
    higher crate (fuzz targets, fixture writers, crash workloads) lives in
    that crate's tests, and `xtask` runs it through `cargo` (ADR-0020).
- **Alternatives considered:** `unsafe` allowed in `storage` for the
  memtable and cache; rejected while safe designs meet the budgets.
- **Revisit when:** a profile shows a safe structure missing a budget by
  more than 2× after optimization.

## ADR-0006 — Targets: Linux, macOS and Windows, 64-bit; local filesystems only

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** Durability semantics differ by OS and filesystem; each
  target multiplies the crash-test matrix.
- **Decision:**
  - Linux x86_64 and aarch64 (tier 1); macOS 13+ aarch64 and x86_64 and
    Windows 10+ x86_64 (tier 2), as `docs/03-platforms.md` defines.
  - Local filesystems only: ext4, XFS, btrfs, APFS, NTFS.
  - No WASM, `no_std`, mobile, 32-bit or big-endian targets.
- **Consequences:** `softshell-platform` has three implementations;
  `FaultVfs` simulates both the Unix and the NTFS directory semantics.
- **Alternatives considered:** Linux only (narrower, but the user's
  projects run on all three desktops); WASM with an in-memory VFS (cheap
  later, if the facade keeps `Vfs` pluggable).
- **Revisit when:** a user needs another target, or `std` gains a portable
  durable-rename and directory-sync API.

## ADR-0007 — Storage engine: a log-structured merge tree with leveled compaction

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:**
  - Time-series ingest, inverted-index postings and graph edges are
    write-heavy and append-like.
  - Crash safety is easiest to reason about with append-only logs and
    immutable files: only the WAL and the manifest are ever appended to,
    and every other file is written once, synced, then installed.
  - Each step (WAL + memtable, then SSTs, then compaction) is testable on
    its own, which suits a task-at-a-time roadmap.
- **Decision:**
  - An LSM tree: WAL, memtable, immutable memtables, level-0 SSTs
    (overlapping), levels 1..6 (non-overlapping, ×10 size ratio).
  - Leveled compaction with a compaction-filter hook for dropped keyspaces,
    TTL and series retention.
  - Internal keys carry a 56-bit sequence number and a kind (put, delete),
    so snapshots are sequence numbers.
  - Bloom filters per SST, prefix-compressed data blocks, a block cache.
  - No range tombstones in v1: drops and retention use compaction filters.
- **Consequences:**
  - Read amplification on misses is bounded by bloom filters; range scans
    merge several sources.
  - Write stalls when L0 grows must be designed and visible (stats, logs).
  - Space is reclaimed only by compaction.
- **Alternatives considered:**
  - Copy-on-write B+tree with a single writer (LMDB, redb style): no WAL,
    simple MVCC, but write amplification on every commit (path copying and
    two syncs) and fragmentation that needs its own compaction.
  - B+tree with a WAL (InnoDB style): in-place updates make recovery and
    latching the hardest parts of the project.
- **Revisit when:** the read budgets (M4.6) are missed by more than 2× after
  tuning cache, filters and compaction.

## ADR-0008 — No mmap: positional reads through our own block cache

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** Memory-mapped files hand eviction, read-ahead and error
  handling to the OS. An I/O error on a mapped page is a signal, not a
  `Result`; a file truncated under a mapping crashes the process. Both
  break the rule that hostile files must not crash us (Crotty, Leis and
  Pavlo, "Are You Sure You Want to Use MMAP in Your DBMS?", CIDR 2022).
- **Decision:** SST blocks are read with positional reads into owned
  buffers and kept in a sharded LRU block cache with byte accounting.
- **Consequences:** an extra copy per cache miss; full control over memory
  use and error handling; no `unsafe` for mapping.
- **Alternatives considered:** mmap for SSTs (fewer copies; rejected for the
  reasons above); `O_DIRECT` (backlog, with a trigger).
- **Revisit when:** a profile shows copying from the page cache above 20%
  of read-path CPU.

## ADR-0009 — Memtable: a persistent copy-on-write B-tree in safe Rust

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** Readers must iterate the memtable while the writer inserts.
  The usual answer, a lock-free arena skiplist, needs `unsafe`, which
  ADR-0005 keeps out of `storage`.
- **Decision:**
  - The memtable is a persistent B-tree whose nodes are shared through
    `Arc`. A commit path-copies the nodes it changes and publishes a new
    root; readers clone the root `Arc` and iterate an immutable tree.
  - Memory use is accounted per commit; a memtable becomes immutable when
    it passes the configured size (64 MiB default).
- **Consequences:** readers never block and never see a torn tree; a commit
  allocates O(log n) nodes per changed key, so large batches build their
  changes in a scratch B-tree first and merge.
- **Alternatives considered:** `RwLock<BTreeMap>` (iterators would block the
  writer); an arena skiplist (needs `unsafe`).
- **Revisit when:** memtable inserts exceed 30% of commit CPU in a profile.

## ADR-0010 — Transactions: one writer at a time, snapshot readers, serializable

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** Multi-writer concurrency control (OCC, SSI, locking) is a
  large source of subtle bugs. Every model's index maintenance (document
  indexes, text statistics, HNSW neighbor lists) is read-modify-write on
  shared keys, which would conflict constantly under OCC.
- **Decision:**
  - At most one write transaction runs at a time (a writer lock). It reads
    the last appended commit plus its own buffered writes. Under commit
    pipelining that commit may not be synced yet; reading only synced
    commits instead would let a writer overwrite changes it never saw.
  - Read transactions run concurrently on a snapshot (a sequence number)
    and never block or are blocked.
  - The result is serializable: writes run in a serial order and each read
    transaction observes one committed state.
  - Commit pipelining: the writer lock is released after the WAL append,
    before the sync, so the next writer runs while the previous one waits
    (ADR-0011).
  - Write transactions are bounded in size (default 64 MiB of writes) and,
    over the server, in duration (ADR-0018).
- **Consequences:**
  - No conflicts, no retries, no deadlocks; counters and index statistics
    are simple.
  - Write throughput is bounded by one core; long write transactions delay
    all other writers.
  - A write transaction can read a commit that a crash later erases. The
    database stays consistent: the WAL is synced in order, so a commit is
    acknowledged as durable only when every earlier one is. But a caller
    must not act on values read inside a write transaction until its
    commit returns `Ok`; the API documentation says so.
- **Alternatives considered:** optimistic transactions with read-set
  validation (more parallel, but retries and conflicts on every shared
  index key); two-phase locking (deadlocks).
- **Revisit when:** writer-lock wait exceeds 50% of commit latency at the
  M5.3 load, or a workload needs parallel writers. Multi-writer OCC is in
  the backlog.

## ADR-0011 — A commit is visible only after it is durable; sync errors are fatal

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** If a reader can see a commit before its WAL record is synced,
  a crash can erase data a client already acted on. After a failed
  `fsync`, Linux may drop dirty pages, so retrying can report success for
  lost data.
- **Decision:**
  - Sync policies: `Sync::Always` (each commit waits for a WAL sync, shared
    by group commit), `Sync::Interval(d)` (a syncer thread syncs every
    `d`), `Sync::Never` (the OS decides). The policy can be overridden per
    commit.
  - Under `Sync::Always`, the visible sequence advances past a commit only
    after its sync, so read transactions never see an unsynced commit.
    Under the other policies, visibility follows the WAL append, and the
    documentation says what a crash can lose.
  - "Visible" means visible to read transactions. The next write
    transaction reads the last appended commit, synced or not (ADR-0010).
  - Any sync or write error on the WAL or manifest, or on an SST that a
    flush or compaction is writing, puts the database into a
    background-error state: writes fail with `Error::Background` (commits
    stalled behind a flush included), background jobs stop, reads
    continue, and a reopen is required. A half-written SST stays in `tmp/`
    and is removed on the next open.
- **Consequences:** a slow disk slows visibility, not correctness; a
  transient disk error forces a reopen; a full disk stops writes with an
  error instead of stalling them forever behind a flush that cannot
  finish.
- **Alternatives considered:** visible at append (lower latency, but readers
  see data a crash can erase); retrying failed syncs (unsafe on Linux);
  retrying a failed flush or compaction with backoff (could ride out a
  disk that fills and then frees, but writers stall meanwhile, and it adds
  a second error path to test).
- **Revisit when:** not expected.

## ADR-0012 — One tree; keyspaces are prefixes; models are libraries; the facade owns the catalog

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** Cross-model transactions need one WAL and one sequence. Each
  model still needs its own key layout and maintenance.
- **Decision:**
  - Every key starts with a 4-byte big-endian keyspace id. Keyspace 0 is
    the catalog; 1 holds auth and settings; ids from 2 are allocated by the
    catalog and never reused.
  - Every collection, graph and series set has its own keyspace, and so
    does every index on a collection: secondary, text or vector.
  - `documents`, `graph`, `series`, `text` and `vector` are libraries that
    take keyspace handles and a transaction. They do not depend on each
    other.
  - The facade owns the catalog (names, keyspace ids, index definitions),
    the glue between models (text and vector indexes on document fields,
    hybrid search) and the maintenance thread.
  - Dropping a collection or index is a catalog write that puts its
    keyspace on a dropped list (a collection's together with all of its
    indexes'); a compaction filter and the maintenance sweep remove their
    keys later. Because keyspace ids are never reused, a new index never
    inherits a dropped index's entries.
- **Consequences:**
  - One compaction policy for every model at first. Time-series data,
    which prefers time-windowed compaction, shares leveled compaction.
  - Model crates are tested alone against `TxnDb::open_in_memory()`.
  - One drop mechanism for everything. A collection with n indexes uses
    n + 1 keyspaces; 32-bit ids leave plenty.
- **Alternatives considered:** a separate LSM tree per keyspace, like column
  families (per-model compaction, but more files and a shared WAL to
  coordinate); models depending on `documents` directly (simpler glue,
  but a tangle); secondary indexes inside the collection's keyspace under
  never-reused index ids, dropped through a list of dropped prefixes
  (fewer keyspaces, but a second drop mechanism to get right).
- **Revisit when:** series retention or ingest dominates compaction I/O in a
  profile; then consider per-keyspace trees (backlog).

## ADR-0013 — The value model, SDOC, keycode, and conversions without serde

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** Documents, graph properties, series fields, query documents
  and protocol messages all need one value type, a compact binary form, and
  an order for index keys.
- **Decision:**
  - `Value`: `Null`, `Bool`, `Int(i64)`, `Float(f64)`, `String`, `Bytes`,
    `Timestamp` (nanoseconds since the Unix epoch, UTC), `Id` (128-bit),
    `Array`, `Object` (insertion-ordered, duplicate keys rejected),
    `F32Vec` (dense vectors).
  - A `Timestamp` is an `i64`, so it covers
    1677-09-21T00:12:43.145224192Z to 2262-04-11T23:47:16.854775807Z.
    RFC 3339 text or extended JSON outside that range is an error, never
    clamped.
  - **SDOC**, our binary encoding: a version byte, then tagged values with
    varint lengths; objects and arrays carry their byte length so readers
    can skip them. It is not BSON-compatible.
  - **keycode**, an order-preserving encoding of value tuples: byte order
    of the encodings equals the value order, defined once in
    `softshell-codec` and property-tested against the comparator.
  - Value order across types: `Null < Bool < numbers < String < Bytes <
    Id < Timestamp < Array < Object < F32Vec`. `Int` and `Float` share one
    numeric order, exact for every pair, and equal values are equal (`5`
    equals `5.0`, in sorts and in unique indexes). NaN sorts after every
    other number. Strings compare by UTF-8 bytes (no locale collation in
    v1).
  - Rust types convert through `ToValue` and `FromValue`; the `record!`
    `macro_rules!` macro implements both for plain structs. No proc macros.
- **Consequences:**
  - One value model everywhere; index order is portable and stable on
    disk, so changing it is a format change.
  - Nanosecond precision for time-series, at the cost of range: dates
    before 1677 or after 2262 (including `9999-12-31` "end of time"
    placeholders) cannot be `Timestamp`s, and callers store them as
    strings or `Null`. Widening the range later is a format change to
    SDOC, keycode and series chunks.
- **Alternatives considered:** BSON (a large spec with deprecated types);
  separate `Int` and `Float` orders like FoundationDB tuples (simpler
  encoding, surprising query results); serde (excluded by ADR-0002).
- **Revisit when:** locale-aware collation is required, the `record!`
  macro cannot express more than three needed type shapes, or a user needs
  timestamps outside 1677–2262.

## ADR-0014 — Query documents parsed into a typed AST; a rule-based planner

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** Callers need to filter documents from Rust and over the
  network. MongoDB-style query documents are familiar, need no grammar of
  their own, and travel as `Value`s.
- **Decision:**
  - Filters, projections, sorts and update specs are written as `Value`s
    (`{"age": {"$gte": 21}}`) and parsed once by `softshell-expr` into a
    typed AST with paths, comparison, set, existence and boolean operators.
    Unknown operators are errors.
  - The documents planner is rule-based: equality predicates on an index
    prefix, then at most one range on the next field, then the index that
    also satisfies the sort. Otherwise a full scan. `explain()` reports the
    choice.
  - No SQL, no joins.
- **Consequences:** predictable plans; poor choices on skewed data until a
  cost-based planner exists (backlog).
- **Alternatives considered:** a text query language (a parser and grammar
  to maintain); SQL (out of scope).
- **Revisit when:** users hit plan regressions that index hints cannot fix.

## ADR-0015 — Time-series: raw points first, then compressed chunks

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** Rewriting a compressed chunk for every point is
  read-modify-write on hot keys. Gorilla-style compression (Pelkonen et
  al., VLDB 2015) reaches about 1.4 bytes per point on regular series.
- **Decision:**
  - A series is a measurement plus a sorted tag set, mapped to a 64-bit
    series id. A tag index maps tag pairs to series ids.
  - Points are first written as raw keys, one per (series, field,
    timestamp). The maintenance thread later compresses each closed time
    bucket (2 hours by default) into one chunk: delta-of-delta timestamps,
    XOR-encoded floats, zigzag-delta integers.
  - Queries merge chunks with raw points; a raw point overrides a chunk
    point with the same timestamp.
  - Retention deletes whole buckets; rollups write aggregates into another
    series set.
- **Consequences:** ingest is cheap; storage is compact only after
  chunking; out-of-order points are allowed and merged later.
- **Alternatives considered:** append to an open chunk in place (RMW on
  every point); one key per point forever (simple, about 20 bytes per
  point after prefix compression).
- **Revisit when:** chunking falls behind ingest at the M8.6 load.

## ADR-0016 — Full-text: one key per posting, BM25, re-analysis on update

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** A posting list stored as one value is rewritten on every
  document change. An LSM with prefix-compressed blocks stores many small
  keys cheaply.
- **Decision:**
  - A posting is a key `term ++ doc_key` whose value holds the term
    frequency and positions. Document lengths, document frequencies and
    collection statistics are their own keys.
  - On update or delete, the old text is re-analyzed to find the postings
    to remove; there is no forward index. Changing an analyzer requires a
    reindex.
  - Ranking is BM25 (k1 = 1.2, b = 0.75). Queries: term, phrase, prefix,
    boolean (must, should, must-not).
  - Analyzer v1: split on non-alphanumeric characters (`std` Unicode
    properties), lowercase, ASCII folding, English stopwords, Porter2
    stemming.
- **Consequences:** no merge operator needed; phrase queries need positions;
  dynamic pruning (WAND) comes later.
- **Alternatives considered:** Lucene-style immutable segments (fast, but a
  second storage engine to make crash-safe).
- **Revisit when:** top-k latency misses its budget at 1M documents after
  caching, or a non-whitespace language is needed.

## ADR-0017 — Vector search: HNSW stored in the key-value store; the flat index is ground truth

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** Vector indexes must be transactional with their documents
  and survive crashes like everything else.
- **Decision:**
  - Metrics: squared L2, cosine (vectors normalized at insert), dot
    product. Dimension fixed per index, up to 4,096.
  - Two index kinds: `Flat` (exact scan, also the test oracle) and `Hnsw`
    (M = 16, ef_construction = 200, ef_search per query). HNSW nodes,
    vectors and neighbor lists are keys in the index's keyspace, read
    through the block cache.
  - Inserts happen inside the writing transaction. The single writer
    (ADR-0010) means neighbor-list updates never conflict.
  - Deletes mark nodes deleted and repair neighbors lazily; the maintenance
    thread rebuilds an index whose deleted share passes 20%.
  - Level assignment uses a PCG32 seeded per index, so builds are
    reproducible.
  - Filtered search post-filters with an expanded ef.
- **Consequences:** HNSW inserts hold the writer lock for their duration;
  bulk loads should batch.
- **Alternatives considered:** an in-memory HNSW rebuilt on open (fast
  queries, slow opens, not transactional); IVF-PQ (needs training, worse
  recall at small scale).
- **Revisit when:** recall or latency misses the budget at 100k × 128, or
  insert time blocks writers at the M10.4 load.

## ADR-0018 — Server: std threads, our own binary protocol, loopback-only until TLS

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** No async runtime is available (ADR-0002); TLS needs a
  specialist dependency we have not evaluated.
- **Decision:**
  - `std::net::TcpListener`, an acceptor thread and a bounded pool of
    connection threads (`max_connections`, default 256).
  - A length-prefixed binary protocol of SDOC messages, with a versioned
    handshake (`docs/subsystems/server-and-protocol.md`).
  - Every connection has read, write and idle timeouts; every request a
    size limit; every result a batch limit with cursors.
  - Interactive write transactions over the network have a maximum
    duration (default 5 s), after which the server aborts them and
    releases the writer lock.
  - The server binds loopback addresses only. Binding any other address
    is refused until TLS is approved and implemented (M11.6, M11.7).
    There is no flag to bypass this.
- **Consequences:** remote access needs an SSH tunnel until TLS lands; one
  thread per connection limits concurrency to hundreds of connections.
- **Alternatives considered:** an own epoll/kqueue/IOCP event loop (much
  more OS surface and `unsafe`); allowing plaintext remote binds behind a
  flag (rejected: credentials would cross the network in clear).
- **Revisit when:** a deployment needs more than about 2,000 concurrent
  connections (register item 3), or TLS is decided.

## ADR-0019 — Authentication with hashed random API tokens; SHA-256 built in-house for that only

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** Passwords need a slow KDF (a crypto dependency, criterion 2).
  Random tokens of 256 bits need only a fast hash to be stored safely.
- **Decision:**
  - Tokens are 32 bytes from the OS random source, shown once in hex at
    creation, and stored as SHA-256 hashes in keyspace 1 with a name, a
    role (`read`, `write`, `admin`) and a creation time.
  - Comparison is constant-time over the hash.
  - SHA-256 is implemented in `softshell-core` from FIPS 180-4 and checked
    against the NIST CAVP vectors. It is used for nothing else.
  - Tokens can be created and revoked offline (`softshell admin token`) or
    by an admin over the server.
- **Consequences:** no passwords to manage; a leaked token is revoked by
  name; any further crypto need goes through the exception test.
- **Alternatives considered:** passwords with PBKDF2 or Argon2 (KDF
  dependency); no authentication on loopback (rejected: other local users
  and processes are a threat).
- **Revisit when:** a user needs passwords, or another use of SHA-256 is
  proposed.

## ADR-0020 — Testing: a fault-injecting VFS, process kills, our own fuzzer

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** Crash-consistency bugs are rare, silent and catastrophic.
  Ordinary tests never crash mid-write.
- **Decision:**
  - `FaultVfs` simulates power loss (pending writes and directory
    operations dropped, last writes torn) and injected errors, and the
    crash-point test crashes at every sync point of a workload.
  - `xtask crash` kills a real child process at random moments and checks
    every acknowledged commit.
  - Every decoder has a fuzz target in our own mutation fuzzer.
  - The fuzz driver (mutations, `catch_unwind`, the hang watchdog,
    findings) lives in `softshell_core::testing`. Fuzz targets, fixture
    writers and crash workloads live in the tests of the crate whose code
    they exercise. `xtask` runs them as `cargo` subprocesses and links
    nothing beyond `core` and `codec` (ADR-0005).
  - Model-based tests compare each stateful component with a simple model.
  - Details in `docs/04-quality.md`.
- **Consequences:** test infrastructure is a large share of the code; the
  storage engine's design must route every file operation through `Vfs`.
- **Alternatives considered:** relying on process-kill tests only (misses
  power loss); `dm-log-writes` on Linux (strong, but root-only and Linux
  only; kept as an optional manual check).
- **Revisit when:** a crash-consistency bug reaches `main` that none of
  these would have caught.

## ADR-0021 — Every on-disk format and the protocol are versioned; fixtures of every version are kept

- **Status:** Accepted (user decision, 2026-11-01)
- **Date:** 2026-11-01
- **Context:** Before 1.0, formats will change often. Data written by an
  older build must never be misread by a newer one.
- **Decision:**
  - Each file type has a magic and a version; SDOC, series chunks,
    catalog entries and the protocol carry versions too
    (`docs/05-release.md`).
  - Every format change bumps the version and adds fixtures in
    `tests/fixtures/<format>/v<N>/`; old versions are read or rejected with
    `UnsupportedVersion`.
  - The manifest records the format versions a database uses, keycode's
    included: every index key depends on its byte order, so it is
    versioned like a file format even though it has no header of its own.
  - Each catalog entry records the key-layout version of the model data it
    describes, so a change to a model's key layout is detected per
    collection, index, graph or series set.
- **Consequences:** a little ceremony per format change, enforced by
  `/change-on-disk-format` and `.claude/rules/on-disk-formats.md`.
- **Alternatives considered:** no compatibility before 1.0 (simpler, but
  users lose data on upgrade).
- **Revisit when:** at 1.0, to set the long-term support window.

## ADR-0022 — One canonical NaN

- **Status:** Accepted (user decision, 2026-11-03)
- **Date:** 2026-11-03
- **Context:** The value comparator (ADR-0013) already treats every NaN
  as one value, and keycode encodes them all as `0x31`. But the SDOC spec
  kept a `Float`'s 8 bytes "exactly", and extended JSON writes any NaN as
  `{"$float": "NaN"}` and reads back the canonical NaN, so an
  export-then-import (M6.8) would change a stored NaN's bytes. NaN bits
  also differ by platform: on x86_64 with rustc 1.99.1 a computed NaN
  (`0.0 / 0.0`, `inf - inf`, `sqrt(-1)`) is `0xfff8000000000000` (`f32`
  `0xffc00000`), while aarch64's default NaN is positive, so keeping bits
  would store different bytes for the same input on each platform. The
  question came up in M0.5's design review.
- **Decision:**
  - NaN is not data: every NaN is the same value, whatever its sign and
    payload.
  - SDOC (M2.1) writes only the canonical quiet NaN, `f64`
    `0x7ff8000000000000` and `f32` `0x7fc00000` (for `F32Vec` elements),
    and rejects any other NaN bit pattern as a non-canonical encoding, so
    `encode(decode(b)) == b` still holds. Floats are otherwise kept bit
    for bit.
  - Extended JSON keeps `{"$float": "NaN"}`, and `$f32vec` elements the
    same form.
  - `Value::identical` treats every NaN as identical to every other NaN;
    round-trip properties compare with it.
- **Consequences:** the same input gives the same files on every platform,
  and export then import reproduces SDOC byte for byte. A NaN's payload
  cannot be stored; an application that encodes data in NaN payloads must
  store the bits as an `Int` or `Bytes` instead.
- **Alternatives considered:** keep NaN sign and payload bits, with an
  extended JSON form such as `{"$float": "NaN:fff8000000000000"}` — rejected:
  a computed NaN would be stored with different bytes on x86_64 and
  aarch64, and every reader of extended JSON would need the extra form.
- **Revisit when:** a user needs NaN payloads stored (for example
  NaN-boxed data imported from another system).
