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
| Cold start | time from process spawn to first successful `initialize` response, candidate package, this exact darwin-arm64 host (recorded baseline: 34ms `API` construction + 1037ms first `updateSnapshot` for a 1-file project — `package-lock-and-semantic-api.md` §4a; NOT a topology comparison, just the one-topology number already measured) | lower is better; a candidate that is slower on cold start must win on warm/rapid-edit to be non-dominated |
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
