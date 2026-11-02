---
paths:
  - "crates/sys/**"
  - "crates/platform/**"
---

# Rules for `unsafe` and OS code

These two crates are the only ones allowed to contain `unsafe` or platform
`cfg`s (ADR-0005).

- **Prefer `std`.** Before declaring anything, check whether `std` already
  does it on the pinned toolchain (`File::lock`, `sync_data`, `read_at`,
  `seek_read`, `OpenOptionsExt::share_mode`). A declaration we do not need
  is a liability.
- **Declarations belong in `softshell-sys`.** `softshell-platform` calls
  them. Never write `extern` blocks in `platform`.
- **To add a declaration, follow `/add-os-binding`.** In short:
  - declare only what you call, and cite the header or documentation page
    for each item;
  - assert `size_of`/`align_of` for every `#[repr(C)]` type, with values
    taken from the header or docs, never from the compiler's output for the
    struct you just wrote;
  - list the library in `docs/02-dependency-policy.md` ("OS libraries").
- **Edition 2024 syntax:** `unsafe extern "system" { ... }` for Win32,
  `unsafe extern "C" { ... }` elsewhere.
- **One unsafe operation per `unsafe` block,** each preceded by
  `// SAFETY:` explaining why it is sound: pointer validity, lengths,
  lifetimes, initialization.
- **Every `pub unsafe fn`** gets a `# Safety` doc section listing the
  caller's obligations. Prefer not to export `unsafe fn` from `platform`
  at all: its API is safe.
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
- **Before reporting done,** run `cargo xtask unsafe-audit` and
  `cargo clippy -p <crate> --all-targets -- -D warnings`.
- If a layout assertion or a test can only run on another OS, say which OS
  must verify it, add a `⏳ needs <OS>` note to the roadmap task, and do not
  claim it passed.
