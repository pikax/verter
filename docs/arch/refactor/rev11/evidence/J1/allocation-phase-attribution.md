# J1 §2 Allocation ceiling — phase attribution (measurement, not optimization)

> **Donor record — NOT reproducible against this tree.** This study was captured on the
> `block/css-closing-items` donor worktree, which the ownership ruling made a staging/evidence
> donor rather than a landing branch. It is retained for its BASELINE numbers, which the
> allocation predecessors are measured against.
>
> The symbols and commands it names do not exist at this SHA and must not be run from here:
> `allocation_ratio_bound`, `style_planner::{set_phase_probe, clear_phase_probe}`, and the
> `phase_attribution` test were donor-local. The live successor harness is
> `crates/verter_compiler/tests/allocator_canaries.rs::intra_parser_attribution`, run with
> `cargo nextest run -p verter_compiler --test allocator_canaries --no-capture`, and its results
> are in [`intra-parser-allocation.md`](intra-parser-allocation.md).
>
> Read every "is still", "is untouched", and "does not fix anything" below as a statement about
> the donor tree at capture time, not about this one.

Captured per the resolution gate in
[`allocation-ceiling-defer-ruling.md`](allocation-ceiling-defer-ruling.md): every one of the 11
`css_bench.rs` generator categories misses the ratified 1.2x ceiling
(`crates/verter_compiler/tests/allocator_canaries.rs::allocation_ratio_bound`, still `#[ignore]`d
and un-weakened). Parse-reuse (A10i/A24) is already fixed and did not close the gap. This document
attributes the residual allocation delta to a named phase, in both COUNT and BYTES, so the correct
owner can fix it. **This document does not fix anything** — no production behavior changes here
beyond the measurement hooks themselves (see "Instrumentation" below), and the discriminating
`allocation_ratio_bound` canary is untouched and still fails exactly as before.

## Instrumentation

Both pipelines gained an always-compiled (`#[cfg(test)]`-free, same pattern as the existing
`parse_ir_invocation_count`/`build_string_invocation_count` hooks), thread-local, test-only
phase-boundary probe:

- `verter_compiler::style_planner::{set_phase_probe, clear_phase_probe}` — brackets `parse`
  (`parse_ir`, includes `CssSource::new`'s `Arc::from(code)` copy), `planner` (each of
  `v_bind_edits_from_ir` / `module_classes_and_edits_from_ir` / `scoped_edits_and_facts_from_ir`),
  `cascade_glue` (the `.to_string()` hand-off between cascade stages, `style_planner.rs:1169-1174`),
  `codetransform` (`CodeTransform::new` + edit application, `emit`), `sourcemap`
  (`generate_map().to_json_string()`, skipped entirely when `want_source_map` is `false`, which is
  how this capture and the `allocation_ratio_bound` canary both run), `build_string`, and
  `output_copy` (`RuntimeOutputDescriptor::generated` — includes a SHA-256 content hash over the
  full output, `crates/verter_compiler/src/framework_common/carrier_compiler.rs:161`, that the
  legacy pipeline does not pay at all).
- `verter_compiler::css::{set_phase_probe, clear_phase_probe}` — brackets `prepass`, `normalize`
  (lightningcss `StyleSheet::parse` + `to_css` reserialize), `modules`, `scoped`. The legacy
  pipeline has no CodeTransform/sourcemap/build_string/output-descriptor machinery at all — it is a
  parse-once-then-string-splice architecture (`css/scoped.rs`, `css/modules.rs`), not an
  edit-list-over-arena architecture, so the phase vocabularies do not line up 1:1. Its source-map
  cost is unconditionally zero: `ProcessStyleResult.source_map` is hardcoded `None` regardless of
  the `sourcemap` option (`css/mod.rs:110,145`, `// TODO: source map support`).

The probe itself is supplied by the observing test (`crates/verter_compiler/tests/allocator_canaries.rs`,
`mod phase_attribution`) — the library modules only call a `Box<dyn FnMut(&'static str)>` with a
static marker name; the counting-allocator snapshot/diff logic lives entirely in the test binary,
which already owns the crate's one allowlisted separate `#[global_allocator]`
(`docs/arch/refactor/rev11/...` anti-binary-growth layout rule; `verter_compiler`'s
`allocator_canaries` target is the named exception). The counting allocator itself was extended to
track cumulative **requested bytes** (`Layout::size()`, or `new_size` for `realloc`) alongside the
existing call count, so this capture separates "many tiny allocations" from "a few large copies" —
those are opposite fixes.

