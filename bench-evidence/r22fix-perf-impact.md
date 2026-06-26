# R22-fix Corpus Perf Impact — sparse `CarrierProvenanceTable`

## TL;DR

The R22 architectural cleanup landed (the per-field
`Option<CarrierProvenance>` was replaced with a sparse
`CarrierProvenanceTable` owned by `ExpandedComponentTypes`, threaded
explicitly only at the two carrier-aware consumer sites). The
aggregate `repo_first_pass` corpus result is **+20.4 % above R21.5**
(70 699 ms vs R21.5's 58 733 ms) — only **-0.7 %** below R22.

**Per the brief's STOP-and-escalate triggers, the fix-cycle stops
here.** Codex's BINDING diagnosis ("the per-field `Option<CarrierProvenance>`
on `ExpandedField` / `ExpandedProperty` is the systemic +30-45 %
overhead driver") is **empirically refuted**: removing the field and
moving the provenance to a sparse sidecar produced only ~0.7 %
aggregate improvement — meaning the wide layout was NOT the dominant
cost driver.

## Aggregate (repeat 1, `repo_first_pass`)

| run     | sum (ms) | vs R21.5 | vs R22 |
| ------- | -------: | -------: | -----: |
| R21.5   | 58 733.06 | (baseline) | — |
| R22     | 71 204.54 | **+21.2 %** | (baseline) |
| **R22-fix** | **70 699.19** | **+20.4 %** | **-0.7 %** |

(Numbers from `bench-evidence/r21.5-bench.txt`, `bench-evidence/r22-bench.txt`,
`bench-evidence/r22fix-bench.txt`, all repeat-1 `repo_first_pass`
sum-of-component-latencies; corpus = 179 nuxt-ui components.)

**Aggregate gate (`sum-after ≤ sum-R21.5`)**: ❌ **FAILS** by +20.4 %.

## R22 wins preserved

| component       | R21.5  | R22    | R22-fix | vs R21.5 | vs R22 |
| --------------- | -----: | -----: | ------: | -------: | -----: |
| `ChatMessage.vue`   | 534.86 | 432.41 | **414.84** | -22.4 % | -4.1 % |
| `Tree.vue`          | 1084.06 | 761.85 | **712.27** | -34.3 % | -6.5 % |
| `Accordion.vue`     | 794.21 | 560.23 | **504.21** | -36.5 % | -10.0 % |
| `CommandPalette.vue` | 1227.25 | 1160.82 | **1127.37** | -8.1 % | -2.9 % |
| `DropdownMenu.vue`  | 662.66 | 674.90 | **628.78** | -5.1 % | -6.8 % |
| `NavigationMenu.vue` | 1145.47 | 1146.98 | **1127.84** | -1.5 % | -1.7 % |

ChatMessage closure preserved: **414.84 ms** — well under the brief's
600 ms gate. Tree / Accordion / CommandPalette / DropdownMenu all
keep R22's wins **AND improve marginally further** under R22-fix
(carrier-aware short-circuits still fire; the producer's
`DoNotDeepen` admission + reducer skip + registry refuse-to-enqueue
all wired through the new table).

## Systemic regressions — NOT closed

| component       | R21.5  | R22    | R22-fix | vs R21.5 | vs R22 |
| --------------- | -----: | -----: | ------: | -------: | -----: |
| `Table.vue` (main) | 1924.63 | 2735.37 | 2742.68 | **+42.5 %** | +0.3 % |
| `Calendar.vue`  | 397.27 | 582.61 | 577.70 | **+45.4 %** | -0.8 % |
| `InputDate.vue` | 574.40 | 827.26 | 797.43 | **+38.8 %** | -3.6 % |
| `Pagination.vue` | 422.23 | 594.76 | 576.76 | **+36.6 %** | -3.0 % |
| `Tooltip.vue`   | 408.70 | 577.72 | 538.17 | **+31.7 %** | -6.8 % |
| `Sidebar.vue`   | 418.48 | 588.09 | 569.93 | **+36.2 %** | -3.1 % |

Every systemic regression codex flagged remains essentially at R22
levels. The R22-fix produced 0.3–6.8 % improvements on these per
component (cache warmup noise band), nowhere near closing the
+30–45 % gap to R21.5.

Other regressed components in the same set:

| component       | R21.5  | R22    | R22-fix | vs R21.5 | vs R22 |
| --------------- | -----: | -----: | ------: | -------: | -----: |
| `ChatMessages.vue` | 749.73 | 1052.01 | 969.58 | +29.3 % | -7.8 % |
| `Listbox.vue`   | 894.22 | 960.67 | 973.99 | +8.9 % | +1.4 % |
| `Select.vue`    | 1590.62 | 1786.15 | 1755.99 | +10.4 % | -1.7 % |
| `InputMenu.vue` | 1864.84 | 2055.20 | 1990.35 | +6.7 % | -3.2 % |
| `SelectMenu.vue` | 1916.97 | 2156.24 | 2189.71 | +14.2 % | +1.6 % |

## Architectural state (commit 1 of the fix-cycle)

The R22-fix code changes:

1. **`crates/verter_semantic/src/analysis/type_expand/request.rs`** —
   removed the `carrier_provenance: Option<CarrierProvenance>` field
   from both `ExpandedField` and `ExpandedProperty`. Added a new
   `CarrierProvenanceTable` struct with two `FxHashMap<String,
   CarrierProvenance>` sub-maps keyed by surface kind (SlotBinding,
   Binding) and field name. Owned by `ExpandedComponentTypes`
   (`#[serde(skip)]` — never leaks to FFI).
2. **`crates/verter_session/src/meta_resolve/slot_binding_graph.rs`** —
   the producer at `publish_merged_bindings`' no-parser branch now
   calls `expanded.carrier_provenance_table.insert(SlotBinding,
   field_name, provenance)` instead of setting a per-field option.
   The `CarrierVerdictDb::admit_do_not_deepen(...)` eager admission
   is unchanged.
3. **`crates/verter_session/src/meta_resolve/projectors/published_reducer.rs`** —
   `should_skip_carrier_reduction` rewritten to take a
   `PublishedSurfaceKind` + `&CarrierProvenanceTable`; consults the
   table by `(surface_kind, &field.name)`. The `reduce_published_field_types`
   loop split-borrows the table immutably alongside the mutable
   slot_bindings / bindings vec iteration. Props / emits paths do
   NOT consult the table.
4. **`crates/verter_session/src/host_manage/component_meta_methods.rs`** —
   the registry's refuse-to-enqueue moved from inside
   `collect_component_meta_registry_public_field_refs` to the
   caller's `slot_bindings` loop (`if carrier_table.contains(SlotBinding,
   field.name.as_str()) { continue; }`). Props / emits paths now
   call `collect_component_meta_registry_public_field_refs` without
   any carrier-aware lookup, so they pay zero layout cost.
5. **Mechanical fixture cleanups** — ~95 `carrier_provenance: None`
   construction-site lines removed across 11 files. Construction
   sites that previously used `..Default::default()` get the new
   field for free.
6. **Test updates** — T1 / T2 / T3 carrier-verdict-discrimination
   tests + `slot_binding_shallow_publication` tests rewritten to
   consult the table (`expanded.carrier_provenance_table.get(...)`
   / `.contains(...)`) instead of `field.carrier_provenance`.

## Empirical refutation of codex's diagnosis

Codex's R22-systemic verdict (`<scratch>/round22-systemic-codex-out.txt`):

> SYSTEMIC-OVERHEAD-LOCATION: a +
> `crates/verter_semantic/src/analysis/type_expand/request.rs:52` and
> `:359`; `Option<CarrierProvenance>` bloats `ExpandedProperty` /
> `ExpandedField` for every component surface even when `None`,
> matching the uniform non-carrier regressions.

The R22-fix removed exactly the field codex identified, replaced it
with a sparse sidecar that is empty for non-carrier components, and
the systemic regressions on `Table.vue` / `Calendar.vue` /
`InputDate.vue` / `Pagination.vue` / `Tooltip.vue` / `Sidebar.vue`
(none of which mint synthetic slot-binding carriers) **did not
close**. The per-component improvements vs R22 are 0.3–6.8 % —
within the cache-warmup noise band, nowhere near the +30–45 %
regression codex's diagnosis predicted would close.

**Conclusion**: the cost driver of the +21.2 % R22 aggregate
regression is NOT the wide `Option<CarrierProvenance>` layout.

The R22-fix is still architecturally valid (the sparse sidecar is the
cleaner long-term design — props / emits paths legitimately should
not pay carrier-aware costs; T1 / T2 / T3 discrimination remains
sound via the table keying). But the perf gate is unaffected.

## STOP-and-escalate

Per the brief's STOP triggers:

> Phase D systemic regressions don't close (Table still > 2000ms,
> etc.). STOP — the sparse-sidecar fix didn't address the actual
> cost driver. Re-consult codex on alternative G (typed-IR variant +
> ShapeCacheDb).

> Phase D aggregate STILL above R21.5 (58 733 ms). STOP — partial
> improvement; document and request codex on next-step.

Both triggers fire. The fix-cycle stops at this point. The
orchestrator should consult codex with the empirical refutation of
the sparse-sidecar diagnosis and the bench evidence above, and
pursue the BINDING ALTERNATIVE-IF-FIX-FAILS-EMPIRICALLY codex
already named:

> ALTERNATIVE-IF-FIX-FAILS-EMPIRICALLY: Revert R22 and promote
> synthetic carriers to a skinny typed-IR variant backed by
> `ShapeCacheDb::semantic_node_whole`.

Suggested next steps for the next round:

1. Run the prefix-bisect codex recommended in BISECTION-STRATEGY (1)
   — checkout `68daa4485`, `b3216149a`, `62f7c54fc`, `6cfebe2d6`,
   `d0b66a47a`, `8f867d2bd`, `7593991bd`, `8ee6792b0` in turn and
   bench each. The bisect was skipped in this fix-cycle for
   compute-budget reasons; with the sparse-sidecar diagnosis now
   refuted, identifying the actual cost commit empirically is the
   shortest path to the real fix.
2. Or jump directly to codex's TYPED-IR-VARIANT-LONG-TERM (YES):
   "implement it as a skinny/boxed first-class carrier routed
   through `ShapeCacheDb::semantic_node_whole`, not as a wide
   `TypeExpr` enum arm." The bench evidence here suggests this is
   now the next move, not deferred.

## Workspace gate state

All Rust tests pass except the 9 documented pre-existing failures
(unchanged across the fix-cycle: 2 lib + 7 integration). All R22
substrate tests (T1 / T2 / T3 / 8 carrier_verdict_db unit + 3 slot
binding shallow publication) pass against the table-based
carrier-provenance plumbing. Cargo clippy + fmt + pnpm
install-frozen-lockfile + build:native all clean. The R22-fix
landed as a structural-cleanup-only commit; the perf gate failure
is the only blocker.

## Files

- `bench-evidence/r22fix-bench.txt` — repeat-1 `repo_first_pass`
  raw timings, 179 components.
- `bench-evidence/r21.5-bench.txt` (existing, pre-R22 baseline).
- `bench-evidence/r22-bench.txt` (existing, R22 final).
- `bench-evidence/r22-perf-impact.md` (existing, R22 analysis).
