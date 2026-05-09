# Component-Meta Hotpath Profile — 2026-04-25 (F8 baseline)

**Plan:** [`bench-meta-ui-latency-plan.md`](../tmp/bench-meta-ui-latency-plan.md) Step 1 (F8).
**Branch:** `refactor/semantic-db-overhaul`
**HEAD:** `b8e0e1b4` (descendant of plan base `4b146ff4`).
**Profile target:** `nuxt-ui` integration repo (`D:/dev/personal/verter/.integration-tests/repos/nuxt-ui`).
**Components profiled:** Table, EditorToolbar, InputMenu, NavigationMenu, Button, Alert (6 components, 1 repeat each).
**Tool:** `cargo run -p verter_bench --example profile_real_component_meta --release --features=hotpath`.

## Per-function hotpath breakdown (post-Step 2 lazy trace macro)

| Function | Calls | Avg | P95 | Total | % of main |
|---|---|---|---|---|---|
| `materialize_component_meta_member_surface_expr_with_active_stack` | 7830 | 29.23 ms | 26.53 ms | 228.89 s | 419.34% |
| `profile_real_component_meta::main` | 1 | 54.58 s | 54.59 s | 54.58 s | 100.00% |
| `materialize_component_meta_type_expr_until_stable` | 1111 | 37.76 ms | 36.34 ms | 41.95 s | 76.85% |
| `rematerialize_public_component_meta_types` | 6 | 5.95 s | 13.92 s | 35.69 s | 65.38% |
| `choose_less_symbolic_component_meta_type_expr` | 251 | 142.16 ms | 209.45 ms | 35.68 s | 65.37% |
| `compute_component_meta_state_inner` | 7 | 2.60 s | 8.46 s | 18.20 s | 33.34% |
| the legacy per-member materialiser | 123 | 112.03 ms | 149.29 ms | 13.78 s | 25.24% |
| the per-field rescue cascade driver (since retired) | 7 | 286.14 ms | 961.54 ms | 2.00 s | 3.66% |
| `produce_macro_object_shapes_for_purpose` | 7 | 158.87 ms | 442.50 ms | 1.11 s | 2.03% |
| `append_component_meta_registry_entries` | 6 | 144.21 ms | 251.13 ms | 865 ms | 1.58% |

`build_origin_graph` and `export_all_origin_edges` did not register a measurable percentage in this 6-component sample (audit-off; both are essentially no-ops post-Step-3 gate when audit is disabled, and the dispatch path that records origin edges isn't yet routed through component-meta resolution).

`projected_member_surface_keys` did not register either, suggesting it isn't on the hot path for these 6 components — its impact will be more visible on a larger sample of the corpus, where it intersects with cold-cache rematerialize work that Step 6.4 deletes.

## Cross-phase walker time ratio (Table + EditorToolbar)

The two engine lifetimes per the plan §1.4:
- **Compute phase** = `compute_component_meta_state_inner` (7 calls, 18.20 s, 33.3 % of main).
- **Rematerialize phase** = `rematerialize_public_component_meta_types` (6 calls, 35.69 s, **65.4 % of main**).

Rematerialize spends nearly **2× the compute-phase time** for this sample. The 2026-04-23 trace investigation
([`docs/component-meta-corpus-perf-investigation.md`](component-meta-corpus-perf-investigation.md)) reported a 108× redundancy for `Table.loadingAnimation` between compute and rematerialize. The aggregate per-component picture confirms the same pattern: rematerialize is a substantial second walk over inputs the compute phase already resolved. Step 6 (`F2 + F2b`) collapses the two walks into one cache-mediated path through `ProjectSemanticDispatch`.

## Eager `format!` allocation count under audit-off

| Run | `getComponentMeta` allocations |
|---|---|
| Pre-Step-2 (eager macro) | not captured (counter test added post-fix) |
| **Post-Step-2 (lazy macro)** | **4724** (from `baseline_trace_alloc_count.rs`) |

The `4724` count is the audit-off resolution allocation total for the small `defineProps<{ label: string; count: number }>()` fixture after F4. The pre-fix number is unrecoverable from the present tree because Step 2 already landed; the F4 commit message documents the eager-evaluation pattern that was eliminated. The bound is loose (200 000) — if a future change pushes this over a few thousand, the trace macro or another hot-path source has regressed.

## Bench baseline fixture

The Step 10 CI regression gate compares against per-scenario JSON baselines committed at:

- [`packages/benchmark/baselines/meta-ui-baseline-single_cold.json`](../packages/benchmark/baselines/meta-ui-baseline-single_cold.json)
- [`packages/benchmark/baselines/meta-ui-baseline-repo_first_pass.json`](../packages/benchmark/baselines/meta-ui-baseline-repo_first_pass.json)

These files were captured at `verterCommitSha: 4b146ff4` (the plan base, equivalent to `main` at the time the plan was authored — verified via the `verterCommitSha` field). They are split per scenario rather than the single combined file the plan suggested; Step 10 reconciles the comparison shape.

Headline numbers from the `single_cold` baseline (per plan §1.1):

| Scenario | p50 | mean | max | within-SLA (250 ms) |
|---|---|---|---|---|
| `single_cold` | 337 ms | 1999 ms | 26 447 ms (`EditorToolbar`) | 76 / 177 (43 %) |
| `repo_first_pass` | 419 ms | 2339 ms | 24 879 ms | 65 / 174 (37 %) |
| `repo_warm_second_pass` | 0 ms | 0 ms | 2 ms | 177 / 177 (100 %) |

Warm path is essentially free (final-result cache, fact-version revalidation). The cold-path tail is what Steps 6+7 attack.

## graphNode_leak baseline

Per plan §1.1 the corpus-categorization `graphNode_leak` bucket was **837** at `4b146ff4`. The Step 6 verification gate reduces this to **≤ 10**.

Re-running the corpus categorization at this checkpoint (post-Step 1+2+3, pre-Step 6) is not expected to move the number — the structural fix is the dispatch-routing change in Step 6, not anything in Steps 1-5. The number is recorded here as the pre-Step-6 baseline; Step 6.6 verification re-runs and asserts the drop.

## Notes on plan-text imperfections discovered during execution

- Plan §3 Step 1 references `materialize_member_route_current` as a function name; that string is the trace event name. The enclosing function is the legacy per-member materialiser ([`crates/verter_session/src/meta_resolve.rs:7899`](../crates/verter_session/src/meta_resolve.rs:7899)) — verified via grep when adding instrumentation.
- Plan §3 Step 1 prescribes `cargo build --release --package verter_napi --features hotpath`; `verter_napi` does not declare a `hotpath` feature. Bench profiling runs via `cargo run -p verter_bench --example profile_real_component_meta --release --features=hotpath` directly. The verter_napi build is only needed when the SECOND part of Step 1 (running `pnpm bench:meta:ui` to capture a baseline JSON) is performed.
- `projected_member_surface_keys` is at line 3067, not 3077 (10-line drift).
- `materialize_component_meta_member_surface_expr_with_active_stack` is at line 9591, not 10246.
