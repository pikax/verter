---
ruling_id: "B6-ROUTE-OVERHEAD-CELL-LOCK-2026-08-23"
type: "architecture-ruling"
date: "2026-08-23"
date_source: "in-document (**Date:** 2026-08-23)"
binds: ["B6", "BF1", "performance-gates.toml"]
source_file: "ARCHITECT-RULING-2026-08-23-B6-ROUTE-OVERHEAD-CELL-LOCK.md"
summary: "Resolves the BF1/B6 authority contradiction over B6_COMPILER_ROUTE_OVERHEAD: BF1 owned locking the cell before successor implementation but an accepted later disposition deferred it to B6's landing. B6 cannot supply its own threshold (ADR-016). This ruling records that contradiction and locks the cell from B5-direct product/CI budgets plus a disjoint holdout, never from B6's contaminated timing/RSS."
supersedes: []
superseded_by: []
contradicts: []
notes: "Does not amend B6's charter, does not mark B6 accepted, does not add or remove a DAG block or a ledger [[block]] row. Existing B6 timing/RSS results remain audit evidence only."
---

# Architect ruling — lock `B6_COMPILER_ROUTE_OVERHEAD` without B6 choosing the gate

**Date:** 2026-08-23
**Status:** RATIFIED — architecture ruling under delegated authority;
maintainer-authorised 2026-08-23 as the pre-B6 gate-authority repair.
**Authority:** architecture consult (codex architect, xhigh, read-only) at
`~/.claude/briefs/rev11/verify/b6-perfcell-consult.out`, plus the
maintainer's dispatch of this narrowly scoped repair block.
**Supersedes:** none. Does not reopen BF1's ACCEPTED row. Does not amend
`charters/B6.md`.

## The contradiction

Two accepted artifacts disagree about who freezes `B6_COMPILER_ROUTE_OVERHEAD`.

1. **BF1 owns the lock, and B6's charter imports it as an exit.** BF1's owned
   scope includes "exact performance cells locked before successor
   implementation" (`charters/BF1.md:20`) and exit #6 requires thresholds,
   counters, memory ceilings, fixture digests, and machine-lease policy frozen
   before BF2 work (`charters/BF1.md:36`). B6's single exit sentence requires
   "the BF1-locked route-overhead cells" (`charters/B6.md:10`). A locked cell
   fixes corpus, profiles, statistics, work assertions, and absolute/relative
   gates (`verification.md:488`).

2. **A later accepted disposition deferred the cell to B6.** 
   `evidence/framework-conformance/performance-impact.md:40` left the remaining
   six BV1/BS1/B6/C4 cells "deferred to their owning blocks' own landings" and
   named B6 as owner of this cell (`performance-impact.md:57`). BF1 was
   recorded ACCEPTED anyway (`program-state.toml` block `BF1`).

Both texts are in the accepted tree. The consult classified claim 2 as
PARTLY: the original omission belongs to BF1, but B6's still-binding charter
imports the missing lock as its own acceptance condition. That is this
ruling's subject.

## What B6 cannot do

ADR-016 prohibits post-measurement gate selection
(`decisions/ADR-016-implementation-lock-and-performance-gates.md:25`).
The repository already rejected the candidate-chooses-its-own-criteria
pattern for BF2 (`performance-gates.toml` open-row comment at the
`BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` marker). Using B6's
own observed wall or RSS to choose B6's limits would make those limits
outcome-driven. B6's existing timing/RSS results also failed the
idle-machine protocol; they are contaminated **audit evidence only** and
were not read for calibration.

Amending B6 to "measurement-only" would be a substantive post-result
weakening: the performance contract makes correctness, absolute/relative
limits, memory, and work counters conjunctive (`performance-impact.md:60`).

Landing B6 with a debt row would satisfy the DAG predecessor mechanically
and let C2, C4, F1, H1, J3, K2, and L2 proceed on an unproven performance
assumption. That is not safe.

## Resolution

A narrowly scoped, pre-B6 gate-authority repair — this block — locks the
cell. The protocol is the BF2 bootstrap shape
(`evidence/BF2/debt-BF2-perf-gate-deferred.md:42`) instantiated on the
B5 direct leg:

1. Pre-measure registration of product/CI absolute budgets, sampling, and
   the relative formula, committed **before** the first calibration run:
   `evidence/B6/cell-lock/pre-measure-registration.md`.
2. Absolute wall = 8 × A6's 2.5 ms/component product budget = 20 ms.
   Absolute RSS = 128 MiB, half of A6's 41-file host catastrophe stop.
   Neither number is a multiple of a B6 observation.
3. Neutral 30-cold-invocation calibration of
   `StandaloneCompiler::compile` (the B5 one-shot) on the registered
   eight-source corpus. Mechanical derivation of
   `no_regression_percent_max = max(3.0000, 2 × population CV)`.
4. Disjoint 30-cold-invocation holdout is the pass/fail evidence.
5. The cell is committed into repo-root `performance-gates.toml` as an
   EXTENSION under that file's SCOPE header. The Implementation Lock
   Record is updated; its digest is recomputed over the new bytes.
6. B6 is then measured against this cell on a fresh idle-machine run.
   This ruling does not accept B6.

Prepared-first, prepared-repeat, and batch arms share the B5-direct
product ceiling. They are not given a larger budget because they did not
exist at lock time.

## Failed-assumption record (governance.md §10)

