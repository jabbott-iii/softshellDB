# softshellDB

softshellDB is an all-in-one NoSQL database written in Rust: one embedded
engine for **key-value, documents, graph, time-series, full-text search and
vector search**, over a single crash-safe storage engine. One transaction
can insert a document, update its text and vector indexes, add a graph edge
and record a time-series point, atomically.

It is built from the ground up, with no third-party crates: its storage
engine, codecs, compression, indexes, server and test harnesses are all in
this repository. Beyond Rust's standard library, it calls only the
operating system.

## Status

**Early implementation.** The design is reviewed and the foundation crate is
under way; most crates are still empty. The design, the decisions behind it
and the task-by-task plan are in [`docs/`](docs):

- [`docs/00-overview.md`](docs/00-overview.md): goals, non-goals,
  performance budgets, risks
- [`docs/01-architecture.md`](docs/01-architecture.md): crates, layers,
  write and read paths, threading, the API sketch
- [`docs/roadmap.md`](docs/roadmap.md): milestones M0–M12, each task with
  what to do, how to accept it and how to verify it
- [`docs/decisions.md`](docs/decisions.md): architecture decision records
- [`docs/subsystems/`](docs/subsystems): one spec per subsystem

Progress is ticked task by task in [`docs/roadmap.md`](docs/roadmap.md).

## What it will look like

```rust
use softshelldb::prelude::*;

fn main() -> Result<(), softshelldb::Error> {
    let db = Database::open("./data", Options::default())?;

    db.write(|tx| {
        tx.create_collection("notes", CollectionOptions::default())?;
        tx.collection("notes")?.insert(value!({
            "title": "Tide tables",
            "tags": ["ocean"],
            "body": "Low tide is the best time to explore tide pools.",
        }))?;
        Ok(())
    })?;

    let ocean_notes = db.read(|rx| {
        rx.collection("notes")?
            .find(query!({ "tags": "ocean" }))?
            .collect_values()
    })?;
    Ok(())
}
```

This is a sketch from the design; the API will change as it is built.

## Plan at a glance

| Milestone | Delivers |
| :- | :- |
| M0 | Workspace, foundation crate, JSON, guardrail checks, fuzzer, CI |
| M1 | Durable file I/O on Linux, macOS and Windows; a fault-injecting filesystem |
| M2 | Binary formats: SDOC documents, order-preserving keys, compression, log framing |
| M3–M4 | The LSM storage engine, crash tests, compaction, caches, benchmarks |
| M5 | Serializable transactions and the embedded `Database` API |
| M6–M10 | Documents, graph, time-series, full-text search, vector search |
| M11 | Network server, protocol and client (loopback-only until TLS is decided) |
| M12 | Hardening and the first release |

## Design principles

- **Never lose an acknowledged commit.** Every record and block is
  checksummed; a commit is acknowledged as durable only after its log
  record is synced; crash tests simulate power loss at every sync point.
- **Hostile input never crashes it.** Every decoder is bounded and fuzzed.
- **One engine, many models.** Each data model is a library over the same
  transactions; the facade glues them together.
- **Build first.** No third-party crates unless an evaluation shows building
  is unreasonable and the maintainer approves it
  ([dependency policy](docs/02-dependency-policy.md)).

## Platforms

Linux (x86_64, aarch64), macOS 13+ and Windows 10+ (x86_64), on local
filesystems ([details](docs/03-platforms.md)).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Coding agents follow
[AGENTS.md](AGENTS.md); Claude Code also reads [CLAUDE.md](CLAUDE.md).

## Security

Report vulnerabilities privately, as [SECURITY.md](SECURITY.md) describes.

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
