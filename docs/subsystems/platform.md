# Subsystems: `softshell-sys` and `softshell-platform`

The only crates allowed `unsafe` and platform `cfg`s (ADR-0005). The
durability semantics they implement are specified in
`docs/03-platforms.md`; this document specifies the API.

## `softshell-sys`

Hand-written raw declarations of OS functions that `std` does not expose,
and nothing else. Every item cites its header or documentation page and
carries layout assertions (see `/add-os-binding`).

| OS | Items | Used for |
| :- | :- | :- |
| Windows | `MoveFileExW`, `MOVEFILE_REPLACE_EXISTING`, `MOVEFILE_WRITE_THROUGH` (kernel32) | `rename_durable` |
| Windows | `ProcessPrng` (bcryptprimitives) | OS randomness |
| Linux, macOS | — (everything comes through `std`) | — |

If M1.3 finds that `std`'s `sync_data` does not use `F_FULLFSYNC` on
macOS, `fcntl` and `F_FULLFSYNC` join this table.

## `softshell-platform`

### The `Vfs` trait

```rust
pub trait Vfs: Send + Sync + 'static {
    fn create_new(&self, path: &Path) -> Result<Box<dyn WritableFile>, VfsError>;  // fails if it exists
    fn open_append(&self, path: &Path) -> Result<Box<dyn WritableFile>, VfsError>;
    fn open_read(&self, path: &Path) -> Result<Arc<dyn RandomAccessFile>, VfsError>;
    fn remove(&self, path: &Path) -> Result<(), VfsError>;
    fn rename_durable(&self, from: &Path, to: &Path) -> Result<(), VfsError>;
    fn sync_dir(&self, dir: &Path) -> Result<(), VfsError>;                        // no-op on Windows
    fn create_dir_all(&self, dir: &Path) -> Result<(), VfsError>;
    fn list_dir(&self, dir: &Path) -> Result<Vec<OsString>, VfsError>;             // sorted
    fn file_size(&self, path: &Path) -> Result<u64, VfsError>;
    fn lock_exclusive(&self, path: &Path) -> Result<FileLock, VfsError>;           // fails at once if held
}

pub trait WritableFile: Send {
    fn append(&mut self, bytes: &[u8]) -> Result<(), VfsError>;
    fn sync_data(&mut self) -> Result<(), VfsError>;
    fn len(&self) -> u64;
}

pub trait RandomAccessFile: Send + Sync {
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), VfsError>;
    fn len(&self) -> u64;
}
```

- Writable files are append-only. The engine never overwrites a byte it
  has written (ADR-0007).
- `read_exact_at` is safe to call from many threads at once.
- `VfsError` keeps the OS error kind, the operation and the path.
- `FileLock` releases the lock on drop.

### Implementations

| Type | Where | Purpose |
| :- | :- | :- |
| `OsVfs` | `os/linux.rs`, `os/macos.rs`, `os/windows.rs` | The real filesystem, per `docs/03-platforms.md` |
| `MemVfs` | `mem.rs` | In-memory files with exact semantics and no durability model; `Database::open_in_memory` and most unit tests |
| `FaultVfs` | `fault.rs` (tests, or feature `fault-injection`) | Durable image + pending writes per file; `crash()`, tearing, error schedules, an operation counter (`docs/04-quality.md`) |

- `FaultVfs` has two directory profiles: `Unix` (create, rename and remove
  are pending until `sync_dir`) and `Ntfs` (they are durable at once, and
  `sync_dir` is a no-op). Crash tests run both.
- `OsVfs` opens every file with `FILE_SHARE_READ | FILE_SHARE_WRITE |
  FILE_SHARE_DELETE` on Windows.

### Randomness

```rust
pub fn fill_random(buf: &mut [u8]) -> Result<(), PlatformError>;
```

`/dev/urandom` on Linux and macOS, `ProcessPrng` on Windows
(`docs/03-platforms.md`). Used by the facade for tokens and as the
`IdGenerator`'s entropy.

### Tests

- The same conformance suite runs against `OsVfs` (in a temporary
  directory under `target/`), `MemVfs` and `FaultVfs` (without crashes):
  create, append, read at offsets from many threads, rename over an
  existing file, remove an open file, list order, lock contention between
  two handles.
- `FaultVfs`: a crash drops exactly the unsynced writes; tearing keeps a
  512-byte-aligned prefix; directory operations follow the profile.
- Windows: removing a file another handle has open succeeds.
- `fill_random` returns different bytes on two calls (a smoke test, not a
  randomness test).
