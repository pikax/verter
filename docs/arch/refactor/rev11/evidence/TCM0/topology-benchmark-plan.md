# TCM0 §7 — Topology benchmarks (plan, not results)

Scope: charter item 7. TCM0 is read-only and runs no build/production benchmark — this file locks the
PLAN (candidates, metrics, harness shape) so TCM2/TCM3 measure against a pre-agreed rubric instead of
picking favorable metrics after the fact, matching the charter's "performance baselines locked before
any implementation result is seen" discipline.

## Why both planes need a topology decision

Confirmed by direct inspection (`package-lock-and-semantic-api.md` §2): the candidate package no longer
ships an in-process JS compiler — every topology candidate for BOTH planes ultimately spawns or attaches
to the same native Go binary. The decision is about **process count and attachment model**, not about
choosing between a "heavy native" and "light JS" engine — that choice no longer exists.

## Projection-plane candidates

1. **Native mapper with in-process compiler** — Verter's own Rust compiler runs the transform in the
   SAME process as the content-mapper's JSON-RPC server (no IPC for the transform itself; only the
   mapper↔native-tsgo JSON-RPC channel exists as IPC).
2. **Thin mapper over a shared native daemon** — the content-mapper process is a thin JSON-RPC shim that
   forwards to Verter's existing long-running native daemon (the same process backing the LSP today).
