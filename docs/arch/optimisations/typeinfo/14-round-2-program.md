# 14 — Round-2 program: allocation/memory reduction + residual CPU + correctness

**Base:** `release/typeinfo-consolidation` @ `b67c05a8e` (round-1 assembled). **Assembled result:** `ff298c5b0`.
**Headline (3× interleaved, quiet, 179/179 both sides):** steady **6144 → 4594 ms (−25.2%)**, p50 16.8→12.5 ms,
p95 113→80 ms, **peak worker RSS 720 → 624 MB (−13.3%)**. Memory audit: allocCount 89.7 M → 80.2 M (−10.6%),
max component peak-live 536 → 518 MB (allocatedBytes total not comparable across the boundary — the round-2
correctness fixes enrich resolution, i.e. more real work).
Cumulative lineage: consolidation base 22.5 s → round-1 5.9 s → **round-2 4.59 s (~4.9×)**.

## Evidence stack (tools; all runtime, single binary)

samply CPU profile; runtime memory audit (`--profile-audit` → `.profile.json`: per-component phase timings +
alloc counters + RSS + JS heap); sampled allocation sites (`VERTER_MEMORY_AUDIT_SAMPLE=N`, in-allocator
backtraces, `estimatedTotalBytes = bytes×N`); macOS `MallocStackLogging`+`malloc_history` caller pass
(under-captures deep stacks — superseded by the in-allocator sampler); audit footprint counters.
Adversarial review: gpt-5.6-sol xhigh, line-cited (rejected M3 whole-env CoW — path designed for deletion —
and M7 fs-exists cache — dir-index already exists; identified the shared name-resolution table as top ROI).

## Landed items (each its own branch, all merged into `ff298c5b0`)

| Branch @ commit | What | Isolated measurement |
|---|---|---|
| `perf/r2-static-default-excludes` @ de34e6bb0 | root-KEYED memo of compiled default-exclude globs (patterns embed root — a root-free singleton is wrong); shared `Arc<[CompiledGlob]>` in membership specs | ~100 MB est alloc + 2.5% glob CPU class |
| `perf/r2-cached-default-env` @ 2923d2792 | `ArcSwap`-cached default env-hash array validated by extensions-`Arc` identity + `OnceLock` project identity (was: fresh `IdeProjectConfig`+full membership per store-view build) | store-view-build hot path |
| `perf/r2-capacity-hints` @ d25f0d7d3 | 15 exact-cardinality `with_capacity`/direct-`Arc`-collect sites (lowering, fact emission, shallow-state assembly); new `TypeExpr::{union,intersection}_from_exact_iter` | −2.0% allocatedBytes, peak-live flat |
| `perf/r2-shared-name-resolution` @ 40dac4629 | ONE per-file base `name_resolution` table (`Arc<OnceLock<…>>`) shared across prepared decls + private namespaced overlay tables; precedence red-proofed | **peak-live −10.8%, RSS −6.3%**, wall-neutral |
| `perf/r2-identity-interning` @ df34554a1 | store-owned byte-bounded (4 MiB) `Arc<str>` interner (SipHash, content Eq/Hash, safe eviction); `ResolvedRootIdentity` fields → `Arc<str>`; route-digest clone-to-hash removed | −1.8% allocs, **peak-live −7.0%**, wall-neutral |
| `perf/r2-owners-memo` @ 15dfea40d | snapshot-owned bounded (16k, SipHash) `owners_for_file` memo incl. negatives | warm lookup 258→57 ns, alloc-free hits |
| `fix/pick-over-instantiated-generic` @ 3c1adfd18 | unverifiable type-param default no longer vetoes key-domain closedness (binds Open positionally); L1 Pick carrier enumerates its CLOSED key selection instead of zero members | ContentSearch 24→42 props; DropdownMenuContent 4→9 slots |
| `fix/distributive-conditional-and-anchor-slot` @ 7b31202dc | distributive gate resolves carrier-shaped union checks via the shared normalizer (fail-closed); `anchor` slot recovered via new structural `SlotAnalysis.declared_in_macro_type_arg` fact (additive proto field 8) | emit payloads match TS truth; Popover publishes `anchor` |
| `feat/memory-audit-alloc-sites` @ 5630955c5 | the profiling tool itself (doc 12) | — |

Merge reconciliation of note: R2-A × M4 combined to
`name_resolution: Arc<FxHashMap<Arc<str>, ResolvedRootIdentity>>` (shared base + interned keys); both suites
green post-merge; a `SharedNameResolutionBase` alias resolves clippy `type_complexity`.

## Rejected / deferred (review-verdicts)

M3 EvalEnv CoW (optimizes the whole-env path already designed for deletion — finish that cutover instead);
M7 fs-exists cache (dir-index with positive/negative + dirty invalidation already exists — attribute residual
syscall callers first); M2 beyond capacity hints (SmallVec/hash-cons/arena rejected — recursion, identity, and
second-representation hazards); M6 worker-handoff fusion (approved-with-conditions but gated on queue/service/
response instrumentation — not attempted this round; the 11.5% main-thread wait is exposure, not proven overhead).

## Parity & verification

179/179 on every run; drift vs the consolidation-base manifest = exactly the enrichment families (script-fix 12
∓ fix components: +Accordion/ContentNavigation/Popover/DropdownMenuContent). Open question logged:
`content/ContentSearch`'s final hash converged back to the base manifest value — verify expected composition on
the production machine. Session lib 4306/0; clippy workspace delta zero; fmt clean; lib.rs ceiling green; gate
run on the assembled branch with pre-existing-failure attribution (see feedback file; the base itself carries
environmental/toolchain-skew failures — every port/round-2 failure was three-way attributed to base).
Known follow-ups: 19 compat vitest pins (4 nuxt-ui parity pins likely need regeneration against enriched
outputs); corpus nondeterminism in AUDIT payloads (`reactivityKind` flap, symlink path spelling) — normalized
bench artifacts are stable; printer-hardening family (doc 13) still open; closedness-proof budget memoization
(pick-fix follow-up); `engine.rs` ownership classifiers bypass the owners memo.
