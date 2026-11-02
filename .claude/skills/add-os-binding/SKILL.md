---
name: add-os-binding
description: Use whenever adding or changing a raw OS declaration (Win32, POSIX, Darwin) that std does not provide. Declares the items in softshell-sys with citations and layout assertions, then wraps them safely in softshell-platform.
argument-hint: "<linux|macos|windows> <API or functions needed>"
---

# Add an OS binding

Request: `$ARGUMENTS`

We use no binding crates (ADR-0002, register item 1), so correctness
depends on following this procedure exactly.

## 0. Check that `std` cannot do it

Search the pinned toolchain's `std` documentation (and, for behavior, its
source) for the capability: `std::fs`, `std::os::unix::fs`,
`std::os::windows::fs`, `OpenOptionsExt`, `FileExt`, `File::lock`. If `std`
does it, use `std` and stop here. Record what you checked in the task note.

## 1. Scope it

- List the exact functions, types and constants needed. **Declare nothing
  else.**
- For each item, find the authoritative source:
  - Windows: Windows SDK headers (`um/*.h`, `shared/*.h`) or the Microsoft
    Learn reference page
  - macOS: SDK headers (`sys/fcntl.h`, …) or Apple's documentation and man
    pages
  - Linux: glibc and kernel uapi headers, man pages
- If the header isn't available in this session (e.g. Windows headers on a
  Linux machine), use the official documentation and mark those values
  "verify on <OS>".

## 2. Declare in `softshell-sys`

- File: `crates/sys/src/<os>/<module>.rs`, gated by `#[cfg(target_os = …)]`.
  Every item carries a doc comment, or a `// source:` comment naming the
  header and declaration.
- Functions: `unsafe extern "system" { ... }` for Win32;
  `unsafe extern "C" { ... }` elsewhere. Use `#[link(name = "…")]` (or
  `kind = "raw-dylib"` on Windows where no import library ships).
- Types: `#[repr(C)]`, fields in header order with exact widths:
  - `DWORD` = `u32`, `BOOL` = `i32`, `LPCWSTR` = `*const u16` on Windows
  - `int` = `i32`, `long` = `i64` on LP64 Linux and macOS
- Layout assertions for every struct, from the header's numbers:
  ```rust
  const _: () = assert!(core::mem::size_of::<Example>() == 24);
  const _: () = assert!(core::mem::align_of::<Example>() == 8);
  ```
- Constants: the header's type, and the value exactly as the header writes
  it (hex stays hex).
- Add the library to `docs/02-dependency-policy.md` ("OS libraries") and
  the items to `docs/subsystems/platform.md`.

## 3. Wrap safely in `softshell-platform`

- Expose a safe API: owned types, `&Path`/`&OsStr` instead of raw strings,
  our error types instead of codes.
- One unsafe operation per block, each preceded by `// SAFETY:` explaining
  pointer validity, lengths, lifetimes and initialization.
- Convert every failure into `VfsError`/`PlatformError` with the operation
  and path. Never ignore a return code silently.
- If the binding changes durability behavior, update
  `docs/03-platforms.md` and `FaultVfs` to match.

## 4. Test

- **Layout:** the assertions are compile-time, verified when the crate
  builds on that OS. Say which OS builds you actually ran.
- **Behavior:** extend the `Vfs` conformance suite (or a platform test) to
  cover the new behavior on that OS.
- Run `cargo xtask unsafe-audit` and
  `cargo clippy -p softshell-sys -p softshell-platform --all-targets -- -D warnings`.

## 5. Report

List:

- each declared item and its source
- what `std` was checked for, and why it was not enough
- which layout assertions compiled on which OS in this session
- which items still need verification on another OS (also add a `⏳` note
  to the roadmap task)
