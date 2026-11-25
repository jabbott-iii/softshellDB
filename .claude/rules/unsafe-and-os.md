---
paths:
  - "crates/sys/**"
  - "crates/platform/**"
---

# Rules for `unsafe` and OS code

These two crates are the only ones allowed to contain `unsafe` or platform
`cfg`s (ADR-0005). The "`unsafe`" section of `rust.md` in this folder and
`/add-os-binding` say how to write `unsafe` code and OS declarations. These
rules are specific to softshellDB's OS layer.

- **Errors:** convert every OS error into `VfsError` or `PlatformError` with
  the operation and the path. Never ignore a return code; if ignoring one is
  truly correct, comment why.
- **Durability semantics are a contract.** Any change to how `sync_data`,
  `sync_dir`, `rename_durable` or locking behave on an OS must update
  `docs/03-platforms.md` and keep the `Vfs` conformance suite passing on
  that OS. `FaultVfs` must keep modeling what the real OS does.
- **Wide strings (Windows):** build `Vec<u16>` with a trailing zero from
  `OsStr::encode_wide`; never pass a path that might contain an interior
  zero.
