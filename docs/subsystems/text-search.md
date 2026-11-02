# Subsystem: `softshell-text`

Full-text indexes over one keyspace per index (ADR-0016). This crate knows
documents only as `(doc_key, text fields)`; the facade feeds it from a
collection.

## Analyzer (version 1)

The analyzer turns text into terms with positions. Its version is recorded
in the index definition; changing it requires a reindex.

1. **Tokenize:** split on characters that are not `char::is_alphanumeric`.
   Tokens longer than 64 bytes are dropped.
2. **Lowercase:** `char::to_lowercase`.
3. **Fold:** map Latin letters with diacritics to ASCII (a fixed,
   hand-written table of the Latin-1 Supplement and Latin Extended-A
   letters, versioned with the analyzer).
4. **Stopwords:** drop the English stopword list (committed, versioned
   with the analyzer). Optional per index.
5. **Stem:** Porter2 (English), as described by the Snowball project.
   Optional per index.

Positions count tokens after step 1, so a dropped stopword still occupies
a position and phrase queries keep their gaps.

## Key layout (inside the index's keyspace)

| Key | Value |
| :- | :- |
| `0x01 ++ term ++ 0x00 ++ doc_key` | varint term frequency, then delta-encoded varint positions |
| `0x02 ++ term` | document frequency `u64` LE |
| `0x03 ++ doc_key` | document length in terms `u32` LE |
| `0x04 ++ "stats"` | document count, total terms |

- `doc_key` is the collection's `keycode(_id)`.
- Terms are UTF-8 with no `0x00`, so `term ++ 0x00` is an exact prefix.

## Maintenance

- **Insert:** analyze the new text; write postings, increment document
  frequencies, write the document length, update stats.
- **Update or delete:** re-analyze the **old** text (the facade passes it
  from the same transaction) to find the postings and frequencies to
  remove, then apply the new text. There is no forward index.
- All of this happens in the writing transaction.

## Queries

| Query | Meaning |
| :- | :- |
| `Term("tide")` | Documents containing the analyzed term |
| `Phrase("tide pool")` | Terms at consecutive positions |
| `Prefix("tid")` | Any term with the prefix (at most `max_prefix_terms`, default 1,000) |
| `Bool { must, should, must_not }` | Boolean combination; `should` adds score |

- The query string is analyzed with the index's analyzer.
- **Scoring:** BM25 with k1 = 1.2 and b = 0.75; IDF
  `ln(1 + (N − df + 0.5) / (df + 0.5))`. Scores are `f64`; ties are broken
  by `doc_key` so results are deterministic.
- **Top-k:** a bounded heap; results are `(doc_key, score)`. The facade
  fetches documents and can apply a document filter afterwards.
- Evaluation is document-at-a-time over the posting iterators; WAND and
  block-max pruning are in the backlog.

## Tests

- **Analyzer:** token tables, Unicode lowercasing, folding table, stopword
  and stemmer vectors. The stemmer is checked against the Snowball
  project's English vocabulary and expected output, committed with that
  project's license and notice beside them (confirm the license when the
  files are copied, M9.2).
- **Scoring model test:** random small corpora; top-k equals brute-force
  BM25 computed directly from the texts, before and after updates and
  deletes.
- **Consistency check:** document frequencies equal the count of postings
  per term, and every posting's document exists (part of `softshell check`).