The `parse` phase is further split by ordinal, using the existing
`style_planner::parse_ir_invocation_count()` counter: call #1 is `parse:initial` (the legitimate
first parse), every call after it is `parse:reparse` (a required new-identity reparse — the content
changed underneath the next stage, A10i).

This instrumentation is diagnostic-only: `mod phase_attribution` asserts only that each pipeline
allocated something and that the probes attributed at least one phase (a non-discriminating sanity
check, not a ceiling) — it never asserts a ratio or a ceiling, and it does not touch
`allocation_ratio_bound`.

Run: `cargo nextest run -p verter_compiler --test allocator_canaries phase_attribution --no-capture`
(`cargo test -p verter_compiler --test allocator_canaries phase_attribution -- --nocapture` works
identically). The WIP commit that introduced this module did not compile
(`PhaseTotals` was moved into a `BTreeMap` inside `Rc::try_unwrap(..).unwrap()`, which requires
`Debug` on the panic path, and `PhaseTotals` did not derive it) — fixed by adding
`#[derive(Debug, ...)]` to `PhaseTotals`, one line, no behavior change. All 11
`phase_attribution::*` tests then pass.

## Results

All figures below are one representative measured run (dev profile, single process, warm
lazy-init already paid before the reset per `measure_converged`/`measure_legacy`, `sourcemap`
option `false` for both pipelines — the same configuration `allocation_ratio_bound` uses). The
count-ratio column reproduces the existing canary's documented 1.99x–4.09x range almost exactly
(1.98x–4.08x here; the residual is ordinary run-to-run noise, not a discrepancy), which is the
cross-check that this instrumentation is measuring the same thing the ratified canary measures.

### Per-category totals (legacy vs. converged)

| category | legacy calls / bytes | converged calls / bytes | count ratio | **bytes ratio** |
|---|---|---|---|---|
| class_rules | 425 / 110,348 | 843 / 336,361 | 1.98x | 3.05x |
| descendant_selectors | 374 / 107,539 | 943 / 378,357 | 2.52x | 3.52x |
| pseudo_selectors | 374 / 105,878 | 793 / 334,347 | 2.12x | 3.16x |
| selector_lists | 825 / 156,998 | 1798 / 675,581 | 2.18x | 4.30x |
| v_bind_rules | 932 / 152,609 | 1885 / 672,821 | 2.02x | 4.41x |
| v_bind_dotted | 932 / 162,349 | 1885 / 692,765 | 2.02x | 4.27x |
| deep_rules | 525 / 114,722 | 1593 / 576,309 | 3.03x | 5.02x |
| slotted_rules | 475 / 111,604 | 1843 / 1,711,525 | 3.88x | **15.34x** |
| mixed_vue | 651 / 132,962 | 2656 / 1,343,546 | 4.08x | **10.10x** |
| global_rules | 373 / 117,458 | 1493 / 573,303 | 4.00x | 4.88x |
| repeated_classes | 374 / 102,928 | 794 / 326,141 | 2.12x | 3.17x |
| **aggregate (sum of all 11)** | **6260 / 1,375,395** | **16526 / 7,621,056** | **2.64x** | **5.54x** |

The count ratio and the bytes ratio diverge sharply for two categories (`slotted_rules`,
`mixed_vue`) — proof, not just an a-priori possibility, that count and bytes are answering
different questions here and need different fixes (see Analysis).

### Converged-pipeline phase breakdown (calls / bytes, per category)

