---
ruling_id: "GATE-BLOCK-DEFERS-VERIFICATION"
type: "maintainer-directive"
date: "2026-08-21"
date_source: "stated"
binds: ["gate architecture", "verification infrastructure"]
source_file: "MAINTAINER-DIRECTIVE-GATE-BLOCK-DEFERS-VERIFICATION.md"
summary: "The gate-performance block implements its full change set WITHOUT running the gate, and runs the gate ONCE at the end. Each gate run costs 30+ minutes and blocks the whole machine, so per-step gate verification would serialise the very work that removes the cost. The performance changes come first; gate runs come after."
supersedes: []
superseded_by: []
contradicts: []
notes: "This is a scoped exception for the gate-performance block only, and it does not weaken the verification bar — it moves it. The block still proves each change; it uses targeted tests and the gate's own self-test suites (gate-selftest.mjs, gate-memory-selftest.mjs) during implementation, and a single full gate at the end. The measured baseline that justified this: archive [debug] 443s + SURFACE 1 726s + SURFACE 2 196s + archive [shipped-cfg] 254s + SURFACE 3 376s = ~1995s (~33 min) of measured phases per run."
---

# Maintainer Directive — the gate block defers gate verification to the end

**Status:** ADOPTED by the maintainer, 2026-08-21.

The gate-performance block implements **all** of its changes without running the
full gate, and runs the gate **once, at the end**.

## Why

A full gate run costs **30+ minutes** and holds the machine lock, so it stalls
every other block. Verifying each step of the gate-performance work with a gate
run would serialise the entire program behind the very cost that work exists to
remove. The performance changes must land first; gate runs come after.

Measured baseline (trunk a53447b19, first fully-instrumented run):

| phase | wall |
|---|---|
| archive [debug] build | 443s |
| SURFACE 1 | 726s |
| SURFACE 2 | 196s |
| archive [shipped-cfg] build | 254s |
| SURFACE 3 | 376s |
| **total** | **~1995s (~33 min)** |

## What this does NOT mean

It is not a licence to skip verification — it MOVES verification, it does not
remove it. During implementation the block still proves every change using:

- targeted `cargo test` / `cargo nextest` runs scoped to what changed,
- the gate's own self-test suites, `scripts/gate-selftest.mjs` and
  `scripts/gate-memory-selftest.mjs`, which exercise the runner without
  executing the test universe,
- the seeded-defect proofs the SINGLE-TEST-UNIVERSE directive requires before
  any surface may be deleted.

The single end-of-block gate run is the acceptance evidence, and it must pass an
explicit `--memory-limit`.

## Scope

This exception applies to the **gate-performance block only**. Every other block
remains under the standing no-gate hold until the gate fix lands, and then
returns to normal gate verification.
