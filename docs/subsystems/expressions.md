# Subsystem: `softshell-expr`

Query documents, parsed into a typed AST and evaluated over `Value`s
(ADR-0014). Used by `documents` (filters, projections, sorts, updates,
aggregations), `graph` (property filters) and `series` (tag filters), and,
through the facade, by the server.

## Paths

- A path is a dot-separated list of field names, with numeric segments
  indexing arrays: `author.name`, `tags.0`.
- Evaluating a path over an array without an index visits every element
  (as MongoDB does): `{"tags": "rust"}` matches `{"tags": ["go", "rust"]}`.
- Paths are parsed once into `Path(Vec<Segment>)`; at most 32 segments.

## Filters

| Operator | Meaning |
| :- | :- |
| `{"f": v}`, `$eq` | Equal under the value order (`5` equals `5.0`) |
| `$ne` | Not equal (also matches a missing field) |
| `$gt`, `$gte`, `$lt`, `$lte` | Ordered comparison, **within one type class** (numbers with numbers, strings with strings): `{"$gt": 5}` never matches a string |
| `$in`, `$nin` | Membership in a list of at most 10,000 values |
| `$exists` | Field present (`true`) or absent (`false`) |
| `$type` | Field's type name: `"null"`, `"bool"`, `"number"`, `"int"`, `"float"`, `"string"`, `"bytes"`, `"timestamp"`, `"id"`, `"array"`, `"object"`, `"f32vec"` |
| `$prefix` | String starts with the given string (index-friendly) |
| `$size` | Array length |
| `$all` | Array contains every listed value |
| `$elemMatch` | An array element matches a nested filter |
| `$and`, `$or`, `$nor` | Lists of filters |
| `$not` | Negates an operator expression |

- Unknown operators, a `$` key in the wrong place, or a wrong argument
  type are parse errors naming the path and operator.
- `$regex` is in the backlog (dependency register item 19).

## Projections, sorts and updates

- **Projection:** `{"title": 1, "author.name": 1}` (include) or
  `{"body": 0}` (exclude), not mixed, except `_id`, which is included
  unless `{"_id": 0}`.
- **Sort:** an ordered list of `(path, Asc | Desc)`; a missing field sorts
  as `Null`. Ties are broken by `_id` so results are deterministic.
- **Update operators:** `$set`, `$unset`, `$inc` (numbers only; `Int`
  overflow is an error, never a wrap), `$push` (with optional `$each`),
  `$pull` (by value or filter), `$addToSet`, `$rename`. An update may not
  change `_id`, and two operators may not touch the same path.

## Aggregation (M6.7)

A small pipeline: `$match` (filter), `$group` (key paths; accumulators
`$count`, `$sum`, `$min`, `$max`, `$avg`, `$first`, `$last`), `$sort`,
`$limit`, `$project`. Groups are held in memory, bounded by
`max_groups` (default 100,000); exceeding it is an error, not a spill.

## Evaluation

- `Filter::matches(&Value) -> bool` reads fields by path without copying
  the document; over SDOC bytes it skips fields it does not need.
- The evaluator never allocates per document for comparisons of scalars.
- `Filter::index_bounds(index_fields) -> Option<Bounds>` extracts the
  equality prefix and range that the planner uses
  (`docs/subsystems/documents.md`).

## Tests

- Parser: every operator, every error, and the fuzz target `query_doc`.
- Evaluator: a table of documents × filters with expected results,
  including arrays, missing fields and mixed types.
- Property: `index_bounds` never excludes a document that `matches`
  accepts (soundness), checked by generating documents and filters.
