# Subsystems: `softshell-cli`, `apps/tidepool`, `xtask`, `softshell-bench`

## `softshell-cli` — the `softshell` binary

| Command | Does |
| :- | :- |
| `softshell open <dir>` | A REPL on an embedded database: one JSONC request per line, in the protocol's message format, results pretty-printed |
| `softshell connect <addr> --token-file <path>` | The same REPL over the server |
| `softshell serve --config <softshell.jsonc>` | Runs the server |
| `softshell import <dir> <collection> <file.jsonl>` | Inserts JSON Lines (extended JSON), in batches of 1,000 per transaction; reports line numbers of failures |
| `softshell export <dir> <collection> [--out file.jsonl]` | Writes every document as extended JSON Lines, in `_id` order |
| `softshell check <dir>` | Runs `db.check()` read-only; exit status 0 if clean, 1 if any problem |
| `softshell dump wal\|sst\|manifest <file>` | Decodes a file for debugging (keys shown as escaped bytes and, where possible, decoded keycode) |
| `softshell checkpoint <dir> <dest>` | A consistent copy |
| `softshell admin token create <dir> --name N --role R` | Prints a new token once; stores its hash |
| `softshell admin token revoke <dir> --name N` / `list` | |
| `softshell upgrade <dir>` | From 1.0: rewrites to the current format versions (`docs/05-release.md`) |

- Tokens are read from a file or the `SOFTSHELL_TOKEN` environment
  variable, never from the command line (it would land in shell history
  and process lists).
- Exit status: 0 on success, 1 on a failed operation or check, 2 on a
  command line not understood.
- The REPL reads plain lines from stdin (dependency register item 27).

## `apps/tidepool` — the reference application

A command-line notes app that drives the requirements: each data-model
milestone ends with a Tidepool task that uses the new model. It runs
embedded only.

| Feature | Model | Milestone |
| :- | :- | :- |
| Notes with titles, tags and bodies; list, show, edit, delete; filter by tag | Documents | M6 |
| `[[links]]` between notes; backlinks; "notes within 2 links" | Graph | M7 |
| Edit activity per note and per tag; a weekly chart in the terminal | Time-series | M8 |
| `tidepool search "tide pools"` with ranked results | Full-text | M9 |
| `tidepool similar <note>` and hybrid search | Vector | M10 |

- Embeddings come from a deterministic toy embedder in Tidepool (hashed
  character trigrams into 256 dimensions). It is not a language model; it
  exists so the vector path runs end to end with no download.
- `tidepool seed --notes N --seed S` generates a synthetic corpus, used by
  tests and the M12 soak.

## `xtask`

Build automation, run as `cargo xtask <command>`. It never uploads,
publishes, signs with real credentials, reads secrets or calls the network.

| Command | Purpose | Spec |
| :- | :- | :- |
| `ci` | The full pull-request gate | `docs/04-quality.md` |
| `deps`, `layers`, `unsafe-audit`, `headers`, `workflows`, `release-no-dev`, `features` | Guardrails | `docs/04-quality.md` |
| `fuzz-smoke [--secs N] [--target T]` | Mutation fuzzer over every decoder | `docs/04-quality.md` |
| `crash [--minutes N] [--seed S] [--exhaustive]` | Process-kill and exhaustive crash-point runs | `docs/04-quality.md` |
| `fixtures --new <format>` | Writes fixtures of the current format version | `docs/04-quality.md` |
| `bench [filter]` | Runs `softshell-bench` in release mode | below |
| `soak --minutes N` | Long mixed workload, reporting memory, file handles, compaction debt | M12.2 |
| `crates` | Publishing metadata checks | `docs/05-release.md` |
| `dist [--out DIR]` | Builds this machine's release archive and `SHA256SUMS` | `docs/05-release.md` |

## `softshell-bench`

- Our own harness: warm-up, samples, median/p95/p99, throughput, JSON
  output; baselines in `bench/baselines/<machine>.json`
  (`docs/04-quality.md`).
- Datasets are generated from seeds: uniform and Zipfian keys, documents
  with skewed tags, power-law graphs, regular and jittered series, a
  synthetic text corpus with a Zipfian vocabulary, Gaussian-cluster
  vectors.
- Each budget in `docs/00-overview.md` has a named bench that measures it.