```text
Failed assumption:
  BF1's ACCEPTED landing closed exit #6 for every named performance cell,
  including B6_COMPILER_ROUTE_OVERHEAD, so B6 could be judged against a
  pre-existing locked cell.

Measured/source evidence:
  performance-gates.toml on trunk c0bc6f347 contains three locked cells
  (A6_META_COMPILE_40_COLD_RUST, BF2_VUE_ORACLE_MANIFEST_GENERATE,
  BF2_SVELTE_ORACLE_MANIFEST_GENERATE). B6_COMPILER_ROUTE_OVERHEAD is
  absent. performance-impact.md:40 deferred it. BF1's ledger row is
  ACCEPTED. B6.md:10 still requires the BF1-locked cell.

Affected architecture/verification invariants:
  ADR-016 (no post-measurement gate selection); verification.md 8.1–8.3;
  B6 charter exit; BF1 exit #6; conjunctive performance contract.

Compatibility or consumer consequences:
  B6 cannot close. C2, C4, F1, H1, J3, K2, L2 stay blocked.

Alternatives:
  (a) lock the cell from an independent B5-direct baseline (this ruling);
  (b) amend B6 to measurement-only (rejected: post-result weakening);
  (c) land B6 with a debt row (rejected: unproven performance assumption
      for every B6 successor);
  (d) reopen BF1 (unnecessary once (a) lands the missing cell).

Recommended amendment:
  None to charters or the DAG. Extend performance-gates.toml and the
  Implementation Lock Record. Record this ruling.

Work that remains valid:
  BF1's two locked oracle-manifest cells; A6's cell; B5's direct compiler
  core, which is the comparable leg this lock measures.
```

## Calibration and holdout — obtained

The pre-measure registration requires a 1-minute load average below 2.00, no foreign
`cargo` / `cargo-nextest` / `rustc` / `gate.mjs` process, low-power mode off, and the
runner's control benchmark at session start and end. This host is shared with other
concurrent build agents and did not satisfy that protocol across two earlier attempts
totalling roughly four hours — 12:39–14:44 (1-minute load 2.94–40.63) and 17:07–18:02
(641 samples, load 3.84–74.34, peak 22 concurrent compilers) — both of which stopped
rather than measure under load (`evidence/B6/cell-lock/idle-protocol-log.md`). The
session below ran only after the maintainer authorised draining the host, and only once
the protocol actually held.

Compliance was enforced mechanically rather than by eye. The session runner re-checks
the load average **and** the foreign-compiler set before every measured step — both
control runs and all thirty invocations — and aborts the whole session if either fails;
the wait driver discards **both** sessions and restarts on any break, so a holdout can
never be re-drawn against a calibration already sitting on disk. The foreign-compiler
half of that per-step check was added before this window precisely because the residual
contention was bursty on a 30–40 s cadence against a ~15 s session, which a start-only
check would not have caught.

| | calibration | holdout |
|---|---:|---:|
| cold invocations | 30 | 30 |
| median wall | 0.3663 ms | 0.3651 ms |
| min / max wall | 0.3530 / 0.4381 ms | 0.3519 / 0.4296 ms |
| population CV (wall) | 5.1678% | 4.8053% |
| max peak RSS | 6.13 MiB | 6.11 MiB |
| population CV (RSS) | 0.5986% | 0.5562% |
| control drift | 0.8113% | 0.8937% |
| load at session start | 1.98 | 1.83 |

Derivation, by the formula frozen in section 7 of the registration before either session
ran: wall `max(3.0000, 2 x 5.1678) = 10.3356`, peak RSS
`max(3.0000, 2 x 0.5986) = 3.0000`. Truncated at four decimal
places, never rounded up.

Holdout verdict against section 10, all six conjunctive:

1. median wall 0.3651 ms <= the 20 ms pre-registered product budget — **pass**;
2. max peak RSS 6.11 MiB <= the 128 MiB catastrophe stop — **pass**;
3. holdout-to-calibration wall drift 0.3186% <= 10.3356% — **pass**;
4. work counters equal section 8 for the direct arm (8 compiles, 8 artifacts, 5384 payload bytes) — **pass**;
5. output digest equal to the correctness pin on every invocation of both sessions — **pass**;
6. idle-machine protocol held for the whole session, control drift inside
   `runner.max_control_drift_percent` — **pass**.

The cell is therefore locked into repo-root `performance-gates.toml` as an EXTENSION under that
file's SCOPE header, and registered as row E-2 of the new section 13 extension register in the
Implementation Lock Record, which is what the SCOPE header's "new lock record digest" requires.
Both absolute budgets were registered before any of this ran and are unchanged by it; the observed
medians sit far inside them, which is the expected shape for a product budget rather than a fit.

**This ruling still does not accept B6.** B6 is measured against this cell later, on its own
idle-machine run, and must satisfy every metric conjunctively — including the three arms
(prepared-first, prepared-repeat, batch) that do not exist on the B5 tree and that the harness
deliberately refuses rather than fabricating a baseline for.

Correctness pin (load-insensitive, taken 2026-08-23, reproduced by every invocation of both sessions):

- `output_digest` = `577f62e3ba72dcf39cd56d62285372b249752be1c1b8c3bedf02e70070446131`
- `payload_bytes` = 5384
- harness git-blob `6c69bd6e6b0f674eec20d92aff9080aad0f877ad`
- request-identity sha256 `bf427b56a4f46a151d818c52e9493fd5817da4a7ac2e74352612962ea2f4ab80`