3. **Node/N-API only if competitive** — a Node-hosted mapper using `@verter/native`'s existing N-API
   bindings, considered only if (1)/(2) do not clear the bar (per the charter's explicit ordering).

## Semantic-plane candidates

1. **Attach to the editor-owned API session** — `API.fromLSPConnection(...)` (confirmed present in the
   candidate, `dist/api/sync/api.d.ts:44-48` — "connecting to an API pipe provided by an LSP server via
   `custom/initializeAPISession`"). **Not yet probed for the session-attach hang class** — recorded as an
   open gap in `package-lock-and-semantic-api.md` §4a; closing that gap is a TCM3 precondition, not
   optional polish.
2. **Direct native client** — Verter spawns its own `API` instance (the exact path already probed live in
   `package-lock-and-semantic-api.md` §4).
3. **Managed process for non-editor hosts** — a Verter-managed `tsc --api` process for hosts with no
   editor-owned session at all (CLI/CI usage, mirroring today's `verter-tsc` binary).

## Metrics (locked now, measured later)

Per the charter's exact list — cold start, first/warm/unchanged transform, rapid edits, CPU,
allocations, RSS and peak, process count, IPC bytes, open/close, consolidation, crash isolation, cleanup
— with the concrete harness each metric must use, so a later block cannot silently narrow the metric to
something more flattering:

| Metric | Harness | Non-dominance criterion |
|---|---|---|
| Cold start | time from process spawn to first successful `initialize` response, candidate package, this exact darwin-arm64 host. **Report a distribution over N>=10 iterations with raw samples** (`performance-baselines.md` requirement 8): the single-sample figures once recorded here (34ms + 1037ms) vary by roughly an order of magnitude run to run on this host and are NOT a baseline | lower is better on the MEDIAN, and a difference inside the observed spread is not a difference; a candidate slower on cold start must win on warm/rapid-edit to be non-dominated |
| First transform | time from `transform()`/first snapshot to first usable diagnostics for a real multi-file fixture (not the trivial 1-file probe used for the defect-reproduction probes) | as above |
| Warm/unchanged transform | repeat `transform()`/`updateSnapshot()` with no content change — must be near-zero cost; the `SourceFileCache` ref-counting mechanism (`package-lock-and-semantic-api.md` §4c) is precisely the thing this metric holds accountable | near-zero warm cost is a HARD requirement, not a nice-to-have — a topology that cannot achieve it fails outright regardless of other metrics |
| Rapid edits | a scripted burst of N edits at editor-realistic cadence (~1 edit/100ms) against one open file | must not regress interactive-tier features (hover/completion) in `feature-ownership-ledger.md`'s `TypeScriptLspDirect`/interactive-tier rows |
| CPU | wall-clock CPU-seconds per fixture run, both planes | lower is better |
| Allocations | peak + total bytes allocated (Rust side: existing allocator-canary harness per CLAUDE.md's `allocator_canaries`; Go/native side: `pprof` heap profile via the API's own `startCPUProfile`/`saveHeapProfile` — confirmed present, `dist/api/sync/api.d.ts:88-90`) | lower is better |
| RSS and peak | process RSS sampled at 100ms intervals across the fixture run | lower is better |
| Process count | exact count of live OS processes per topology candidate, steady-state and during a rapid-edit burst | fewer is better, but not at the cost of crash isolation (below) |
| IPC bytes | bytes crossing the mapper↔native-tsgo boundary and (for semantic-plane candidates 1/2) the client↔`API`-server boundary, per transform/query | lower is better |
| Open/close | latency of `openProject`/`closeProject` (mapper) and `updateSnapshot{openProjects,closeProjects}`/`release` (semantic API) | lower is better |
| Consolidation | can one process serve N projects/N editor windows — the upstream design text (microsoft/typescript-go#4712, fetched live during this investigation — the same PR cited for the four-step lifecycle in `package-lock-and-semantic-api.md` §3, though this specific sentence is not itself quoted there): "TypeScript deduplicates processes by resolved package name/version, allowing one process to serve multiple projects with isolated state per project handle" states an INTENDED dedup behavior; this was not independently verified against the binary/package bytes the way the `internal/contentmapper` symbol evidence was, so it sets a bar to CONFIRM during TCM2, not an already-proven baseline | must not regress below the upstream-DESCRIBED (not yet independently verified) dedup behavior — TCM2 must confirm this behavior actually holds in the candidate before treating it as a floor |
| Crash isolation | inject a panic/OOM-kill in the mapper/API process mid-request; measure whether other open projects/sessions survive | must not cascade-fail unrelated projects |
| Cleanup | verify zero leaked processes/fds after `api.close()`/`closeProject()` across 100 open/close cycles | zero leaks is a HARD requirement |

## Selection rule

"Select the non-dominated topology on evidence" (charter's own words) — a candidate is eliminated only
when another candidate is at least as good on every metric above and strictly better on at least one. Two
non-dominated candidates surviving after measurement is a legitimate TCM2/TCM3 outcome (record both, pick
by a stated secondary criterion), not a defect in this plan.

## What TCM0 explicitly does not do here

No benchmark in this file has been RUN. The one live number in the table above (cold-start figures from
`package-lock-and-semantic-api.md` §4a) is a single-topology probe result already gathered in this
investigation, reused here as a reference point — it is not a comparison across candidates and must not
be read as one.

## Addendum, 2026-08-23: why TCM0 cannot select a topology, and what it selects instead (`G-TOPOLOGY`)

**Ratified 2026-08-24.** The reallocation this addendum proposed is no longer a proposal.
`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q2 ratifies the transfer: TCM0 owns candidate screening, the survivor sets, the metrics, the harness, the
baseline method and the selection rule; TCM2 owns evidence-based projection-plane topology selection and
TCM3 owns evidence-based semantic-plane topology selection, **each as a blocking exit of its own block**.
Comparative topology numbers are therefore not a TCM0 acceptance precondition, and the transfer is not
pending a maintainer ratification act — the ruling IS that act. Everything below stands as the evidence
and the measurement contract the transfer rests on; only its "PROPOSED"/"pending amendment" framing is
superseded.

`OPEN-GAPS.md`'s `G-TOPOLOGY` row records that no comparative numbers exist across the candidate
topologies, and that TCM0's acceptance requires the steering's "select the non-dominated topology based on
evidence" (charter item 7). Those two statements cannot both be satisfied, and the reason is structural
rather than a shortfall of effort.

### The requirement as written is an unsatisfiable cycle

The projection-plane candidates are *native mapper with in-process compiler*, *thin mapper over a shared
native daemon*, and *Node/N-API*. The semantic-plane candidates are *attach to the editor-owned API
session*, *direct native client*, and *managed process for non-editor hosts*. **Four of those six do not
exist.** They cannot be measured without being built, and building them is precisely TCM2's and TCM3's
owned scope.

`program-dag.toml` then closes the loop: `TCM2.predecessors = ["TCM0", "TCM1"]` and
`TCM3.predecessors = ["TCM0", "TCM1"]`. Neither block may be dispatched until TCM0 is ACCEPTED. So
requiring comparative topology numbers before TCM0 leaves LOCKED requires TCM2/TCM3 output before TCM2/TCM3
may start — the same unsatisfiable shape already identified and rejected for the rows #25-26 gate
(`feature-ownership-ledger.md`'s "Correction, 2026-08-23", and `OPEN-GAPS.md`'s
`G-TCM0-ACCEPTANCE-ROWS-25-26`, which reasons identically about TCM3).

### What TCM0 can and does select on evidence

Four selections are decidable from evidence TCM0 holds, and they are recorded as selections rather than
deferrals. Two are new since the review that flagged this row.

1. **Node/N-API is eliminated from the projection plane — on a STRUCTURAL argument, and the steering
   itself sanctions eliminating it this way.** Its own wording is *"Node/N-API topology only if it remains
   competitive **after initial evidence**"* (`rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md:862`) —
   i.e. this candidate is explicitly conditional on initial evidence rather than entitled to a full
   benchmark. The initial evidence is that a Node/N-API projection plane is strictly the native path plus
   two additional layers (a Node runtime, and an N-API crossing per `transform()`) in front of a mapper
   whose work is entirely native Rust, for no capability the native path lacks. A topology that can only
   add cost to the one it is measured against cannot "remain competitive". Struck.
2. **The three semantic-plane candidates share ONE engine**, so no comparison between them can be about
   engine weight. `package-lock-and-semantic-api.md` §2 establishes the candidate ships no
   `tsserver.js`/`typescript.js`/`services.js` — compiler, checker and language service are all inside the
   Go binary, and the npm package is a thin client. Any benchmark between attach / direct-client /
   managed-process measures IPC and session lifetime only.
3. **NEW: semantic-plane candidate 1 (attach to the editor-owned session) is PROVEN VIABLE**, not merely
   plausible. `probes/probe8-lsp-session-attach.mjs` obtains an API pipe via `custom/initializeAPISession`,
   attaches a second client, sees all 64 program files, and answers a real `Checker` query over that pipe —
   with no session-attach hang (`package-lock-and-semantic-api.md` §4a-attach). This matters for selection
   because the steering states a PREFERENCE that turns on exactly this question: *"Avoid starting a second
   TypeScript project graph when an editor-owned current graph can safely and correctly be reused"*
   (`:894`). TCM0 can now say the editor-owned graph **is** reusable, which makes candidate 1 the preferred
   semantic-plane topology unless a later measurement shows it dominated.
4. **NEW: the attach topology forces the ASYNC client.** The sync client refuses socket connections
   outright (*"Socket connections are not yet supported in the sync client"*,
   `dist/api/sync/client.js:11`). This is a capability constraint on candidate 1 that no benchmark would
   have surfaced, and it is an input to the selection rather than a disqualifier.

### What still cannot be measured, and why that is structural

Projection-plane candidates 1 and 2 (native mapper with in-process compiler; thin mapper over a shared
native daemon) and semantic-plane candidates 2 and 3 (direct native client; managed process) **do not
exist**. Building them is TCM2's and TCM3's owned scope. Measuring the full metric list above — cold
startup, first/warm/unchanged transform, rapid edits, CPU, allocations, RSS, process count, IPC bytes,
open/close, consolidation, crash isolation, cleanup, packaging, security boundaries — against artifacts
nobody has written is not a shortfall of effort.

`performance-baselines.md`'s addendum adds a second, independent reason the full comparison cannot be
completed here even for what exists: this host's wall-clock measurements show double-digit-or-larger
spreads within a single ten-iteration run, and the exact multiple itself drifts by an order of magnitude
between committed re-runs (see that addendum for the current figures), so a cross-topology comparison taken
on it would not discriminate between topologies even if all six existed.

### Disposition

TCM0 records: the candidate list is narrowed on evidence (Node/N-API struck as a conditional candidate on
a structural argument — not a measured Pareto elimination; the semantic-plane candidates shown to share
one engine), and the harness and metric list in this file stand as the measurement contract.

**Selection among the surviving candidates belongs to TCM2 and TCM3, by ruling.** `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q2: TCM0 owns candidate screening, survivor sets, metrics, harness, baseline method and selection rule;
TCM2 owns evidence-based projection-plane selection and TCM3 owns evidence-based semantic-plane selection,
each **as a blocking exit of its own block**. Each such selection is governed by
`performance-baselines.md`'s requirements 6-8 (a pre-implementation baseline capture, a pre-named
comparison workload, distribution-with-raw-samples reporting), so it is still judged against numbers
locked before that block's own results are visible. The reallocation is settled by the ruling rather than
carried as a pending charter amendment, and it is not a precondition of TCM0's acceptance. See
`OPEN-GAPS.md`'s `G-TOPOLOGY` row.
