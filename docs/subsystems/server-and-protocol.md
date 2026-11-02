# Subsystems: `softshell-proto`, `softshell-server`, `softshell-client`

The single-node server and its protocol (ADR-0018, ADR-0019). Everything a
client can do is a call on the facade's public API.

## `softshell-proto` — wire protocol (version 1)

### Framing

```
frame = length u32 BE      # bytes after this field, ≤ max_frame_bytes (default 16 MiB + 64)
        kind   u8          # 0x01 request, 0x02 response, 0x03 error, 0x04 cursor batch
        id     u32 BE      # request id chosen by the client; echoed in the reply
        body   SDOC        # one Value
```

- A frame longer than the limit, an unknown kind, or a body that fails to
  decode closes the connection after an error frame.
- Replies may arrive in any order relative to other requests on the same
  connection only when the client pipelines; v1 servers answer in order.

### Handshake

1. The client sends `{"hello": {"protocol": [1], "client": "name/version"}}`.
2. The server replies `{"hello": {"protocol": 1, "server": "softshell/x.y.z",
   "auth": "token"}}`, or an error if no protocol version is shared.
3. The client sends `{"auth": {"token": "<hex>"}}`. Until it succeeds, any
   other request is an error and the connection is closed. Three failures
   close the connection; failures are logged with the peer address but
   never the token.

### Messages

Requests are objects with one key naming the operation:

| Group | Operations |
| :- | :- |
| Session | `ping`, `begin` (`read` or `write`), `commit`, `abort` |
| Key-value | `kv.get`, `kv.put`, `kv.delete`, `kv.range` |
| Documents | `doc.insert`, `doc.get`, `doc.find`, `doc.update`, `doc.delete`, `doc.count`, `doc.aggregate` |
| Graph | `graph.add_node`, `graph.add_edge`, `graph.neighbors`, `graph.traverse`, `graph.shortest_path`, … |
| Series | `series.write`, `series.query` |
| Search | `text.search`, `vector.search`, `search.hybrid` |
| Schema | `create_collection`, `create_index`, `create_text_index`, `create_vector_index`, `create_graph`, `create_series_set`, `drop` |
| Admin | `stats`, `check`, `checkpoint`, `token.create`, `token.revoke`, `token.list`, `shutdown` |

- Outside `begin`/`commit`, each request runs in its own transaction.
- Results larger than one batch (`batch_size`, default 1,000 values or
  4 MiB) return a cursor id; `cursor.next` and `cursor.close` page it.
  Cursors are tied to the session and its snapshot, and expire after
  `cursor_idle` (default 60 s).
- Errors are `{"code": "...", "message": "...", "details": {...}}` with
  stable codes (`not_found`, `duplicate_key`, `invalid_query`,
  `too_large`, `unauthorized`, `forbidden`, `timeout`, `busy`,
  `internal`).

## `softshell-server`

- **Listener:** `std::net::TcpListener` on the configured addresses. Any
  non-loopback address is refused at startup until TLS is approved and
  implemented (ADR-0018). There is no override.
- **Connections:** an acceptor thread hands sockets to a bounded pool of
  connection threads. Beyond `max_connections` (default 256), new
  connections get a `busy` error frame and are closed.
- **Timeouts:** read and write (30 s), idle (300 s), handshake (10 s).
- **Interactive write transactions:** at most `max_write_txn_duration`
  (default 5 s) from `begin` to `commit`; the server aborts a transaction
  that runs over and releases the writer lock. Waiting for the writer lock
  is bounded by `writer_wait` (default 5 s), after which `begin` fails with
  `busy`.
- **Authorization:** each token has a role. `read` may run read
  operations; `write` adds writes and schema changes; `admin` adds admin
  operations. A forbidden request gets `forbidden` and stays connected.
- **Limits:** frame size, query nesting (`Limits`), `$in` list size,
  traversal and series bounds, and per-request time budgets
  (`request_timeout`, default 30 s) apply to every request.
- **Configuration:** `softshell.jsonc` (unknown keys are errors):
  `data_dir`, `listen`, `max_connections`, timeouts, cache and memtable
  sizes, `log` (`level`, `file`), `slow_query_ms` (default 100).
- **Logging:** connection open and close, auth failures, slow requests
  (operation, duration, collection; never document contents or tokens).
- **Shutdown:** the admin request `shutdown` stops accepting, lets running
  requests finish for up to 10 s, aborts open transactions and closes the
  database. `std` has no signal handling, so in v1 Ctrl-C or `SIGTERM`
  ends the process at once; that is safe, because every acknowledged
  commit is already durable and the next open recovers. Graceful signal
  handling needs `sigaction` and `SetConsoleCtrlHandler` in
  `softshell-sys` (backlog).

## `softshell-client`

- `Client::connect(addr, token)` performs the handshake; methods mirror
  the facade: `client.collection("notes").find(query!(…))`, iterators that
  page cursors transparently, `client.write(|tx| …)` for interactive
  transactions.
- Blocking `std::net`; one connection per `Client`. A connection pool is
  in the backlog.
- Every response is decoded with the same `Limits` as the server uses.

## Tests

- **Proto:** every message round-trips; the fuzz target `proto_frame`.
- **Server integration:** start a server on `127.0.0.1:0` over an in-memory
  database; run each operation through the client; check auth, roles,
  limits, timeouts, cursor expiry, and that a dropped connection aborts
  its transaction and frees the writer lock.
- **Bind refusal:** configuring `0.0.0.0` or a non-loopback address fails
  startup with an error naming ADR-0018.
- **Overhead bench:** the same workload embedded and over loopback (budget
  in `docs/00-overview.md`).
