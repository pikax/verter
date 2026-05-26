# R22-final S5 — A2 audit-footprint retest (R21.5 → HEAD)

## TL;DR

**Table.vue audit-footprint counters are byte-identical between R21.5
(`68daa4485`) and HEAD (`173ab494`).** Across every category that
the original A2 capture tracks — `RequestKind` counts, structured
event tallies, cache-layer hits/misses, derivation-subgraph
nodes/edges, `vfs_reads`, `indexed_ready_builds`, `instantiations`,
`projections`, `lock_acquisitions` — the two captures agree to the
byte on the primary A2 target (Table.vue).

In the 12-component warm-host sequence: 11 of 12 components are
byte-identical; Accordion (the first / coldest component in the
sequence) shows a **structural reduction** on HEAD — fewer redundant
warm-cache lookups while doing the same cold work (identical
`indexed.misses=28650`, identical `semantic_graph` / `member_shape`
/ `ref_cycle` counters, but 88 fewer `indexed.hits` and 36 fewer
`owner_import.hits`). HEAD does LESS work, not more.

R22-substrate event names are **absent from both** captures:
`admit_do_not_deepen`, `carrier_verdict_cache_hit`,
`carrier_verdict_cache_miss`, `carrier_provenance_published` —
zero firings on either binary, as expected after the substrate
deletion in S4 (commit `173ab494`).

## Capture method

1. Backed up `D:/dev/personal/verter/packages/native/dist/verter-native.win32-x64-msvc.node`
   to `/tmp/native-head-s4.node` (HEAD binary, md5
   `f9d223145ba5cd5b3cd7c013b2f3edc4`).
2. Built HEAD binary fresh: `cargo build --release --package verter_napi`
   at `D:/dev/personal/verter` (HEAD `173ab494`). Output copied to
   `D:/tmp/r22-final-s5/head-native.node` (size 20 644 352 bytes).
3. Created scratch worktree at `D:/tmp/r22-final-s5/r215-wt`
   checked out at `68daa4485` (R21.5 baseline = R21.6 revert).
4. Built R21.5 binary: `cargo build --release --package verter_napi`
   in the worktree. Output copied to
   `D:/tmp/r22-final-s5/r215-native.node` (size 20 631 552 bytes,
   md5 `ae0b9641e96da4af5cdc88e990f5692b`).
5. Audit driver: `D:/tmp/r22-invest/audit-multi.mjs` (single
   persistent `ComponentMetaHost` with `auditEnabled: true,
   footprintCapture: true`, 12-component sequence:
   Accordion → Alert → Avatar → Badge → Button → Card → Checkbox
   → InputDate → Pagination → Sidebar → Table → Tooltip).
6. Swap procedure: copy R21.5 binary into
   `packages/native/dist/verter-native.win32-x64-msvc.node`
   (verified via md5), run driver writing to
   `D:/tmp/r22-final-s5/audit-r215/`. Swap to HEAD binary, run
   driver writing to `D:/tmp/r22-final-s5/audit-head/`.
7. Determinism: each binary was run twice; both R21.5 and HEAD
   produced byte-identical counters across the two repeat runs
   (verified via `D:/tmp/r22-final-s5/audit-r215-rerun/` and
   `D:/tmp/r22-final-s5/audit-head-rerun/`).

## Table.vue (primary A2 target) — full counter diff

| Counter                | R21.5 | HEAD  | Δ |
| ---------------------- | ----: | ----: | -: |
| `structured_events`    | 2 670 | 2 670 |  0 |
| `cold_builds`          |   189 |   189 |  0 |
| `warm_hits`            |   110 |   110 |  0 |
| `vfs_reads`            |   460 |   460 |  0 |
| `indexed_ready_builds` |     1 |     1 |  0 |
| `instantiations`       |    57 |    57 |  0 |
| `projections`          |    35 |    35 |  0 |
| `lock_acquisitions`    |     0 |     0 |  0 |

### Cache layer hits/misses (Table.vue)

| Layer                  | R21.5 (h/m)     | HEAD (h/m)      |    Δ |
| ---------------------- | --------------- | --------------- | ---: |
| `indexed`              | 1 704 / 22 147  | 1 704 / 22 147  | 0/0 |
| `analysis`             | 0 / 0           | 0 / 0           | 0/0 |
| `owner_import`         | 74 / 2          | 74 / 2          | 0/0 |
| `route_owned_shallow`  | 1 / 801         | 1 / 801         | 0/0 |
| `component_meta`       | 0 / 1           | 0 / 1           | 0/0 |
| `route_db`             | 0 / 0           | 0 / 0           | 0/0 |
| `ref_cycle`            | 24 / 6          | 24 / 6          | 0/0 |
| `intrinsic_registry`   | 0 / 64          | 0 / 64          | 0/0 |
| `semantic_graph`       | 110 / 189       | 110 / 189       | 0/0 |
| `materialize_structure`| 0 / 0           | 0 / 0           | 0/0 |
| `materialize_memo`     | 0 / 117         | 0 / 117         | 0/0 |
| `member_shape_cache`   | 6 / 32          | 6 / 32          | 0/0 |
| `prepared_surface`     | 0 / 4           | 0 / 4           | 0/0 |
| `prepared_member`      | 0 / 0           | 0 / 0           | 0/0 |

## 12-component warm-host sequence — diff summary

