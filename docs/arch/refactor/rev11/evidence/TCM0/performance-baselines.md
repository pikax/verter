# TCM0 §10 — Performance baselines (locked before any implementation result is seen)

Scope: charter item 10. Per the charter's own ordering ("locked before any implementation result is
seen"), this file states THRESHOLDS now, from evidence already gathered in this investigation, and
explicitly excludes any number that could only come from an implementation this program has not yet
built.

## Baselines available now (measured in this investigation, not invented)

These are real numbers from a real probe against the exact candidate package on this host
(`package-lock-and-semantic-api.md` §4a) — usable as a reference point, not a cross-topology comparison
(see `topology-benchmark-plan.md`'s explicit caveat):

| Measurement | Value | Conditions |
|---|---|---|
| `API` construction (in-process spawn) | 34 ms | darwin-arm64, candidate `7.1.0-dev.20260822.1`, cold |
| First `updateSnapshot` (opens one project, one TS file) | 1037 ms | same conditions, 1-file fixture — NOT representative of a real project; recorded as a floor, not a target |
| Post-dispose stale `getSourceFile` (cache hit, no server round-trip) | 0 ms | demonstrates the client cache short-circuits entirely — see the correctness caveat in `package-lock-and-semantic-api.md` §4c; a 0ms number here is a DEFECT signature, not a performance win, and must not be misread as a target to preserve |

**Read this table as single samples, not as baselines** (added 2026-08-23 after review). Each figure above
is one observation from one run. Ten-iteration characterisation of the same measurements on the same host
shows spreads that vary by more than an order of magnitude from one committed run to the next (see the
addendum below for the exact current figures and why no single multiple is quotable here), so none of
these numbers is reproducible to better than an order of magnitude and none may be used as, or derived
into, an acceptance threshold. The addendum at the end of this file records the method, the conditions and
the withdrawal of the figures that were.

## Thresholds locked now (apply to TCM1-TCM4; not yet measured against an implementation)

Consistent with the existing repo-wide performance discipline (CLAUDE.md's warm-path invariants,
`docs/arch/optimisations/typeinfo/` program) and this investigation's own topology-benchmark-plan.md:

1. **Warm/unchanged transform must be near-zero cost.** A `HARD REQUIREMENT`, stated STRUCTURALLY and
   carrying no numeric band (see the addendum): a repeat `transform()`/`updateSnapshot()` with no content
   change must not re-do the cold path's work — no project reload, no re-parse of unchanged files — and
   must be demonstrated so over a distribution (requirement 8), not by one fast sample. The near-zero cost
   must be achieved correctly, with the snapshot-dispose asymmetry from §4c fixed rather than relied upon.
   An earlier revision attached a "single-digit-millisecond" band to this requirement; that band is
   WITHDRAWN as underived. **Nothing replaces it and nothing is owed**:
   `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q3 rules
   requirements 6-8 below the COMPLETE Scope-10 performance contract with no dedicated-machine
   absolute baseline required, so this bar has no open numeric half — see "What this means for the
   contract, settled by ruling" in the addendum.
2. **Cold-start ceiling.** STRUCTURAL — because the contract this bar belongs to is an EQUIVALENT-WORK
   contract, not because a number is pending. `G-PERF-NUMBERS` is CLOSED by Q3 with no open half: no TCM2/TCM3
   topology may regress cold start beyond the in-process reference topology except by work it can NAME
   (e.g. spawning an additional process for a daemon topology), and the comparison must be a
   distribution-vs-distribution one per requirement 8. An earlier revision cited "34ms + 1037ms" as the
   numeric ceiling; that reference is WITHDRAWN — those are single samples whose measurement varies by
   roughly an order of magnitude run to run on the only host TCM0 has.
3. **Zero process/fd leaks across 100 open/close cycles.** Hard requirement, restated from
   `topology-benchmark-plan.md`'s Cleanup row — this is a correctness bar phrased as a performance metric
   because an unbounded leak degrades performance to failure over a long-running editor session.
4. **Interactive-tier features (hover/completion/definition/signature-help) must not regress versus
   today's relay-based latency**, even though their OWNER changes (`feature-ownership-ledger.md`). The
   ownership change is architectural, not a performance mandate — TCM2/TCM3 must show the new path is at
   least as fast as the measured relay baseline for these specific rows before TCM4 may delete the relay
   code that currently serves them.
5. **The debounced background-diagnostics push (`sync_coordinator.rs`'s 300ms silence window) is an
   existing, unchanged threshold** — TCM1-TCM3 must not need to widen it to accommodate the new
   transport; if a design requires widening it, that is a rescope trigger, not a quiet adjustment.

## What is explicitly NOT locked here

No comparative topology numbers (native-in-process vs. thin-shim-over-daemon vs. Node/N-API; attach vs.
direct-client vs. managed-process) — those do not exist yet and inventing a plausible-sounding number now
would violate the charter's own ordering rule. `topology-benchmark-plan.md` is the harness those numbers
must be produced through; this file is the acceptance bar those numbers are judged against once produced.

## Addendum, 2026-08-23 (revised after review): the absolute figures are WITHDRAWN

### What the first version of this addendum got wrong

An earlier revision of this section tabled three figures — 32 ms construction, 1333 ms cold first
snapshot, 2 ms warm — attributed them to `probes/probe1-init-timing.mjs`, and hardened the warm figure
into a restatement of hard requirement 1 ("must stay in the single-digit-millisecond band the candidate
itself achieves (2 ms measured)"). Three separate review legs rejected that, correctly, on three grounds,
all of which reproduce:

1. **The figures came from a run that was never committed.** The committed transcript of that same probe,
   in the same tree, recorded 7 ms / 324 ms / 0 ms. Nothing disclosed that the tabled numbers came from a
   different run. That is the same defect class this evidence pass itself found and corrected elsewhere —
   citing a measurement with no committed record of it.
2. **The warm figure the addendum claimed to "replace a defect-derived 0 ms" with was itself 0 ms** in the
   committed transcript, so the stated improvement did not exist.
3. **The bar was not reproducible.** Repeated runs of the unmodified probe against the pinned package on
   this host class produce warm values from 0 ms to 49 ms and cold values from 27 ms to 1289 ms. The
   certified candidate fails the "single-digit-millisecond" bar derived from itself in a substantial
   fraction of runs.

**All three absolute figures are withdrawn.** Hard requirement 1 reverts to its structural form and
acquires no numeric band. The corresponding cold-start reference in requirement 2 is withdrawn on the same
grounds, and `topology-benchmark-plan.md`'s addendum is corrected to match.

### What replaces them: a method and a distribution, not a number

`probes/probe1-init-timing.mjs` now runs N independent iterations (default 10, `TCM0_PROBE_ITERATIONS`),
each with a fresh `API` and a fresh fixture, and reports **min / median / max plus every raw sample** for
construction, cold first snapshot, and warm unchanged snapshot. It asserts exactly one thing — that the
cold path completes on every iteration, i.e. that there is no hang, which is the charter's actual item-2
question and does not depend on a wall-clock threshold. Every figure it prints is labelled an observation.

A representative run is committed in `probes/transcript.md`. The material fact it records is the
**spread**: fastest-to-slowest construction, cold and warm each show a double-digit-or-larger multiple
within one ten-iteration run, and — because the probe has been re-run and re-committed several times over
the course of this investigation — the exact multiple is itself unstable: 11x/6x/30x, then 54x/6x/2x, then
5x/7x/10x (construction/cold/warm respectively) across three different committed versions of the same
file. That instability IS the finding; do not quote a specific multiple here, because it is stale by
construction the moment a fourth run is committed. Read the exact current numbers straight out of the
currently committed `probes/transcript.md` rather than off this page. It is not noise around a usable
central value; it is a measurement environment in which absolute wall-clock milliseconds carry no
acceptance signal at all.

### Conditions, stated so a future measurement can be compared

- Host: darwin-arm64 developer workstation, **contended** — other builds and agent processes were running
  concurrently throughout. This is the only hardware available to TCM0.
- Package: `typescript@7.1.0-dev.20260822.1`, native binary `@typescript/typescript-darwin-arm64`.
- Fixture: 3 authored files plus 63 default-library files; not representative of a real project.
- Method: N=10 independent iterations, fresh process state per iteration, min/median/max plus raw samples.

### What this means for the contract, settled by ruling

TCM0 **cannot** produce a defensible absolute performance threshold from the hardware available to it: an
absolute bar would need a quiet, dedicated machine and a representative fixture, and inventing a number
from a contended run — or worse, reading one off a single sample — is exactly the failure this program has
already rejected once. That observation stands as a finding.

**No absolute figure is owed.** `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q3: requirements 6-8 below **are** the complete TCM0 Scope-10 performance contract, and **no
dedicated-machine absolute baseline is required**. Independently owned correctness and lifecycle gates
remain applicable. The equivalent-work form is not a partial contract awaiting a number on better
hardware; it is the whole of it. `OPEN-GAPS.md`'s `G-PERF-NUMBERS` row is closed on that ruling, and
there is no "open numeric half" to reopen later.

### What IS locked, because it needs no number

The equivalent-work requirements below stand, and are unaffected by the withdrawal: they are fully
specified today, they fix pass/fail before any TCM1-TCM4 result exists, and they never require TCM0 to have
named a millisecond value.

6. **Equivalent-work no-regression.** For each of edit-to-hover, edit-to-completion, edit-to-definition,
   build, incremental build, watch and declaration emit, the implementing block MUST capture the current
   path's timing as its first act — before any implementation of its own exists — using the harness named
   in `topology-benchmark-plan.md`, commit that capture as evidence, and then demonstrate its own path is
   no slower on the same workload and the same host. A baseline captured after implementation work has
   begun does not satisfy this requirement, and a block that cannot produce a pre-implementation baseline
   for a metric may not claim that metric.
7. **The comparison workload is named, not chosen at measurement time.** It is the workload the block's own
   charter's Material Bounds section names. Substituting a different workload after seeing results is the
   failure mode requirement 6 exists to prevent, and it is a rescope trigger, not a quiet adjustment.
8. **Every timing claim reports a distribution over N>=10 iterations with its raw samples, and states
   whether the host was quiet or contended.** A single-sample timing is not admissible evidence for any
   TCM1-TCM4 performance claim. This requirement exists because TCM0's own first attempt violated it.

Requirements 1-5 above are unchanged EXCEPT that requirement 1 carries no numeric band and requirement 2
carries no numeric cold-start reference; both are structural bars, and they stay structural bars.
`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q3 rules requirements 6-8 the complete Scope-10 performance contract with no dedicated-machine absolute
baseline required, so no later measurement pass is owed a number to fill those two in.
