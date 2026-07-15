# 15 — Round-3: allocation wave (landed) + two data-settled negatives

**Base:** `release/typeinfo-consolidation` @ `fad3488e4` (post round-2).

## Landed: `perf/r3-clone-reduction` (4 commits, tip `87fcfcdfd`)

Full-corpus 3× interleaved: **steady 4976 → 4783 ms (−3.9%**, branch max < base min, base spread ±1.1%),
p50 −4.3%, p95 −3.9%, RSS flat. profile-audit (limit-30, N=97): **allocCount −15.1%, allocatedBytes −8.3%**.
Parity: 0 drift (179/179 sha256, double-run-deterministic manifests). Gates: session 4312/0, semantic 1445/0,
workspace clippy clean, fmt clean.

1. `1b2cec1c4` — `FileArtifactStore::snapshot_artifacts` OFF the query path: `snapshot_file_facts_into`
   (store-view build!) walked the WHOLE store per request; replaced with targeted per-tracked-canonical reads
   via the canonical→keys index (new dangling-tolerant `for_each_artifact_for_canonical_content`); the
   audit-only caller became a non-materializing fold.
2. `06b855af3` — `DeclarationScopePayload::from_bundle` = `Arc<PreparedDeclBundle>` view (1 refcount bump;
   4 map deep-copies + a materialized name-union deleted; 9 consumer sites compiler-enumerated to co-check
   `scope_type_bindings`). 10.7 MB → 0 on the sampled window.
3. `08a3f2c2a` — top-2 `EvalEnv::clone` sites: `local_type_declaration_id` deep-cloned the per-file env per
   `get_component_meta` to read ONE map entry → Arc read (owned `base_eval_env` deleted);
   `FallthroughComputeHost::EvalEnv = Arc<EvalEnv>` (no-hydration path returns the memo Arc, ptr_eq-pinned).
   25.9 MB → 0. The merge substrate was NOT CoW'd (review-rejected).
4. `87fcfcdfd` — `observe_fan_out` moves the owned `FactVersionRef` into the last tracer (zero fact clones on
   the single-tracer common case) + exact pre-sized audit trace strings.
Skipped with reason: `property_key_name` interning (Strings MOVE into typed-IR fields — no double alloc;
session interner unusable inside verter_semantic). Round-4 leads in feedback: `canonical_dep_union` ~5.6 MB,
`probe_path` format chains, `as_tuple` Arc-str migration (schema-bound).

## Data-settled negative #1: M6 handoff fusion — REJECTED by its own gate

Branch `perf/r3-handoff-fusion` @ `4cc524e3a` (phase 1 only — KEEPER TOOL: env-gated
`VERTER_DECL_HANDOFF_PROFILE` three-way rendezvous profiler + `dump_decl_handoff_stats` accessors + bench
example wiring; off-path byte-identical). Measured: caller-blocked handoff = 4.7% of a cold pass, of which
~63% is useful-work waiting; the FUSIBLE portion (acquire queue+response) = **0.14%** — far under the 2%
gate. Warm passes acquire zero leases. The round-2 profile's 11.5% `semaphore_wait_trap` is exposure, not
overhead. Do not implement fusion; keep the profiler.

## Data-settled negative #2: intra-query parallelism — NOT PRODUCTIONIZED

Branch `perf/r3-intra-query-parallelism` @ `1ebe27647` (env-gated `VERTER_EXPERIMENT_PARALLEL=1`; env-off
untouched; 4306 lib tests green both modes; 0 parity drift; no deadlocks in ~40 runs / >1500 fanned resolves).
Two sound seams (per-macro projector fan-out; per-field registry collection) via a fact-capsule fork pattern:
workers rebuild concrete resolver contexts from Send-able parts, run under the parent's RequestContext guard
(shared budget), capture facts in worker-local tracers, and the parent REPLAYS fact sets via
`observe_borrowed_signature` before publish; worker overflow ⇒ parent non-cacheable (never poisons).
Results: heavies −2.5…−10% solo-cold, −10…−17% steady re-query, ONE regression (ChatMessages +6.6%),
full-corpus NEUTRAL. **Core finding: the p95 tail is depth-bound, not width-bound** — one
`Instantiate`/`ResolveMacroPayload` cascade is a sequential recursive walk protected by per-thread cycle
rails (`IN_FLIGHT_ON_THIS_THREAD`, per-thread `MaterializeRuntimeKey`); fanning inside a walk converts
same-thread cycle detection into cross-thread condvar mutual waits (a real deadlock class). The gating
redesign for any future attempt: cross-thread cycle detection / wait-graph in `execute_cooperative`
(salsa-style). If revisited: keep the fact-capsule scaffold, move workers onto `HostCpuPool` with a
reentrancy guard, and gate fan-out on width (fixes the ChatMessages regression).

## Program status after round 3

Cumulative lineage: 22.5 s → 5.9 s (R1) → 4.59 s (R2) → **≈4.4 s (R3 keeper est. on quiet runs)**; RSS
720 → 624 MB; allocations −25%+ vs pre-R2. Remaining step-changes are DESIGN programs, not rounds:
cross-thread cooperative cycle detection (unlocks the depth-bound tail), persistent cross-session artifact
cache (changes cold-pass economics), ambient-lib deep resolution (doc 10), whole-env consumer cutover
(Stage-10-owned). Diminishing-returns point for the current methodology: reached.
