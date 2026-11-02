# Subsystem: `softshell-documents`

Collections of documents with secondary indexes and queries, over one
keyspace per collection and one per secondary index (ADR-0012, ADR-0013,
ADR-0014).

## Collections and ids

- A collection is a named keyspace plus options, recorded in the catalog
  by the facade. This crate receives the collection's keyspace, the
  keyspace of each of its secondary indexes, and the collection's
  definition; it does not read the catalog.
- Every document has an `_id`: an `Id` generated at insert if absent, or
  a caller-supplied `Int`, `String`, `Bytes` or `Id`. Other types are
  rejected. `_id` is immutable.
- A document is an `Object`, at most `Limits::max_document_bytes` (16 MiB)
  as SDOC.

## Key layout

Inside the collection's keyspace:

| Key | Value |
| :- | :- |
| `0x01 ++ keycode(_id)` | SDOC document |
| `0x02 ++ "stats"` | Document count and total bytes (maintained in each write) |

Inside each secondary index's own keyspace:

| Key | Value |
| :- | :- |
| `0x01 ++ keycode(field values…) ++ keycode(_id)` | empty (non-unique index entry) |
| `0x02 ++ keycode(field values…)` | `keycode(_id)` (unique index entry) |

Index entries hold no copy of the document; a query reads the document by
`_id` (covered queries are in the backlog). Dropping an index drops its
keyspace; keyspace ids are never reused, so a new index never sees a
dropped index's entries (ADR-0012).

## Indexes

```rust
IndexSpec::new("by_author_date")
    .field("author", Order::Asc)
    .field("published", Order::Desc)
    .unique(false)
    .sparse(false)
```

- **Compound:** up to 8 fields, each ascending or descending.
- **Unique:** a second document with the same key fails the write with
  `DocError::DuplicateKey { index, key }`. Missing fields count as `Null`
  unless the index is sparse.
- **Sparse:** documents missing every indexed field get no entry.
- **Multikey:** when an indexed field holds an array, there is one entry
  per element. At most one field of a compound index may be an array in a
  given document; a second is `DocError::ParallelArrays`.
- **Not indexable:** `F32Vec` values (use a vector index) and keys longer
  than `Limits::max_key_bytes` after encoding: the write fails, naming the
  index.
- **Building:** creating an index on a non-empty collection scans it in
  batches of write transactions; the index is marked `building` in the
  catalog and the planner ignores it until it is `ready`. A crash during a
  build resumes it on open.
- **Maintenance:** every insert, update and delete computes the old and
  new index keys and writes only the difference, in the same transaction.

## Operations

| Operation | Notes |
| :- | :- |
| `insert(doc) -> Id` / `insert_many` | Fails on a duplicate `_id` |
| `get(id) -> Option<Value>` | Point read |
| `replace(id, doc)` | Whole-document replace; `_id` must match |
| `update(filter, update_spec, UpdateOptions { upsert, multi })` | Operators from `docs/subsystems/expressions.md` |
| `delete(filter, multi)` / `delete_by_id(id)` | |
| `find(filter) -> Find` | Builder: `.project()`, `.sort()`, `.skip()`, `.limit()`, `.after(cursor)`, `.hint(index)`, `.explain()` |
| `count(filter)` | Uses the stats key when the filter is empty |
| `aggregate(pipeline)` | `docs/subsystems/expressions.md` |

- Results stream through an iterator bound to the transaction's snapshot;
  nothing is materialized unless a sort needs it.
- A sort that no index satisfies sorts in memory, bounded by `max_sort_bytes`
  (default 32 MiB); exceeding it is an error that names the missing index.
- **Pagination:** `Find::cursor()` returns an opaque token (keycode of the
  last sort key and `_id`); `.after(token)` resumes after it. Tokens are
  stable across transactions; `skip` is supported but discouraged.

## Planner (rule-based, ADR-0014)

1. If `.hint(index)` is given, use it (error if it cannot serve the
   filter).
2. Otherwise, for each ready index, compute `index_bounds`: the longest
   equality prefix, plus one range on the next field. Prefer, in order:
   - the most equality fields;
   - then a range on the next field;
   - then an index whose order also satisfies the sort;
   - then the index with fewer fields.
3. If no index has any bound, scan the collection, unless the sort is
   satisfied by an index (scan that index instead).
4. `$or` with every branch indexable uses one index scan per branch,
   deduplicated by `_id`.
5. Every candidate document is re-checked with the full filter, so a plan
   can only be slow, never wrong.

`explain()` returns the chosen index, the bounds, the candidates
considered and why each was rejected.

## Tests

- **Model test:** random inserts, updates and deletes against a
  `Vec<Value>`; every `find` must equal a naive filter-and-sort, with and
  without each index, before and after reopen.
- **Index consistency check:** for every document, the index entries it
  should have exist, and every entry points at a document that has it
  (also part of `softshell check` for collections).
- **Planner table tests:** filter × index set → expected plan.
- **Unique and multikey edge cases:** arrays, missing fields, `Int`/`Float`
  equality (`5` and `5.0` collide in a unique index).