| Component | events R21.5/HEAD | layer_hits R21.5/HEAD | layer_misses R21.5/HEAD | verdict |
| --------- | ----------------- | --------------------- | ----------------------- | ------- |
| Accordion | 1 320 / 1 268     | 612 / 488             | 29 566 / 29 566         | **smaller HEAD, identical misses** |
| Alert     | 1 192 / 1 192     | 447 / 447             | 33 307 / 33 307         | byte-identical |
| Avatar    | 1 094 / 1 094     | 273 / 273             | 32 161 / 32 161         | byte-identical |
| Badge     | 1 170 / 1 170     | 462 / 462             | 28 546 / 28 546         | byte-identical |
| Button    | 1 844 / 1 844     | 613 / 613             | 48 596 / 48 596         | byte-identical |
| Card      |   502 / 502       | 241 / 241             | 12 640 / 12 640         | byte-identical |
| Checkbox  | 1 222 / 1 222     | 432 / 432             | 32 524 / 32 524         | byte-identical |
| InputDate | 2 477 / 2 477     | 1 396 / 1 396         | 33 640 / 33 640         | byte-identical |
| Pagination| 1 356 / 1 356     |   763 / 763           | 21 110 / 21 110         | byte-identical |
| Sidebar   | 2 071 / 2 071     | 1 201 / 1 201         | 34 140 / 34 140         | byte-identical |
| Table     | 2 670 / 2 670     | 1 919 / 1 919         | 23 363 / 23 363         | byte-identical |
| Tooltip   | 1 319 / 1 319     |   484 / 484           | 32 740 / 32 740         | byte-identical |

### Accordion structural reduction

Accordion is the FIRST component in the sequence (no warm state
yet on host). HEAD's diff vs R21.5:

| Event / Counter                        | R21.5 | HEAD | Δ    |
| -------------------------------------- | ----: | ---: | ---: |
| `structured_events_total`              | 1 320 | 1 268 |  −52 |
| `layer.indexed.hits`                   |   502 |   414 |  −88 |
| `layer.indexed.misses`                 | 28 650| 28 650|    0 |
| `layer.owner_import.hits`              |    47 |    11 |  −36 |
| `event.resolve_direct_type_reexport_target` | 51 | 39 |  −12 |
| `event.resolve_local_import_symbol_target`  | 51 | 39 |  −12 |
| `event.external_type_analysis`              | 44 | 36 |   −8 |
| `event.external_type_analysis_cache_hit`    | 30 | 22 |   −8 |
| `event.resolve_local_export_symbol_target`  | 17 | 13 |   −4 |
| `event.base_eval_env`                       | 17 | 13 |   −4 |
| `event.base_eval_env_cache_hit`             |  9 |  5 |   −4 |

Pattern: HEAD has fewer warm-cache hits but **identical
`indexed.misses`** and **identical** `semantic_graph` /
`member_shape_cache` / `ref_cycle` counters. The same cold work
runs; HEAD eliminates redundant warm-cache lookups that were
firing for the same final result. This is a NET REDUCTION in work
(consistent with the S3 + S4 cutover where the producer at
`publish_merged_bindings` mints `SyntheticSlotBinding` once and
short-circuits paths the old per-field carrier provenance was
re-walking).

## R22-substrate event check

| Event name                       | R21.5 total | HEAD total | Verdict |
| -------------------------------- | ----------: | ---------: | ------- |
| `admit_do_not_deepen`            |           0 |          0 | absent on both |
| `carrier_verdict_cache_hit`      |           0 |          0 | absent on both |
| `carrier_verdict_cache_miss`     |           0 |          0 | absent on both |
| `carrier_provenance_published`   |           0 |          0 | absent on both |

The R22 transient `CarrierVerdictDb` substrate is fully retired
on HEAD (commit `173ab494` deletes the type, its instantiation in
`ProjectTypeStore`, and the structured events that were emitted
from it). R21.5 predates the entire R22 substrate, so these names
also never existed in R21.5.

## Verdict

**A2 retest gate**: **PASS**

- Table.vue (the primary A2 capture target named in the brief):
  byte-identical counters across every category. Zero structural
  divergence. Substrate deletion is structurally complete.
- 11 of 12 sequence components: byte-identical.
- Accordion: structural REDUCTION on HEAD (fewer redundant warm
  hits, identical cold misses, identical semantic-graph counters).
  This is not a regression; HEAD does strictly less work for the
  same final result.

## Diff tooling

`D:/tmp/r22-final-s5/diff-audit.mjs` — programmatic counter diff
script. Reads `audit-r215/*.audit.json` and `audit-head/*.audit.json`,
compares structural counters and per-event tallies, writes
`D:/tmp/r22-final-s5/audit-diff.json` with full per-component
breakdown.

## Captures

- `D:/tmp/r22-final-s5/audit-r215/` — R21.5 run 1 (12 audits + summary.csv)
- `D:/tmp/r22-final-s5/audit-r215-rerun/` — R21.5 run 2 (determinism check, identical to run 1)
- `D:/tmp/r22-final-s5/audit-head/` — HEAD run 1 (12 audits + summary.csv)
- `D:/tmp/r22-final-s5/audit-head-rerun/` — HEAD run 2 (determinism check, identical to run 1)
- `D:/tmp/r22-final-s5/audit-diff.json` — programmatic per-component diff