| category | parse:initial | parse:reparse | planner | codetransform | cascade_glue | build_string | output_copy | sourcemap |
|---|---|---|---|---|---|---|---|---|
| class_rules | 617/284,088 | — | 207/15,090 | 3/31,344 | 1/2,029 | 1/2,879 | 12/643 | 0/0 |
| descendant_selectors | 717/327,536 | — | 207/15,090 | 3/30,192 | 1/1,879 | 1/2,729 | 12/643 | 0/0 |
| pseudo_selectors | 617/286,472 | — | 157/14,700 | 3/28,176 | 1/1,609 | 1/2,459 | 12/643 | 0/0 |
| selector_lists | 1168/557,896 | — | 609/50,630 | 4/52,656 | 1/2,459 | 1/5,009 | 12/643 | 0/0 |
| v_bind_rules | 617/283,888 | 615/283,464 | 617/33,494 | 6/62,592 | 1/2,229 | 2/5,308 | 24/1,286 | 0/0 |
| v_bind_dotted | 617/284,736 | 615/284,216 | 617/37,684 | 6/74,496 | 1/2,979 | 2/6,808 | 24/1,286 | 0/0 |
| deep_rules | 1317/526,896 | — | 257/16,190 | 3/28,464 | 1/1,639 | 1/2,189 | 12/643 | 0/0 |
| slotted_rules | 1317/527,000 | — | 507/**1,150,530** | 3/29,136 | 1/1,739 | 1/2,189 | 12/643 | 0/0 |
| mixed_vue | 1079/444,384 | 1077/443,696 | 464/**386,772** | 6/60,384 | 1/1,948 | 2/4,516 | 24/1,286 | 0/0 |
| global_rules | 1317/526,952 | — | 157/13,740 | 3/28,752 | 1/1,689 | 1/1,239 | 12/643 | 0/0 |
| repeated_classes | 567/272,112 | — | 207/14,950 | 4/34,800 | 1/1,249 | 1/2,099 | 12/643 | 0/0 |

`sourcemap` is 0/0 in every category by construction (`want_source_map=false`, matching how
`allocation_ratio_bound` runs both pipelines — this instrumentation cannot and does not speak to
the sourcemap-on cost). `UNATTRIBUTED` (warm-init / probe-boundary noise not bracketed by any
marker) is 2–3 calls and ≤6,288 bytes per category out of totals in the hundreds/hundred-thousands
— immaterial, not a hidden phase.

### Aggregate phase share (converged pipeline, summed over all 11 categories)

| phase | calls | % of total calls | bytes | % of total bytes |
|---|---|---|---|---|
| `parse:initial` | 9,950 | 60.2% | 4,321,960 | 56.7% |
| `planner` | 4,006 | 24.2% | 1,748,870 | 22.9% |
| `parse:reparse` | 2,307 | 14.0% | 1,011,376 | 13.3% |
| `output_copy` | 168 | 1.0% | 9,002 | 0.1% |
| `codetransform` | 44 | 0.3% | 460,992 | 6.0% |
| `build_string` | 14 | 0.1% | 37,424 | 0.5% |
| `cascade_glue` | 11 | 0.1% | 21,448 | 0.3% |
| `sourcemap` | 0 | 0.0% | 0 | 0.0% |

`codetransform`'s share inverts between the two units — 0.3% of calls but 6.0% of bytes, i.e. very
few but very large allocations (see Analysis: `Allocator::new()`). Legacy's own phase split
(`normalize` 38.0%/83.0%, `scoped` 39.0%/10.6%, `prepass` 22.9%/6.4% of its own, much smaller,
totals) is included for the same reason in the raw output below but is not separately tabulated
here — the two pipelines' phase vocabularies do not line up 1:1 (see Instrumentation).

## Raw output

```
$ cargo nextest run -p verter_compiler --test allocator_canaries phase_attribution --no-capture

J1_PHASE[class_rules][legacy][normalize] calls=218 bytes=96561 invocations=1
J1_PHASE[class_rules][legacy][prepass] calls=2 bytes=824 invocations=1
J1_PHASE[class_rules][legacy][scoped] calls=204 bytes=12947 invocations=1
J1_PHASE[class_rules][legacy][TOTAL] calls=425 bytes=110348
J1_PHASE[class_rules][legacy][UNATTRIBUTED] calls=1 bytes=16
J1_PHASE[class_rules][converged][build_string] calls=1 bytes=2879 invocations=1
J1_PHASE[class_rules][converged][cascade_glue] calls=1 bytes=2029 invocations=1
J1_PHASE[class_rules][converged][codetransform] calls=3 bytes=31344 invocations=1
J1_PHASE[class_rules][converged][output_copy] calls=12 bytes=643 invocations=1
J1_PHASE[class_rules][converged][parse:initial] calls=617 bytes=284088 invocations=1
J1_PHASE[class_rules][converged][planner] calls=207 bytes=15090 invocations=2
J1_PHASE[class_rules][converged][sourcemap] calls=0 bytes=0 invocations=1
J1_PHASE[class_rules][converged][TOTAL] calls=843 bytes=336361
J1_PHASE[class_rules][converged][UNATTRIBUTED] calls=2 bytes=288
J1_PHASE[deep_rules][legacy][normalize] calls=167 bytes=93898 invocations=1
J1_PHASE[deep_rules][legacy][prepass] calls=154 bytes=11125 invocations=1
J1_PHASE[deep_rules][legacy][scoped] calls=203 bytes=9683 invocations=1
J1_PHASE[deep_rules][legacy][TOTAL] calls=525 bytes=114722
J1_PHASE[deep_rules][legacy][UNATTRIBUTED] calls=1 bytes=16
J1_PHASE[deep_rules][converged][build_string] calls=1 bytes=2189 invocations=1
J1_PHASE[deep_rules][converged][cascade_glue] calls=1 bytes=1639 invocations=1
J1_PHASE[deep_rules][converged][codetransform] calls=3 bytes=28464 invocations=1
J1_PHASE[deep_rules][converged][output_copy] calls=12 bytes=643 invocations=1
J1_PHASE[deep_rules][converged][parse:initial] calls=1317 bytes=526896 invocations=1
J1_PHASE[deep_rules][converged][planner] calls=257 bytes=16190 invocations=2
J1_PHASE[deep_rules][converged][sourcemap] calls=0 bytes=0 invocations=1
J1_PHASE[deep_rules][converged][TOTAL] calls=1593 bytes=576309
J1_PHASE[deep_rules][converged][UNATTRIBUTED] calls=2 bytes=288
J1_PHASE[descendant_selectors][legacy][normalize] calls=167 bytes=92876 invocations=1
J1_PHASE[descendant_selectors][legacy][prepass] calls=2 bytes=824 invocations=1
J1_PHASE[descendant_selectors][legacy][scoped] calls=204 bytes=13823 invocations=1
J1_PHASE[descendant_selectors][legacy][TOTAL] calls=374 bytes=107539
J1_PHASE[descendant_selectors][legacy][UNATTRIBUTED] calls=1 bytes=16
J1_PHASE[descendant_selectors][converged][build_string] calls=1 bytes=2729 invocations=1
J1_PHASE[descendant_selectors][converged][cascade_glue] calls=1 bytes=1879 invocations=1
J1_PHASE[descendant_selectors][converged][codetransform] calls=3 bytes=30192 invocations=1
J1_PHASE[descendant_selectors][converged][output_copy] calls=12 bytes=643 invocations=1
J1_PHASE[descendant_selectors][converged][parse:initial] calls=717 bytes=327536 invocations=1
J1_PHASE[descendant_selectors][converged][planner] calls=207 bytes=15090 invocations=2
J1_PHASE[descendant_selectors][converged][sourcemap] calls=0 bytes=0 invocations=1
J1_PHASE[descendant_selectors][converged][TOTAL] calls=943 bytes=378357
J1_PHASE[descendant_selectors][converged][UNATTRIBUTED] calls=2 bytes=288
J1_PHASE[global_rules][legacy][normalize] calls=217 bytes=109965 invocations=1
J1_PHASE[global_rules][legacy][prepass] calls=2 bytes=824 invocations=1
J1_PHASE[global_rules][legacy][scoped] calls=153 bytes=6653 invocations=1
J1_PHASE[global_rules][legacy][TOTAL] calls=373 bytes=117458
J1_PHASE[global_rules][legacy][UNATTRIBUTED] calls=1 bytes=16
J1_PHASE[global_rules][converged][build_string] calls=1 bytes=1239 invocations=1
J1_PHASE[global_rules][converged][cascade_glue] calls=1 bytes=1689 invocations=1
J1_PHASE[global_rules][converged][codetransform] calls=3 bytes=28752 invocations=1
J1_PHASE[global_rules][converged][output_copy] calls=12 bytes=643 invocations=1
J1_PHASE[global_rules][converged][parse:initial] calls=1317 bytes=526952 invocations=1
J1_PHASE[global_rules][converged][planner] calls=157 bytes=13740 invocations=2
J1_PHASE[global_rules][converged][sourcemap] calls=0 bytes=0 invocations=1
J1_PHASE[global_rules][converged][TOTAL] calls=1493 bytes=573303
J1_PHASE[global_rules][converged][UNATTRIBUTED] calls=2 bytes=288
J1_PHASE[mixed_vue][legacy][normalize] calls=202 bytes=103941 invocations=1
J1_PHASE[mixed_vue][legacy][prepass] calls=244 bytes=14723 invocations=1
J1_PHASE[mixed_vue][legacy][scoped] calls=204 bytes=14282 invocations=1
J1_PHASE[mixed_vue][legacy][TOTAL] calls=651 bytes=132962
J1_PHASE[mixed_vue][legacy][UNATTRIBUTED] calls=1 bytes=16
J1_PHASE[mixed_vue][converged][build_string] calls=2 bytes=4516 invocations=2
J1_PHASE[mixed_vue][converged][cascade_glue] calls=1 bytes=1948 invocations=1
J1_PHASE[mixed_vue][converged][codetransform] calls=6 bytes=60384 invocations=2
J1_PHASE[mixed_vue][converged][output_copy] calls=24 bytes=1286 invocations=2
J1_PHASE[mixed_vue][converged][parse:initial] calls=1079 bytes=444384 invocations=1
J1_PHASE[mixed_vue][converged][parse:reparse] calls=1077 bytes=443696 invocations=1
J1_PHASE[mixed_vue][converged][planner] calls=464 bytes=386772 invocations=2
J1_PHASE[mixed_vue][converged][sourcemap] calls=0 bytes=0 invocations=2
J1_PHASE[mixed_vue][converged][TOTAL] calls=2656 bytes=1343546
J1_PHASE[mixed_vue][converged][UNATTRIBUTED] calls=3 bytes=560
J1_PHASE[pseudo_selectors][legacy][normalize] calls=167 bytes=92365 invocations=1
J1_PHASE[pseudo_selectors][legacy][prepass] calls=2 bytes=824 invocations=1
J1_PHASE[pseudo_selectors][legacy][scoped] calls=204 bytes=12673 invocations=1
J1_PHASE[pseudo_selectors][legacy][TOTAL] calls=374 bytes=105878
J1_PHASE[pseudo_selectors][legacy][UNATTRIBUTED] calls=1 bytes=16
J1_PHASE[pseudo_selectors][converged][build_string] calls=1 bytes=2459 invocations=1
J1_PHASE[pseudo_selectors][converged][cascade_glue] calls=1 bytes=1609 invocations=1
J1_PHASE[pseudo_selectors][converged][codetransform] calls=3 bytes=28176 invocations=1
J1_PHASE[pseudo_selectors][converged][output_copy] calls=12 bytes=643 invocations=1
J1_PHASE[pseudo_selectors][converged][parse:initial] calls=617 bytes=286472 invocations=1
J1_PHASE[pseudo_selectors][converged][planner] calls=157 bytes=14700 invocations=2
J1_PHASE[pseudo_selectors][converged][sourcemap] calls=0 bytes=0 invocations=1
J1_PHASE[pseudo_selectors][converged][TOTAL] calls=793 bytes=334347
J1_PHASE[pseudo_selectors][converged][UNATTRIBUTED] calls=2 bytes=288
J1_PHASE[repeated_classes][legacy][normalize] calls=167 bytes=92365 invocations=1
J1_PHASE[repeated_classes][legacy][prepass] calls=2 bytes=824 invocations=1
J1_PHASE[repeated_classes][legacy][scoped] calls=204 bytes=9723 invocations=1
J1_PHASE[repeated_classes][legacy][TOTAL] calls=374 bytes=102928
J1_PHASE[repeated_classes][legacy][UNATTRIBUTED] calls=1 bytes=16
J1_PHASE[repeated_classes][converged][build_string] calls=1 bytes=2099 invocations=1
J1_PHASE[repeated_classes][converged][cascade_glue] calls=1 bytes=1249 invocations=1
J1_PHASE[repeated_classes][converged][codetransform] calls=4 bytes=34800 invocations=1
J1_PHASE[repeated_classes][converged][output_copy] calls=12 bytes=643 invocations=1
J1_PHASE[repeated_classes][converged][parse:initial] calls=567 bytes=272112 invocations=1
J1_PHASE[repeated_classes][converged][planner] calls=207 bytes=14950 invocations=2
J1_PHASE[repeated_classes][converged][sourcemap] calls=0 bytes=0 invocations=1
J1_PHASE[repeated_classes][converged][TOTAL] calls=794 bytes=326141
J1_PHASE[repeated_classes][converged][UNATTRIBUTED] calls=2 bytes=288
J1_PHASE[selector_lists][legacy][normalize] calls=368 bytes=128461 invocations=1
J1_PHASE[selector_lists][legacy][prepass] calls=2 bytes=824 invocations=1
J1_PHASE[selector_lists][legacy][scoped] calls=454 bytes=27697 invocations=1
J1_PHASE[selector_lists][legacy][TOTAL] calls=825 bytes=156998
J1_PHASE[selector_lists][legacy][UNATTRIBUTED] calls=1 bytes=16
J1_PHASE[selector_lists][converged][build_string] calls=1 bytes=5009 invocations=1
J1_PHASE[selector_lists][converged][cascade_glue] calls=1 bytes=2459 invocations=1
J1_PHASE[selector_lists][converged][codetransform] calls=4 bytes=52656 invocations=1
J1_PHASE[selector_lists][converged][output_copy] calls=12 bytes=643 invocations=1
J1_PHASE[selector_lists][converged][parse:initial] calls=1168 bytes=557896 invocations=1
J1_PHASE[selector_lists][converged][planner] calls=609 bytes=50630 invocations=2
J1_PHASE[selector_lists][converged][sourcemap] calls=0 bytes=0 invocations=1
J1_PHASE[selector_lists][converged][TOTAL] calls=1798 bytes=675581
J1_PHASE[selector_lists][converged][UNATTRIBUTED] calls=3 bytes=6288
J1_PHASE[slotted_rules][legacy][normalize] calls=167 bytes=93640 invocations=1
J1_PHASE[slotted_rules][legacy][prepass] calls=104 bytes=7965 invocations=1
J1_PHASE[slotted_rules][legacy][scoped] calls=203 bytes=9983 invocations=1
J1_PHASE[slotted_rules][legacy][TOTAL] calls=475 bytes=111604
J1_PHASE[slotted_rules][legacy][UNATTRIBUTED] calls=1 bytes=16
J1_PHASE[slotted_rules][converged][build_string] calls=1 bytes=2189 invocations=1
J1_PHASE[slotted_rules][converged][cascade_glue] calls=1 bytes=1739 invocations=1
J1_PHASE[slotted_rules][converged][codetransform] calls=3 bytes=29136 invocations=1
J1_PHASE[slotted_rules][converged][output_copy] calls=12 bytes=643 invocations=1
J1_PHASE[slotted_rules][converged][parse:initial] calls=1317 bytes=527000 invocations=1
J1_PHASE[slotted_rules][converged][planner] calls=507 bytes=1150530 invocations=2
J1_PHASE[slotted_rules][converged][sourcemap] calls=0 bytes=0 invocations=1
J1_PHASE[slotted_rules][converged][TOTAL] calls=1843 bytes=1711525
J1_PHASE[slotted_rules][converged][UNATTRIBUTED] calls=2 bytes=288
J1_PHASE[v_bind_dotted][legacy][normalize] calls=268 bytes=118461 invocations=1
J1_PHASE[v_bind_dotted][legacy][prepass] calls=459 bytes=28569 invocations=1
J1_PHASE[v_bind_dotted][legacy][scoped] calls=204 bytes=15303 invocations=1
J1_PHASE[v_bind_dotted][legacy][TOTAL] calls=932 bytes=162349
J1_PHASE[v_bind_dotted][legacy][UNATTRIBUTED] calls=1 bytes=16
J1_PHASE[v_bind_dotted][converged][build_string] calls=2 bytes=6808 invocations=2
J1_PHASE[v_bind_dotted][converged][cascade_glue] calls=1 bytes=2979 invocations=1
J1_PHASE[v_bind_dotted][converged][codetransform] calls=6 bytes=74496 invocations=2
J1_PHASE[v_bind_dotted][converged][output_copy] calls=24 bytes=1286 invocations=2
J1_PHASE[v_bind_dotted][converged][parse:initial] calls=617 bytes=284736 invocations=1
J1_PHASE[v_bind_dotted][converged][parse:reparse] calls=615 bytes=284216 invocations=1
J1_PHASE[v_bind_dotted][converged][planner] calls=617 bytes=37684 invocations=2
J1_PHASE[v_bind_dotted][converged][sourcemap] calls=0 bytes=0 invocations=2
J1_PHASE[v_bind_dotted][converged][TOTAL] calls=1885 bytes=692765
J1_PHASE[v_bind_dotted][converged][UNATTRIBUTED] calls=3 bytes=560
J1_PHASE[v_bind_rules][legacy][normalize] calls=268 bytes=118461 invocations=1
J1_PHASE[v_bind_rules][legacy][prepass] calls=459 bytes=21079 invocations=1
J1_PHASE[v_bind_rules][legacy][scoped] calls=204 bytes=13053 invocations=1
J1_PHASE[v_bind_rules][legacy][TOTAL] calls=932 bytes=152609
J1_PHASE[v_bind_rules][legacy][UNATTRIBUTED] calls=1 bytes=16
J1_PHASE[v_bind_rules][converged][build_string] calls=2 bytes=5308 invocations=2
J1_PHASE[v_bind_rules][converged][cascade_glue] calls=1 bytes=2229 invocations=1
J1_PHASE[v_bind_rules][converged][codetransform] calls=6 bytes=62592 invocations=2
J1_PHASE[v_bind_rules][converged][output_copy] calls=24 bytes=1286 invocations=2
J1_PHASE[v_bind_rules][converged][parse:initial] calls=617 bytes=283888 invocations=1
J1_PHASE[v_bind_rules][converged][parse:reparse] calls=615 bytes=283464 invocations=1
J1_PHASE[v_bind_rules][converged][planner] calls=617 bytes=33494 invocations=2
J1_PHASE[v_bind_rules][converged][sourcemap] calls=0 bytes=0 invocations=2
J1_PHASE[v_bind_rules][converged][TOTAL] calls=1885 bytes=672821
J1_PHASE[v_bind_rules][converged][UNATTRIBUTED] calls=3 bytes=560

Summary [11 tests run: 11 passed, 33 skipped]
```

## Analysis

### Dominant owner #1 (COUNT): the single required parse itself, before any reparse

`parse:initial` is 60.2% of converged allocation **count** and 56.7% of converged **bytes**,
aggregated over all 11 categories — already the largest phase before adding `planner`,
`parse:reparse`, or any codegen stage. It is not close: the second-largest phase (`planner`) is
24.2%/22.9%.

More importantly, `parse:initial` in isolation — the one parse every category is architecturally
required to pay, zero reparses, zero planning, zero codegen — already **exceeds legacy's entire
pipeline total** in 9 of the 11 categories, both in count and in bytes:

| category | parse:initial alone | legacy TOTAL (all phases) | parse:initial vs. legacy TOTAL |
|---|---|---|---|
| class_rules | 617 / 284,088 | 425 / 110,348 | 1.45x count, 2.57x bytes |
| descendant_selectors | 717 / 327,536 | 374 / 107,539 | 1.92x count, 3.05x bytes |
| pseudo_selectors | 617 / 286,472 | 374 / 105,878 | 1.65x count, 2.71x bytes |
| selector_lists | 1168 / 557,896 | 825 / 156,998 | 1.42x count, 3.55x bytes |
| deep_rules | 1317 / 526,896 | 525 / 114,722 | 2.51x count, 4.59x bytes |
| slotted_rules | 1317 / 527,000 | 475 / 111,604 | 2.77x count, 4.72x bytes |
| mixed_vue | 1079 / 444,384 | 651 / 132,962 | 1.66x count, 3.34x bytes |
| global_rules | 1317 / 526,952 | 373 / 117,458 | 3.53x count, 4.49x bytes |
| repeated_classes | 567 / 272,112 | 374 / 102,928 | 1.52x count, 2.64x bytes |
| v_bind_rules | 617 / 283,888 | 932 / 152,609 | 0.66x count, **1.86x bytes** |
| v_bind_dotted | 617 / 284,736 | 932 / 162,349 | 0.66x count, **1.75x bytes** |

Even in the two categories where `parse:initial`'s *count* trails legacy's total (`v_bind_rules`,
`v_bind_dotted` — legacy's `prepass` phase is unusually call-heavy there, 459 calls scanning for
`v-bind()` occurrences, `css/mod.rs:126-138`), `parse:initial`'s *bytes* still exceed legacy's
entire pipeline.

**Owner:** `style_planner::parse_ir` (`crates/verter_compiler/src/style_planner.rs:293-308`),
which calls `verter_css_syntax::style_ir::parse_style_ir`
(`crates/verter_css_syntax/src/style_ir.rs:935-943`) — the shared carrier-IR CSS parser. This is
architecturally mandated (Carrier Geometry From Registered Facts / the single shared parser
invariant) and is not itself a bug to "route around" — legacy's competing `lightningcss` parse+
`to_css` reserialize (`css/mod.rs:104-107`) is a different, non-shared parser this program is
retiring, not a fairer baseline. But its allocation profile — building `StyleSyntaxIr` — is
measurably 1.4x–3.5x more call-heavy and 1.75x–4.7x more byte-heavy than legacy's *entire*
parse-through-scope-rewrite pipeline, for equivalent input. That gap is real, is not explained by
reparsing (this row excludes every `parse:reparse` call), and is the single largest owner of the
aggregate regression in both units. Closing it is `verter_css_syntax`'s to scope, not
`verter_compiler`'s — outside the "measurement only" mandate of this document, but the ownership
is now named with certainty.

### Dominant owner #2 (BYTES, distinct from #1): `Allocator::new()` per-occurrence bump-arena floor

The bytes ratio and the count ratio disagree sharply for exactly two categories —
`slotted_rules` (3.88x count vs. **15.34x** bytes) and `mixed_vue` (4.08x count vs. **10.10x**
bytes) — and the phase breakdown pins it to one bucket: `slotted_rules`'s `planner` phase alone
carries 1,150,530 of its 1,711,525 converged bytes (67%), for only 507 calls (2 invocations of the
whole phase, 50 generated `:slotted(...)` rules). Every other category's `planner` phase, at the
same 50-rule generator scale, stays in the 13,740–50,630 byte range — `slotted_rules` is **23x–84x**
larger than its peers for the same phase at the same input scale. `mixed_vue` (1/3 of its 50 rules
are `:slotted(...)`) shows the same signature at 1/3 amplitude: 386,772 planner bytes against a
~15k–50k peer range.

**Owner:** `VueScopePlanner::render_special_argument`
(`crates/verter_compiler/src/style_planner.rs:1706-1754`), specifically line 1725:
`let allocator = Allocator::new();` — a fresh `oxc_allocator::Allocator` bump arena, created once
per `:slotted(...)` occurrence (and per `:deep(...)` occurrence when it has edits — see the guard
at line 1717 `if slotted`; the `:deep`/`:global` calls at `style_planner.rs:1595,1608` pass
`slotted=false` and hit the empty-edits early return at line 1721-1723 before reaching
`Allocator::new()`, so they are not charged this cost). This mirrors the *same* pattern one level
up, in the always-paid `codetransform` phase: `emit`'s own `let allocator = Allocator::new();`
(`style_planner.rs:350`) is called once per `emit` invocation to build a `CodeTransform` over the
*entire* file's edits — reasonable in principle, but `codetransform` is 0.3% of aggregate
converged **calls** and 6.0% of aggregate converged **bytes** (44 calls, 460,992 bytes, ~10.5KB/
call average) for input files of a few hundred bytes to a few KB. Both call sites allocate a brand
new bump arena, sized to its own default first-block reservation rather than to the (tiny) edit
list actually being built, and neither arena is pooled or reused across the ≤2 planner
invocations per file or across the N `:slotted`/`:deep` occurrences within one file. For
`slotted_rules`, that is 50 separate arena reservations (roughly 23KB/occurrence measured:
1,150,530 bytes / 50 occurrences) to render 50 one-line selector substitutions.

This is the "opposite fix" the brief anticipated, made concrete: `slotted_rules`'s allocation
**count** regression (3.88x) is unremarkable and explained entirely by owner #1 (parse) plus the
ordinary per-rule planner work every category pays — a fine-grained, many-tiny-allocations story.
Its allocation **bytes** regression (15.34x, the worst of all 11 categories) is a *separate*,
coarse-grained, few-large-copies defect that a count-only ceiling would never surface: an
allocator floor cost paid per `:slotted()`/`:deep()` occurrence, independent of and additive to
owner #1. Fixing owner #1 would not move `slotted_rules`'s bytes ratio; fixing owner #2 would not
move any category's count ratio below roughly 2x. They are separate owners requiring separate
fixes, exactly as the brief warned.

### `parse:reparse` (A10i) — present, small, and already-fixed-scope

Only 3 of 11 categories pay a reparse at all (`mixed_vue`, `v_bind_dotted`, `v_bind_rules` — the
categories whose generated CSS contains `v-bind()`, which forces a second real-content-change
pass per A10i). Where it fires, it costs almost exactly as much as `parse:initial` did (e.g.
`v_bind_rules`: 617/283,888 initial vs. 615/283,464 reparse) — consistent with "one required
additional parse of changed content," not a duplicate/redundant reparse of unchanged content. This
matches the brief's standing finding: parse-reuse (A10i/A24) is already fixed, reparses here are
the legitimate `1 + K` shape, and this is not where the residual gap lives. `parse:reparse` is
14.0%/13.3% of the aggregate converged total — real, but well behind owners #1 and #2, and out of
scope per the standing ruling (do not re-run this work).

### Minor/architectural-tax owners (not worth chasing further)

- **`output_copy`** (`RuntimeOutputDescriptor::generated`,
  `crates/verter_compiler/src/framework_common/carrier_compiler.rs:161`, includes a SHA-256
  content hash): 1.0% of calls, 0.1% of bytes aggregate. Legacy pays zero equivalent (no output
  descriptor, no content hash) — a real, unconditional architecture tax, but too small to matter
  next to owners #1/#2.
- **`build_string`, `cascade_glue`**: <0.2% of calls and <1% of bytes each, aggregate. Not
  material.
- **`sourcemap`**: 0/0 everywhere in this measurement by construction (`want_source_map=false`).
  This document does not speak to the sourcemap-on cost.

### What this does not answer

This attribution does not decompose `parse:initial`'s internal cost further (which
`StyleSyntaxIrSink`/`parse_with_sink` construct is the actual per-token allocator, inside
`verter_css_syntax`) — that is a `verter_css_syntax`-owned investigation, not
`verter_compiler`'s, and is out of the scope named for this document. Owner #1 is named at the
crate/function boundary it crosses (`style_planner::parse_ir` → `verter_css_syntax::style_ir::
parse_style_ir`); owner #2 is named down to the exact line. Both are real, both are named with
file:line, and between them they account for the overwhelming majority of both the count and the
bytes regression in every one of the 11 categories — this is not a "no clear owner" result.
