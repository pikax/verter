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

## Thresholds locked now (apply to TCM1-TCM4; not yet measured against an implementation)

Consistent with the existing repo-wide performance discipline (CLAUDE.md's warm-path invariants,
`docs/arch/optimisations/typeinfo/` program) and this investigation's own topology-benchmark-plan.md:

1. **Warm/unchanged transform must be near-zero cost.** A `HARD REQUIREMENT`, not a target range — TCM2
   fails its own acceptance if a repeat `transform()`/`updateSnapshot()` with no content change costs
   materially more than the client-side cache lookup alone (order of the 0ms figure above, MINUS the
   defect it currently represents — i.e., the same near-zero cost achieved correctly, with the snapshot
   dispose asymmetry from §4c fixed rather than relied upon).
2. **Cold-start ceiling.** No TCM2/TCM3 topology may regress cold start beyond the single-topology
   reference point above (34ms + 1037ms for the trivial fixture) by more than a small constant factor
   attributable to genuine additional work (e.g. spawning an additional process for a daemon topology);
   any larger regression must be justified in the topology-benchmark write-up, not silently accepted.
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
