---
paths:
  - "**/Cargo.toml"
  - "Cargo.lock"
  - ".cargo/**"
  - "xtask/**"
  - "rust-toolchain.toml"
  - "clippy.toml"
---

# Rules for manifests and dependencies

- **No third-party crates.** Do not add any non-workspace crate to
  `[dependencies]`, `[dev-dependencies]` or `[build-dependencies]` in any
  manifest. The approved set is listed in `docs/02-dependency-policy.md`,
  and it is currently empty.
- **If you think a crate is needed, stop.**
  1. Run `/evaluate-dependency <crate> <need>`.
  2. Present the evaluation to the user.
  3. Change nothing until the user approves.
- **Workspace dependencies must follow the matrix** in
  `docs/01-architecture.md` → "Dependency matrix", for normal, dev and
  build dependencies alike. If a task seems to need a new edge, stop and
  ask. Do not edit the matrix to make code compile.
- **Lints:** every member uses `[lints] workspace = true`. Never weaken
  `[workspace.lints]`, `clippy.toml` or `unsafe_code`. Never add
  `#![allow(...)]` to silence a lint without a comment explaining why the
  lint is wrong in that spot.
- **Features:** `dev` and `fault-injection` are turned on only from a
  member's `[features]` table or a dev-dependency, never from a normal
  dependency (`cargo xtask release-no-dev` checks).
- **Toolchain:** don't change the pinned toolchain in `rust-toolchain.toml`
  or `rust-version` as a side effect of another task. Toolchain bumps are
  their own task.
- **Lockfile:** `Cargo.lock` must stay free of `source = ` entries. Check
  with `cargo xtask deps`.
- **Build scripts:** `build.rs` may only emit `cargo:` directives. No
  network access, no running external tools, no code generation from
  downloaded data.
- **xtask:**
  - must never upload, publish, sign with real credentials, or read secrets;
  - must not call the network.
