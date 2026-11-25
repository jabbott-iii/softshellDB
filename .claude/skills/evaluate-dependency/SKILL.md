---
name: evaluate-dependency
description: Use before proposing any third-party crate or vendored code. Runs the project's build-vs-buy test from docs/02-dependency-policy.md and writes an evaluation for the user to decide on. Never adds the dependency.
argument-hint: "<crate-name> <what it would be used for>"
---

# Evaluate a proposed dependency

Proposal: `$ARGUMENTS`

The project rule: **build it ourselves unless a dependency is impossible to
avoid** (ADR-0002, which starts the approved set empty, and ADR-0023, which
sets the exception test). You are writing an evaluation for the user to
decide on. You never approve it yourself, and you never change any
`Cargo.toml` or `Cargo.lock`.

## Steps

1. **Restate the need.** Describe the capability concretely: inputs,
   outputs, performance, platforms, and where in the architecture it would
   live (which crate and layer). Say whether it would ship in the embedded
   library `softshelldb`, only in the server or CLI, or only in dev tooling.

2. **Try to remove the need.** List design changes that would make the
   dependency unnecessary, each with its cost:
   - a simpler format or algorithm
   - reduced scope
   - something `std` already provides on the pinned toolchain
   - an OS API we could declare in `softshell-sys`

3. **Estimate building it ourselves.**
   - Components and approximate lines of Rust.
   - Specifications to implement (with links).
   - The test strategy (vectors, property tests, fuzzing, crash tests).
   - Risks, especially security and data-loss risks.
   - Engineer-weeks, as a range, labeled as an estimate.
   - The estimate tells the user how large the work is, so they can decide
     whether to cut scope, change the design, or schedule it. It never
     makes a crate approvable on its own (ADR-0023).

4. **Apply the exception test** in `docs/02-dependency-policy.md`. For each
   reason, say "met" or "not met", with the evidence:
   1. The platform requires it.
   2. Only its owner can talk to the thing: a format, protocol, device, or
      service with no public specification.
   3. A law, a contract, or a certification names it.
   4. A homemade version would be a security hole: cryptography, or a
      protocol built on it such as TLS, that `std` and the OS don't
      provide.

   A dev-only tool passes the same test as a crate that ships. If no
   reason is met, the recommendation is "build" or "reject".

5. **Gather facts about the candidate.**
   - Use the web tools if available: crates.io, docs.rs, the repository,
     its security advisories (RustSec).
   - Collect:
     - license
     - latest version and release date
     - maintainers and activity
     - transitive dependency count (and which of them need the allowlist)
     - `unsafe` usage, and any C, C++ or assembly it builds
     - MSRV against our pinned toolchain
     - binary-size impact
     - whether it would ship in `softshelldb`, the server, or dev only
   - **Never add the crate to this workspace to inspect it.** If you
     can't verify a fact, write "unverified".

6. **Write the evaluation** to `docs/dependency-evaluations/<crate>.md`
   using the template below, and set the register row's status to
   `Proposed` (add a row if there is none).

7. **Ask the user for a decision.**
   - If they approve, follow the policy's "Exception process" step 3 as a
     separate change:
     - register row `Approved`, with an ADR via `/record-decision`
     - allowlist lines for the crate and every transitive package
     - exact version pins (`= x.y.z`)
     - one module or crate boundary of ours around it
     - a cargo feature if only the server or CLI needs it
   - If they reject it, record that in the evaluation file and set the
     register row to `Rejected`.

## Template

```markdown
# Dependency evaluation: <crate> (<version>)

- **Date:** YYYY-MM-DD
- **Requested for:** <capability>, used in <crate/layer>
- **Ships in:** softshelldb | server/CLI only (feature `<name>`) | dev only

## Need
<concrete description>

## Design changes that would avoid it
| Option | Cost | Viable? |
| :- | :- | :- |

## Build-it-ourselves estimate
<components, specs, test strategy, risks, estimate range>

## Exception test
| Reason | Met? | Evidence |
| :- | :- | :- |
| 1. The platform requires it | | |
| 2. Only its owner can talk to the thing | | |
| 3. A law, contract, or certification names it | | |
| 4. A homemade version would be a security hole | | |

## Candidate facts
| Fact | Value | Verified? |
| :- | :- | :- |
| License | | |
| Latest version / date | | |
| Maintainers / activity | | |
| Transitive deps | | |
| `unsafe` / native code | | |
| MSRV | | |
| Binary size impact | | |
| Security advisories | | |

## Recommendation
<build | adopt with conditions | reject>, with one paragraph of reasoning.

## Decision
<left blank for the user>
```
