---
paths:
  - "**/Cargo.toml"
  - "**/build.rs"
---

# Rules for manifests and build scripts

`dependencies.md` and `rust.md` in this folder cover dependencies, lints,
features, and the toolchain. These two rules are specific to softshellDB.

- **Workspace dependencies must follow the matrix** in
  `docs/01-architecture.md` → "Dependency matrix", for normal, dev and
  build dependencies alike. If a task seems to need a new edge, stop and
  ask. Do not edit the matrix to make code compile.
- **Build scripts:** `build.rs` may only emit `cargo:` directives. No
  network access, no running external tools, no code generation from
  downloaded data. No `xtask` check catches a build script that downloads
  or generates code (`docs/04-quality.md`), so review must.
