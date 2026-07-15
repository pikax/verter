# 16 — Round-4: two-model adversarial hunt + hot-path/encode trims

**Base:** `release/typeinfo-consolidation` @ `d624dfd1b` (post round-3). **Assembled:** `acf84457f`.
**Cumulative R4 (bin-swap vs d624dfd1b, 3× interleaved, 179/179, 0-drift):** steady **−3.0%**
(Table −4.5%, SelectMenu −5.6%, p95 −4.6%), peak RSS flat.

## Method: two independent models, same prompt

Both `codex gpt-5.6-sol` (xhigh) and `grok-4.5` were given the identical hunt prompt (landed+rejected
lists included) read-only on the release tree. **Convergence = high-confidence; divergence = leads;
re-derivation of already-landed work = validation.** This is the reusable pattern — see the
`perf-review-tools` memory for invocation quirks (`codex` needs `--disable code_mode_host`; `grok -p`
headless). It caught a mislocated lever (below) that a single opinion would have sent to implementation.

## Landed (4 commits, all 0-drift parity, clippy/fmt clean, ~4339 lib tests)

Wave-1 (both models independently re-derived these — validation):
- `perf/r4-walk-fastpaths` — (a) fast-path already-materialized shallow Object roots in the empty-path
  walker (skip re-intern when no role/provenance transform applies); (b) borrowed hop needle (`&str`, no
  alloc) + identity-excluded member-ordinal sidecar (O(1) lookup on >16-member surfaces) + one-pass
  union-arm merge (was O(arms×members²)). **−3.3% steady** solo (Accordion −16.6%, Theme −4.0%);
  +12 MB RSS (the sidecar); latency win, net-flat allocs.
- `perf/r4-key-trims` — (a) hash `FactVersionRef` variants via their typed `Hash` directly (was
  `format!("{:?}")`+hash; fingerprint is write-only/in-memory, audited inert); (b) single prepared
  `{family,slot,requested_point,cached_hash}` dispatch-identity token reused across entries/in-flight/
  recursion (bijection-tested exhaustively over all 22 `SemanticQueryKey` variants, R6-safe). **−2.0%
  steady**, p50 −6.9%, RSS −12 MB.

Wave-2 (the genuinely-new double-confirmed picks):
- `perf/r4-node-arena-intern` (codex#4 = grok#1, both top) — `NodeArena` dedup index was keyed on the
  full `(SemanticNodeData, NodeScopeId)` payload (hashed 2×, payload owned twice). Now a process-seeded
  SipHash u64 fingerprint narrows to a `SmallVec` bucket; payload interned once + `Arc`-shared; content-
  `Eq` on collision is the identity authority (collision-disambiguation test forces a same-fingerprint
  bucket). **p50 −1.8% / p95 −2.8%** (Table −5.2%, NavMenu −8.2%); **memory FLAT** — see premise
  correction below.
- `perf/r4-encode-single-own` (codex#6 = grok#8) — `GraphBuilder` de-duplicated: id-only non-owning
  reverse indices (was `HashMap<String,u32>` + `HashMap<GraphNode,u32>` owning twice), payload/nodes
  interned once, `into_tables()` consumed by value into proto (no whole-table re-clone). **−34% encode
  alloc count / −18.8% bytes**, byte-identical proto verified over 250 components, wire contract
  unchanged. Transient allocs → flat steady RSS/wall (allocation-churn win).

## Premise correction (the honest memory finding)

Both models predicted the NodeArena "double-own" would cut RSS 2-5%. It did NOT — the duplicate was
*shallow*: `SemanticNodeData`'s heavy fields (surface members, id slices, `Arc<str>` names) were
**already `Arc`-shared** by rounds 1-3's interning + Arc-sharing work. **There is little deep-copy
resident memory left to reclaim.** The remaining wins are latency and allocation-churn, not
resident-set — a signal the memory axis is near its floor for this workload.

## Measured NO-GO (investigate-first paid off)

grok#4 "materialize/raise memo" — proposed as the biggest lever (⅔-of-query materialize phase). A
scope-first investigation (env-gated fold instrumentation) proved it **mislocates the cost**: the fold
(`fold_to_type_expr`) is a pure fn of node id (soundly memoizable) but only **0.02-0.4%** of the phase
(avg fold tree 3.1 nodes); the raise is already `OutputSourceMemo`-deduped; grok's "⅔" came from a
harness that never calls the output-fold (`build_output_calls=0`). Real `materialize_ms` =
`reduce_graph_node_iterative` cold reduction, already cached at 3 levels (family memo / ShapeCacheDb
peek-first / MaterializeMemoDb). **NOT implemented.** Future lever (deferred): improve
`reduce_graph_node_iterative`/ShapeCacheDb *hit-rate* (why cold builds recur), not add a cache.

## Model divergence (unique leads, not yet pursued)

- codex-only: projection-path spine (build.rs:8471+ regenerate owned prefixes per deep-path probe/
  record/backfill — parent-linked path handles, p95-weighted 3-8%); generic-aware one-pass OXC lowering
  (avoid lower-then-rebuild of fn-type generics, the 79 MB `lower_ts_type` site); quadratic registry-
  sidecar join (convert/component_meta.rs:247).
- grok-only: MergedDecl reduce memo (rebuilds every hop); ImportedRoot `Arc<str>` + delete `as_tuple`
  clone API (~28 MB class, low-risk); ObjectProperty.name `String`→`Arc<str>` at OXC lower (79 MB site,
  but wire/TS-mirror blast radius).
Residual-floor leads (both/R3): FamilyKey length-1 path Arc reuse, canonical_dep_union incremental,
probe_path buffer.

## Program status

Cumulative lineage: 22.5 s → 5.9 s (R1) → 4.59 s (R2) → 4.53 s (R3) → **~4.4-4.6 s (R4, −3%)**; RSS
720 → ~620 MB. Per-round deltas: 3.8× / −25% / −4% / −3% — clearly diminishing. The two-model hunt's
own verdict corroborates it: the biggest fresh candidates were already-done, mislocated, or memory-flat.
The one remaining plausible real lever is codex's projection-path spine (p95/tail-weighted). Beyond it,
the step-changes are design programs (cross-thread cooperative cycle detection, persistent artifact
cache) documented in doc 15. **Recommended stopping point for the micro/macro optimisation methodology.**
