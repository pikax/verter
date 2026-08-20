---
ruling_id: "GATE-SCOPE"
type: "maintainer-directive"
date: "2026-08-17"
date_source: "stated"
binds: ["program-wide gate discipline"]
source_file: "MAINTAINER-RULING-GATE-SCOPE.md"
summary: "The full gate runs once, at landing readiness, immediately before squash/fast-forward — never mid-work, never after every fix round. A test-only change or a leaf-with-no-consumers production change does not warrant a full gate (targeted tests only); a production change with real reach does. The leaf claim must be verified by an actual call-site search, never merely asserted."
supersedes: []
superseded_by:
  - ruling: "GATE-UNRESTRICTED"
    claim: "Not superseded in substance — that later ruling explicitly states it 'SHARPENS the existing one-gate-at-a-time discipline... rather than contradicting it' and restates that the landing gate is never skipped for a change with real reach. Listed here for cross-reference only; no claim in this document is actually overridden."
contradicts: []
notes: ""
---

# Maintainer standing ruling — when the full gate runs (2026-08-17)

Maintainer: Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax), designated maintainer.
Binding on every track/block orchestrator and every worker for the remainder of the program.

## Verbatim ruling

> RULE: gate should be only run just before squash or when is necessary, running full gate on
> just test updates is not good, production code must be changed and changes must require a full
> gate, if we only update a leaf function that no other systems use, we should not run gate

## Normalized rules

1. **The full gate runs ONCE, at landing readiness — immediately before the squash/fast-forward.**
   Not mid-work, not after every fix round, not per review round.
2. **A test-only change does NOT warrant a full gate.** Run the affected targeted tests
   (`cargo test -p <crate> <name>`, the package's own `pnpm`/`vitest` script) and nothing more.
3. **A production change confined to a LEAF with no consumers does NOT warrant a full gate.**
   Targeted tests only.
4. **A production change with real reach DOES warrant a full gate**, run before squash.
5. **The leaf claim must be VERIFIED, never asserted.** Before skipping the gate on leaf grounds,
   run an actual call-site/reference search and record it. "Nothing else uses this" is a claim about
   code, and this program's standing discipline is that a claim about code you did not open is a
   QUESTION, not a finding. An unverified leaf claim is not a licence to skip the gate.

## Why this matters beyond speed

The canonical gate builds two whole-workspace archives and has twice driven this machine into an
OOM-triggered reboot, which loses state across every concurrent worktree — not just the one that ran
it. Running it on test-only deltas spends that risk for no information. The MEMORY-CEILING-MANDATORY
flags (`--build-jobs 4 --test-threads 4 --memory-limit 12GiB` on this 24 GB / 8-core host) remain in
force for every run that does happen, and an aborted/timed-out/incomplete run (exit 123/124/125, or
no terminal summary) is still never a PASS.

## Relationship to existing program rules

This SHARPENS the existing "one gate at a time" discipline (implementers run targeted tests; a
dedicated agent runs the gate at landing) rather than contradicting it. It does NOT weaken the
requirement that a landing carries a genuine terminal three-surface gate verdict: rule 1 puts the
gate exactly where the program already required a verdict. What is removed is the redundant
mid-work and per-round gate runs.

Where a charter or brief demands a gate run that rules 2-3 exempt, the exemption applies and the
reason is recorded — but the LANDING gate is never skipped for a change with real reach.
