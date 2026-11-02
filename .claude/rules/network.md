---
paths:
  - "crates/proto/**"
  - "crates/server/**"
  - "crates/client/**"
  - "crates/cli/**"
---

# Rules for the server, protocol, client and CLI

Clients are untrusted until authenticated, and authenticated clients are
still bounded (ADR-0018, ADR-0019).

- **Loopback only, no bypass.** The server refuses any non-loopback listen
  address until TLS is approved (M11.6) and configured. Do not add a flag,
  environment variable or config key that skips this check.
- **Authenticate first.** Before a successful `auth`, only `hello` and
  `auth` are accepted. Compare token hashes in constant time. Three failures
  close the connection.
- **Never log or echo secrets.** Tokens never appear in logs, errors,
  `Debug` output, panic messages or test fixtures. A `Token` type's `Debug`
  prints a redacted form.
- **CLI secrets:** tokens come from a file or `SOFTSHELL_TOKEN`, never from
  a command-line argument.
- **Every limit applies to every request:** frame size, `Limits` on decoded
  bodies, batch sizes, `$in` lengths, traversal and series bounds, request
  timeout, interactive transaction duration, writer-lock wait. A new
  operation states which limits bound it.
- **Authorization per operation.** Each message kind declares the role it
  needs; a new message without a declared role must not compile (use an
  exhaustive `match`).
- **Release what you hold.** A closed, timed-out or failed connection aborts
  its transaction and frees the writer lock and its cursors. Tests prove it
  for every new stateful operation.
- **Errors to clients** use the stable codes in
  `docs/subsystems/server-and-protocol.md` and never include internal
  paths, other users' data, or stack traces.
- **Protocol changes** follow `/change-on-disk-format` (the protocol is a
  format): additive changes keep the version; anything else bumps it.
- **No async runtime and no TLS crate** unless the dependency policy
  approves one.
