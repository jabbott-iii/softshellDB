# 03 — Platforms and durability

Everything in this document is implemented in `softshell-platform`, with
raw declarations in `softshell-sys`. No other crate may depend on what an
OS does with a file (ADR-0005, ADR-0006).

## Targets

| Target | Tier | CI | Crash tests |
| :- | :- | :- | :- |
| Linux x86_64 (glibc 2.31+) | 1 | build, test, fuzz smoke, crash | `FaultVfs` and process kill |
| Linux aarch64 (glibc 2.31+) | 1 | build, test | `FaultVfs` and process kill |
| macOS 13+ aarch64 (Apple Silicon) | 2 | build, test | `FaultVfs`; process kill before release |
| macOS 13+ x86_64 | 2 | build, test (when a runner exists) | `FaultVfs` |
| Windows 10+ x86_64 (MSVC) | 2 | build, test | `FaultVfs`; process kill before release |

- **Tier 1:** builds and tests run on every pull request, and Linux x86_64
  also runs the time-boxed fuzz and process-kill runs; a release is
  blocked by any failure.
- **Tier 2:** builds and unit/integration tests run on every pull request;
  the long crash runs happen before each release, on that OS.
- Linux lands first in every milestone (it is where CI and agents run),
  then macOS, then Windows. A task whose remaining work needs another OS
  stays unticked with a `⏳ needs <OS>` note (`docs/roadmap.md`).
- Not supported: 32-bit targets, big-endian targets, WASM, mobile, musl
  (until someone needs it; it should work but is not tested).

## Filesystems

| Supported | Not supported |
| :- | :- |
| ext4, XFS, btrfs (Linux); APFS (macOS); NTFS (Windows) | NFS, SMB/CIFS, sshfs and other network filesystems; FAT/exFAT |

Network filesystems break exclusive locking and often lie about sync. The
engine cannot detect them reliably, so this is documented, not enforced:
opening a database on one is unsupported and may lose data.

## The durability contract

The storage engine needs exactly these operations, and each has a
specified meaning on every OS:

| Operation | Meaning | Used for |
| :- | :- | :- |
| `append(file, bytes)` | Bytes are written at the end; not durable until `sync` | WAL, manifest, SST writing |
| `read_at(file, offset, buf)` | Positional read, no shared cursor; safe from many threads | SST blocks |
| `sync_data(file)` | File contents written so far, and the size needed to read them, are durable on stable storage, not just in a drive cache | WAL commits, finished SSTs, manifest records |
| `sync_dir(dir)` | A file created, renamed or removed in `dir` is durable | After creating an SST, a WAL segment or a manifest |
| `rename_durable(from, to)` | Atomic replace of `to`; durable when it returns | `CURRENT`, `IDENTITY` |
| `lock_exclusive(file)` | Fails at once if another process holds it | `LOCK` |
| `remove(file)` | Not durable until `sync_dir` | Obsolete SSTs and WAL segments |

The engine's ordering rules (written, then synced, then referenced) are in
`docs/subsystems/storage-engine.md` and `.claude/rules/storage-engine.md`.

### Linux

- `sync_data` → `File::sync_data` → `fdatasync(2)`. On ext4 and XFS this
  flushes the drive's volatile cache with a FLUSH command when the drive
  reports one.
- `sync_dir` → open the directory read-only with `File::open`, then
  `File::sync_all` (`fsync(2)` on a directory descriptor).
- `rename_durable` → `std::fs::rename` (`rename(2)`, atomic), then
  `sync_dir` of the parent.
- `lock_exclusive` → `File::try_lock` (stable since Rust 1.89).
- `read_at` → `std::os::unix::fs::FileExt::read_at` (`pread(2)`).
- A failed `fsync` is fatal for that file: after a sync error, Linux may
  have dropped the dirty pages, so a retry can report success for data that
  is gone. The engine treats any sync error as a background error, stops
  accepting writes, and requires a reopen (ADR-0011).

### macOS

- `fsync(2)` on macOS does **not** flush the drive cache. Durability needs
  `fcntl(F_FULLFSYNC)`.
- `std`'s `sync_all` and `sync_data` call `fcntl(fd, F_FULLFSYNC)` on Apple
  targets. **Verify this in the pinned toolchain's `std` source in M1.3**
  before relying on it; if it is not so, declare `fcntl` in `softshell-sys`.
- `sync_dir` → `File::open` on the directory, then `sync_all`.
- `rename_durable` → `rename(2)`, then `sync_dir`.
- `F_FULLFSYNC` is slow (milliseconds on many SSDs); group commit is what
  makes `Sync::Always` usable here.

### Windows

- `sync_data` and `sync_all` → `FlushFileBuffers`.
- A directory cannot be synced like a file. Creating and removing files is
  made durable by the NTFS journal; the engine does not call `sync_dir` on
  Windows, and the `Vfs` implementation documents that it is a no-op.
- `rename_durable` → `MoveFileExW(from, to, MOVEFILE_REPLACE_EXISTING |
  MOVEFILE_WRITE_THROUGH)` from `softshell-sys`. `std::fs::rename` does
  not ask for write-through, so it is not used for durable renames.
- Every file is opened with `FILE_SHARE_READ | FILE_SHARE_WRITE |
  FILE_SHARE_DELETE` (`OpenOptionsExt::share_mode`), so that an SST can be
  deleted while a reader still holds it, as on Unix.
- `read_at` → `std::os::windows::fs::FileExt::seek_read`. It moves the
  handle's file pointer as a side effect; the engine never uses the file
  pointer, so this is harmless.
- `lock_exclusive` → `File::try_lock` (`LockFileEx`).
- Mark `MoveFileExW` and `ProcessPrng` layouts and link names "verify on
  Windows" until a Windows CI run builds and tests them.

## Randomness

| OS | Source | Notes |
| :- | :- | :- |
| Linux, macOS | Read 32 bytes from `/dev/urandom` through `std::fs` per refill | Never blocks after boot on supported kernels |
| Windows | `ProcessPrng` from `bcryptprimitives.dll` | The API `std` itself uses for `RandomState` |

Randomness is used for API tokens and as the random part of `Id`s. Ids take
their entropy from a buffered source refilled from the OS; tests inject a
seeded PCG32 instead.

## Paths and names

- The database directory path is passed through as an `OsStr`; the engine
  never lowercases or normalizes it.
- File names inside the directory are ASCII and fixed-width
  (`000123.sst`), so they are valid on every supported filesystem.
- Collection, index, graph and series names are UTF-8, 1–128 bytes, with
  no control characters. They are stored in the catalog, never used as
  file names.

## Clocks

- `std::time::SystemTime` for wall-clock timestamps (ids, TTL, retention).
  A `Clock` trait is injected everywhere wall time matters, so tests are
  deterministic.
- `std::time::Instant` for durations (timeouts, stall timing, latencies).
- A wall clock that goes backwards does not reorder ids from one
  generator (the facade keeps one per open database): it never issues a
  timestamp smaller than the last one it issued.

## Per-OS verification checklist

Run before each release on each OS (the M12.3 task automates what it can):

1. `cargo xtask ci` passes.
2. `cargo xtask crash --minutes 10` passes (process kill with reopen and
   history check).
3. Open a database created on another OS (fixtures in `tests/fixtures/`).
4. Two processes opening one database: the second fails with
   `Error::Locked`.
5. Delete an SST while a reader holds it (Windows share mode).
