# Subsystem: `softshell-graph`

Property graphs over one keyspace per graph (ADR-0012).

## Model

- **Node:** an id (`Int`, `String`, `Bytes` or `Id`; generated `Id` if
  absent), a label (UTF-8, ≤ 128 bytes), and properties (an `Object`).
- **Edge:** an id (generated `Id`), a source node, a target node, a label,
  and properties. Edges are directed; a query may follow them in either
  direction. Parallel edges with the same label are allowed.
- Nodes may represent documents of a collection by using the document's
  `_id` as the node id; the graph does not check that the document exists.
  (The facade can enforce it; backlog.)

## Key layout (inside the graph's keyspace)

| Key | Value |
| :- | :- |
| `0x01 ++ keycode(node_id)` | SDOC `{label, props}` |
| `0x02 ++ keycode(edge_id)` | SDOC `{from, to, label, props}` |
| `0x03 ++ keycode(from) ++ keycode(label) ++ keycode(to) ++ keycode(edge_id)` | empty (out-adjacency) |
| `0x04 ++ keycode(to) ++ keycode(label) ++ keycode(from) ++ keycode(edge_id)` | empty (in-adjacency) |
| `0x05 ++ keycode(label) ++ keycode(node_id)` | empty (label index) |
| `0x06 ++ "stats"` | Node and edge counts |

Out- and in-adjacency are prefix scans: all out-edges of a node, or only
those with one label, without reading any edge record.

## Operations

| Operation | Notes |
| :- | :- |
| `add_node(id?, label, props)`, `get_node`, `update_node`, `remove_node(id, Cascade)` | `remove_node` fails if edges exist, unless `Cascade::Edges`, which removes them in the same transaction |
| `add_edge(from, to, label, props) -> EdgeId` | Both nodes must exist |
| `get_edge`, `update_edge`, `remove_edge` | |
| `neighbors(node, Direction, label?)` | Iterator of `(edge_id, node_id)` |
| `nodes_with_label(label)` | Label index scan |
| `traverse(start, Traversal)` | BFS or DFS, below |
| `shortest_path(from, to, PathSpec)` | Unweighted BFS, or Dijkstra on a numeric edge property |

### Traversals

```rust
Traversal::bfs()
    .direction(Direction::Out)
    .labels(["links_to"])
    .max_depth(3)
    .node_filter(query!({ "label": "note" }))     // expr filter on node {label, props}
    .edge_filter(query!({ "props.weight": { "$gte": 0.5 } }))
    .max_visited(10_000)
```

- Every traversal has `max_depth` (default 3, limit 16) and `max_visited`
  (default 10,000, limit 1,000,000). Reaching a limit stops the traversal
  and reports `truncated: true`; it is not an error.
- Results are yielded in BFS (or DFS) order, with the depth and the path
  of edge ids when `.with_paths()` is set.
- Dijkstra requires the weight property to be a non-negative number on
  every followed edge; a negative or missing weight is an error naming the
  edge.

## Tests

- **Model test:** random node and edge operations against adjacency maps;
  `neighbors` and traversals compared with a naive BFS, before and after
  reopen.
- **Adjacency consistency check:** every edge record has its out- and
  in-adjacency keys, and every adjacency key has its edge (part of
  `softshell check`).
- **Cascade:** removing a node with 10,000 edges in one transaction leaves
  no adjacency keys behind.
- **Limits:** a fully connected graph of 1,000 nodes stops at `max_visited`.
