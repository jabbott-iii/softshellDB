---
name: record-decision
description: Append an architecture decision record to docs/decisions.md when a design decision is made, changed, or superseded, and update the docs that reference it.
argument-hint: "<short decision title>"
---

# Record an architecture decision

Title: `$ARGUMENTS`

1. Read `docs/decisions.md`. The next number is the highest `ADR-NNNN` plus
   one, zero-padded to four digits.
2. Draft the ADR using the template at the top of that file:
   - **Status:** `Accepted (user decision, <date>)` only if the user
     explicitly made or approved the decision in this conversation.
     Otherwise `Proposed`.
   - **Date:** today's date.
   - **Context, Decision, Consequences, Alternatives considered:** keep it
     factual and specific. State numbers where they exist.
   - **Revisit when:** a concrete, observable trigger, not "if needed".
3. **If this supersedes an earlier ADR:**
   - Change only that ADR's `Status` line to `Superseded by ADR-NNNN`.
   - Do not rewrite its text. History stays intact.
4. **If the user is accepting an existing `Proposed` ADR** without changes,
   change only its `Status` line to `Accepted (user decision, <date>)`; do
   not append a new ADR.
5. Append any new ADR at the end of `docs/decisions.md`.
6. Update every doc that states the old decision: architecture, subsystem
   specs, `docs/02-dependency-policy.md`, `docs/roadmap.md` (milestone
   `Decisions` lines and affected tasks), `AGENTS.md`. Reference the ADR
   number in each place you change.
7. If the decision changes an on-disk format or the protocol, say that
   `/change-on-disk-format` must follow before any code changes.
8. Report the ADR number and title, its status, and the list of docs
   updated. If it's `Proposed`, say what the user needs to decide.
