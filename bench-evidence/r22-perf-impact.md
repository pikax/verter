# R22 — synthetic slot-binding carrier verdict cache: per-component perf impact

Bench: `pnpm --filter @verter/benchmark bench:meta:ui -- --scenarios=repo_first_pass --hard-timeout-ms=60000`

Corpus: 179 nuxt-ui components (workspace at `.integration-tests/repos/nuxt-ui`).

Gate: **358 / 358** outcomes (179 baseline-ready + 179 repo_first_pass, all `outcome=success`) across both runs.

Baseline (compared against): `bench-evidence/r21.5-bench.txt` (commit `ccaddd481` — R21.5 landing).

After: `bench-evidence/r22-bench.txt` (run 1) + `bench-evidence/r22-bench-rerun.txt` (run 2).

R22 numbers below are **best-of-two** to suppress per-run noise.

## Aggregate

```
sum-R21.5-repo_first_pass         = 58 733.06 ms
sum-R22-repo_first_pass (run 1)   = 71 204.54 ms
sum-R22-repo_first_pass (run 2)   = 70 646.77 ms
sum-R22-repo_first_pass (best/2)  = 69 546.63 ms
delta-aggregate (best/2 vs R21.5) = +10 813.57 ms (+18.41 %)
```

**The aggregate gate FAILS.** The brief's threshold is `sum-after ≤ sum-R21.5` (58 733 ms);
the observed best-of-two is +18.41 % over baseline. This triggers the brief's
STOP-and-escalate condition:

> Final bench shows the 10 components STILL regressed (>20 % above R21-fix baseline).
> The cost driver is elsewhere. STOP — re-consult codex with empirical data + the
> ALTERNATIVE codex named ("promote carrier to explicit typed-IR variant").

The 10 codex-flagged components are NOT uniformly regressed (5 improved, 5 regressed,
1 flat — see next section). The aggregate regression is concentrated outside the
flagged set: ~165 components show a uniform ~+30-45 % increase. Per-component data
is reproducible across the two runs (within ~1 % aggregate), so this is not run-to-run noise.

## 10 codex-flagged components — observed delta

| component | R21.5 (ms) | R22 run 1 | R22 run 2 | R22 best | delta | % | status |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `ChatMessage.vue` | 534.86 | 432.41 | 372.77 | **372.77** | **-162.09** | **-30.3 %** | **IMPROVED** — R21.5 closure preserved (target was <600 ms) |
| `Tree.vue` | 1 084.06 | 761.85 | 724.98 | 724.98 | -359.08 | -33.1 % | IMPROVED |
| `CommandPalette.vue` | 1 227.25 | 1 160.82 | 1 105.38 | 1 105.38 | -121.87 | -9.9 % | IMPROVED |
| `ChatMessages.vue` | 749.73 | 1 052.01 | 972.07 | 972.07 | +222.34 | +29.7 % | REGRESSED |
| `Accordion.vue` | 794.21 | 560.23 | 522.28 | 522.28 | -271.93 | -34.2 % | IMPROVED |
| `InputMenu.vue` | 1 864.78 | 2 055.20 | 2 074.84 | 2 055.20 | +190.36 | +10.2 % | REGRESSED |
| `NavigationMenu.vue` | 1 145.45 | 1 146.98 | 1 169.66 | 1 146.98 | +1.51 | +0.1 % | FLAT |
| `SelectMenu.vue` | 1 916.97 | 2 156.24 | 2 170.10 | 2 156.24 | +239.27 | +12.5 % | REGRESSED |
| `Select.vue` | 1 590.62 | 1 786.15 | 1 745.41 | 1 745.41 | +154.79 | +9.7 % | REGRESSED |
| `DropdownMenu.vue` | 663.10 | 674.90 | 639.69 | 639.69 | -22.97 | -3.5 % | IMPROVED |
| `Listbox.vue` | 894.20 | 960.67 | 1 048.32 | 960.67 | +66.45 | +7.4 % | REGRESSED |

Net on the 10-component set:

```
improvements (sum of -delta): −938 ms (Tree, Accordion, ChatMessage, CommandPalette, DropdownMenu)
regressions  (sum of +delta): +873 ms (ChatMessages, SelectMenu, InputMenu, Select, Listbox)
NavigationMenu flat at +1 ms
net on 10 components:        −65 ms
```

