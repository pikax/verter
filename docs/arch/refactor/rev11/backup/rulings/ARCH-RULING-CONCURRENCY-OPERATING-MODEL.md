---
ruling_id: "CONCURRENCY-OPERATING-MODEL"
type: "architecture-ruling"
date: "2026-08-20"
date_source: "file-mtime (in-document: 'run against program/architecture-lock at 5b899200b', no calendar date stated)"
binds: ["program-wide (ledger/concurrency operating model, not a single block)"]
source_file: "ARCH-RULING-CONCURRENCY-OPERATING-MODEL.md"
summary: "Two bounded read-only consults rule the operating model: allow up to five disjoint blocks in IMPLEMENTATION and targeted testing, but SERIALISE final certification (one full gate + one impact-bounded mandate re-attestation per landing) — because the gate cascade under concurrent certification is quadratic (N(N+1)/2) while implementation/review iteration dominates wall-clock. Recommends separating IN_PROGRESS into implementation vs certification states."
supersedes: []
superseded_by:
  - ruling: "CONCURRENCY-CEILING-AND-ROSTER"
    claim: "This ruling's own implicit ~2-block concurrent-certification cap discussion is superseded by the maintainer's explicit ceiling of 5 concurrent claude-max blocks/trains (with grok-implementer trains beyond 5); this ruling's underlying quadratic-gate-cost analysis and 'serialise certification' recommendation are not themselves contradicted, only the numeric ceiling is superseded by direct maintainer ratification."
contradicts: []
notes: "Lists prerequisites still outstanding at time of writing: a stack-window validator + composite cross-validation (AMD-001, tools did not yet exist), review-verdict-to-candidate binding (fixed in flight), and landing_equivalence_digest strengthening. Notes the practical bottleneck found independently: most of the next nine blocks lack charters, which parallelises with zero merge risk."
---

# Architecture ruling — concurrency operating model: parallel implementation, serial certification

Two bounded read-only architecture consults, run against `program/architecture-lock` at `5b899200b`.

## The decisive number: the gate cascade is quadratic

Fast-forward-only landing forces a declared order, and each block must be gated on the tree it actually
lands on. If all N concurrent blocks are individually reviewed and gated, block *k* needs *k* gates —
total **N(N+1)/2**:

| N | concurrent cascade | serial | extra gates |
|---:|---:|---:|---:|
| 2 | 3 | 2 | +1 |
| 3 | 6 | 3 | +3 |
| 4 | 10 | 4 | +6 |
| 5 | **15** | 5 | **+10** |

At N=5 gate work is **three times serial** even if review became free. The gate is the binding
constraint, not review: it is single-flight (`scripts/gate.mjs:91-96`), runs two whole-workspace archive
builds plus three test surfaces, and uses `--memory-limit 18GiB` on a locked 24 GiB host.

## The operating model to adopt

**Allow up to five disjoint blocks in IMPLEMENTATION and targeted testing; SERIALISE final
certification.** After each predecessor lands: restack the next block once, freeze it once, run ONE full
gate, obtain ONE impact-bounded mandate re-attestation.

This captures the expensive parallel work — implementation and review iteration, which dominate
wall-clock — while keeping total gate count at N, identical to serial. Concurrent *certification*
windows, if ever retained, cap at N=2 and require measured savings exceeding one whole extra gate.
There is no evidence-backed throughput case for N=3-5 concurrent certification.

## What this implies for the ledger

`IN_PROGRESS` currently conflates "being implemented" with "being certified". The operating model needs
those separated, so that up to five blocks may be in implementation while exactly one is in
certification. That is a smaller, more honest change than relaxing the single-`IN_PROGRESS` check
outright, and it preserves what that check actually protects.

## Prerequisites that remain (from the companion consult)

- A6 locks sequential depth 1 and permits a window only for D1/D2 or an explicitly ratified exception
  (`evidence/A6/stack-window-policy.toml:30,36`).
- AMD-001 makes a stack-window validator plus composite cross-validation a hard prerequisite
  (`AMD-001:43`). **Those tools do not exist.**
- Review verdicts are unbound to the candidate they were issued against — a live false-green in serial
  mode today, and fatal under any concurrency. Fix in flight.
- `landing_equivalence_digest` is checked only for SHA-256 SHAPE (`validate-program-state.mjs:961-975`);
  the artifact bytes are explicitly not verified (`evidence/A0-summary.md:97-103`). A computed
  `restack_equivalence_digest` could support a SMALL re-attestation but must not carry verdicts.
- Path disjointness must extend to SEMANTIC closure — shared registries, APIs, generated artifacts,
  build configuration, resource budgets, integration tests — not merely disjoint file paths.

## Practical bottleneck found independently

Charters, not policy. Of the next nine blocks only B5, B6, C1 (draft) and BS1 have one; D1, D2, C2, J1
and J2 have none. Charter authoring parallelises with zero merge risk and is the real critical path.