The R21.5 closure invariant holds: **ChatMessage stays well under the 600 ms gate**
(372 ms — even better than R21.5's 535 ms baseline). Tree, Accordion, CommandPalette,
and DropdownMenu absorb the targeted on-demand re-resolve cost the R21.5 perf-impact
doc named as the "lazy-resolution cache miss on graph-native-only bindings" follow-up.

The five regressed components (ChatMessages, InputMenu, SelectMenu, Select, Listbox)
do not match the predicted cache-hit pattern. Their tail cost did NOT drop with the
verdict-cache skip-and-refuse-to-enqueue pair.

## Top regressions outside the 10-component set

The aggregate regression of +18.41 % is dominated by ~165 components showing
uniform ~+30-45 % increases. Top 15 absolute deltas:

| component | R21.5 (ms) | R22 (best of 2) | delta | % |
| --- | ---: | ---: | ---: | ---: |
| `Table.vue` | 1 924.63 | 2 621.27 | +696.64 | +36.2 % |
| `ChangelogVersions.vue` | 780.51 | 1 052.34 | +271.83 | +34.8 % |
| `ChatMessages.vue` | 749.73 | 972.07 | +222.34 | +29.7 % |
| `SelectMenu.vue` | 1 916.97 | 2 156.24 | +239.27 | +12.5 % |
| `FileUpload.vue` | 627.01 | 849.39 | +222.38 | +35.5 % |
| `InputDate.vue` | 574.40 | 827.26 | +252.86 | +44.0 % |
| `InputTime.vue` | 536.77 | 749.80 | +213.03 | +39.7 % |
| `InputNumber.vue` | 486.34 | 688.76 | +202.42 | +41.6 % |
| `InputMenu.vue` | 1 864.78 | 2 055.20 | +190.36 | +10.2 % |
| `Calendar.vue` | 397.27 | 582.61 | +185.34 | +46.7 % |
| `ContentSearch.vue` | 542.64 | 719.86 | +177.22 | +32.7 % |
| `DashboardSidebarCollapse.vue` | 646.53 | 819.55 | +173.02 | +26.8 % |
| `Pagination.vue` | 422.23 | 594.76 | +172.53 | +40.9 % |
| `Sidebar.vue` | 418.48 | 588.09 | +169.61 | +40.5 % |
| `Tooltip.vue` | 408.70 | 577.72 | +169.02 | +41.4 % |

The pattern — uniform ~+30-45 % regression across many small components that
almost certainly do not mint synthetic slot-binding carriers — points to a
**systemic R22 overhead** rather than a downstream effect of S4/S5's
short-circuits. The shape of the regression rules out "the consumer-side
on-demand resolution costs more than the producer-side reduction did" as the
sole driver: the small-component regressions affect components without
synthetic carriers at all.

## R22 changes that landed in this block (recap)

| commit | description |
| --- | --- |
| `b3216149a` | refactor(types): add CarrierProvenance sidecar to ExpandedField + ExpandedProperty |
| `62f7c54fc` | feat(meta): emit CarrierProvenance at publish_merged_bindings for synthetic carriers |
| `6cfebe2d6` | feat(session): host-owned CarrierVerdictDb with DoNotDeepen sentinel |
| `d0b66a47a` | fix(meta): published_reducer skips synthetic carriers with DoNotDeepen verdict |
| `8f867d2bd` | fix(meta): registry refuse-to-enqueue for synthetic slot-binding carriers |
| `7593991bd` | test(meta): T1+T2+T3 carrier-verdict discrimination tests |
| (this commit) | chore(bench): R22 corpus perf impact + bench evidence |

`cargo test --workspace --tests --no-fail-fast`: same 8 pre-existing failures
(unchanged); +12 new R22 tests added and passing (9 carrier_verdict_db unit
tests, 2 slot_binding_shallow_publication integration tests, T1/T2/T3
discrimination tests).

## STOP — codex re-consult required

The brief's gate (`sum-after ≤ sum-R21.5`) fails. This is the STOP-and-escalate
trigger; the orchestrator must surface the empirical regression to codex with:

1. The aggregate +18.41 % regression evidence (this doc + the two bench txt files).
2. The targeted improvements on Tree / Accordion / ChatMessage / CommandPalette — proving
   the verdict-cache mechanism IS doing what codex described, just on too narrow a set.
3. The five-component regression on the codex-flagged set (ChatMessages, InputMenu, SelectMenu, Select, Listbox) that did NOT respond to the verdict cache.
4. The systemic regression across ~165 components that don't mint synthetic carriers,
   pointing to a yet-unidentified per-component overhead introduced by the R22
   substrate (carrier_provenance field on ExpandedField/ExpandedProperty? eager
   admission ahead of every component-meta query? Something else?).
5. Codex's named ALTERNATIVE-IF-OPTION-FAILS-EMPIRICALLY:
   > Promote the carrier to an explicit typed-IR variant or attach the graph
   > `SemanticNodeId` to `ExpandedField`, then route all later materialisation
   > through `ShapeCacheDb::semantic_node_whole` instead of string-name lookup.

The R22 work has produced a strong producer-side substrate (CarrierProvenance
+ CarrierVerdictDb + producer-eager admission), exercised by the discriminating
tests under T1/T2/T3 plus the existing R21.5 shallow-publication test. The
gating regression is empirically real and reproducible across two runs. Next
step: codex re-consult with the data above, deciding between (a) accepting the
substrate and bisecting the systemic regression, (b) reverting R22 in favour
of the typed-IR-variant ALTERNATIVE, or (c) a hybrid that keeps the substrate
but adds a missing piece (e.g. consumer-side cache for the on-demand
re-resolutions consumers now perform).

## Pre-existing test failures (unchanged by R22)

Same 8 as on baseline `68daa4485`:

```
meta::meta_tests::evaluate_types_hydrates_transitive_imported_pick_dependencies_from_dual_script_vue_deps
project_semantic_dispatch::tests::class_a_invisibility_mapped_pick_two_keys_unchanged
chain_y_routed_surface_demand_split_does_not_leak_inherited_library_members
chain_v_generic_carrier_does_not_leak_inherited_library_members_through_per_prop_publication
lib_parity::pick_and_my_pick_produce_identical_props
correctness_snapshot_for_every_fixture
fixture_14_partial_structural_assertion, fixture_15_required_structural_assertion
audit_ts_bindings_are_in_sync
```

`audit_ts_bindings_are_in_sync` regenerates `packages/types/audit.generated.ts`
on each run — that file is intentionally NOT staged in any R22 commit per the
brief's standing instruction.
