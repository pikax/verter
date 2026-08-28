# Assembled-map composition — literal mutation replay recipes

This closes the gap the final conformance review found twice: first "the evidence provides a generic
method and hand-cycle summaries, not an exact reversible recipe and durable RED/GREEN receipt for every
new correctness-bearing test"; then, after a first pass at this document, "recipes lack an explicit
starting SHA and an exact unplanted-control command per recipe." Both are fixed here: every row below
names the exact commit the plant was applied against, the exact plant, the exact target command, the
exact unplanted-control command (independently confirmed GREEN while the plant is live — the side the
mutation does NOT target), and the exact restore-and-reverify step. `mutation-evidence-fields-and-staging.md`
documents the METHOD and the historical narrative; this document is the literal, per-recipe replay ledger.

**All 15 sub-plants across all 5 permanent tests now have a dated, verbatim, independently-confirmed
RED→control-GREEN→revert→GREEN receipt in this document** — the four rows a prior pass of this document
left un-replayed (JS U5.1, U4.1, U3.3-accumulator, U3.3-index) were replayed in the same session that
added the starting-SHA/control-command columns.

Every plant was applied with `Edit`, confirmed present via `grep -c "<marker>"` returning exactly `1`
(new — the marker did not exist in the file before), run, reverted, and re-confirmed by an EMPTY
`git diff --stat` before the next plant — the same present/unique/new/revert discipline
`mutation-evidence-fields-and-staging.md §1` established.

## 1. `the_comparator_discriminates_every_compared_member`

**Starting commit:** `d3d0f83f0` (tree clean before and after).
**File:** `crates/verter_session/src/compile/map_equality_tests.rs`, inside `compared_artifact`.
**Anchor:**

```rust
    let source_root = object.get("sourceRoot").map(|value| {
        value
            .as_str()
            .unwrap_or_else(|| panic!("{side}: `sourceRoot` is present but is not a string\n{raw}"))
            .to_string()
    });
```

**Plant:** replace the whole block with `let source_root = None;`

**Target command:** `cargo test -p verter_session --lib compile::map_equality_tests::the_comparator_discriminates_every_compared_member`
— RED, verbatim: `` assertion `left == right` failed: perturbing `source_root` must move exactly ["source_root"] — left: [], right: ["source_root"] ``

**Unplanted control (JS side, untouched by this plant):** `npx vitest run test/assembled-map-composition-validation.spec.mjs test/assembled-map-composition-algebra.spec.mjs` (run from `packages/framework-conformance-harness`) — GREEN throughout, unaffected (this plant touches only the Rust-side test harness's own comparator, never the reference).

**Restore:** put the original block back. Re-run target → GREEN; `git diff --stat` empty.

## 2. `a_mappings_divergence_is_caught_even_when_the_decoded_segments_agree`

**Starting commit:** `d3d0f83f0`.
**File:** `crates/verter_session/src/compile/map_equality_tests.rs`, inside `compared_artifact`, immediately after the `mappings` binding.
**Anchor:**

```rust
    let mappings = object
        .get("mappings")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{side}: `mappings` is absent or not a string\n{raw}"))
        .to_string();
```

**Plant:** append one line directly after it: `let mappings = mappings.trim_end_matches(';').to_string();`

**Target command:** `cargo test -p verter_session --lib compile::map_equality_tests::a_mappings_divergence_is_caught_even_when_the_decoded_segments_agree`
— RED, verbatim: `` assertion `left != right` failed: the wire strings differ by exactly the trailing group — left: "AACAC;A;SEQEC;A", right: "AACAC;A;SEQEC;A" ``

**Unplanted control:** same as row 1 — `npx vitest run test/assembled-map-composition-validation.spec.mjs test/assembled-map-composition-algebra.spec.mjs` — GREEN.

**Restore:** delete the appended line. Re-run target → GREEN; `git diff --stat` empty.

## 3. `the_chained_script_map_carries_both_rewrite_passes_in_sequence`

**Starting commit:** `d3d0f83f0`.
**File:** `crates/verter_session/src/compile/map_compose.rs`, inside `rewrite_script`.
**Anchor:**

```rust
        let after_two = pass_two.chain_source_map(&after_one).expect(
            "the export-removal pass is an overwrite-only transform over pass one's output",
```

**Plant:** replace `&after_one` with `&to_source_map(map)`.

**Target command:** `cargo test -p verter_session --lib compile::map_equality_tests::the_chained_script_map_carries_both_rewrite_passes_in_sequence`
— RED, verbatim: `pass one's rename must be carried into the chained map: the authored position at original column 14 belongs at generated column 16. Chained over the ORIGINAL fragment map it would sit at 14.` (followed by a chained-segment dump containing a segment at generated column 14, pass one's own input column).

**Unplanted control:** `npx vitest run test/assembled-map-composition-algebra.spec.mjs` (the reference's own chaining tests, run from `packages/framework-conformance-harness`) — GREEN; this plant touches only production Rust's `rewrite_script`, never the reference's chaining implementation.

**Restore:** put `&after_one` back. Re-run target → GREEN; `git diff --stat` empty.

## 4. `every_staged_tie_break_has_a_live_loser` (Rust)

**Starting commit for U1.8/U5.1/U4.1/U3.3/U6.1:** `d3d0f83f0`.
**File:** `crates/verter_session/src/compile/map_input.rs`, inside `validate_and_decode`.
**Target command (all five sub-plants):** `cargo test -p verter_session --lib compile::map_tests::every_staged_tie_break_has_a_live_loser`
**Unplanted control (all five):** `npx vitest run test/assembled-map-composition-validation.spec.mjs -t "staged tie-break"` (run from `packages/framework-conformance-harness`) — GREEN throughout every sub-plant below; the JS validator is a separate file, untouched by any of these edits.

| Sub-code | Anchor | Plant | Verbatim RED |
|---|---|---|---|
| U1.8 | `if document.has_duplicate_member() {` (§1.2, ~line 310) | prefix with `false && ` | `this input must fail closed: AssembledVueModule { ... }` |
| U5.1 | `if document.member("sections").is_some() {` (§1.7, ~line 332) | prefix with `false && ` | `` indexed-map beats missing `mappings`: ... — left: MappingsMemberAbsent, right: SectionsMemberPresent `` |
| U4.1 | `source_rows.push(row.as_str().ok_or(C::SourceRowNotAString)?.to_owned());` (§1.17, ~line 377) | replace with `row.as_str().unwrap_or("").to_owned()` | `` row typing beats wire decoding: ... — left: VlqInvalidCharacter, right: SourceRowNotAString `` |
| U3.3 | `if !matches!(fields.len(), 1 \| 4 \| 5) {` (§1.21 phase b, ~line 605) | prefix with `false && ` | `` arity beats accumulator range: ... — left: AccumulatorOutOfRange, right: SegmentFieldCount `` |
| U6.1 | `if payload.source_index as usize >= source_rows.len() {` (§1.22, ~line 415) | prefix with `false && ` | `this input must fail closed: AssembledVueModule { ..., "mappings":"ACAA;A" ... }` |

**Restore (each):** remove the `false && ` prefix (or put `.ok_or(C::SourceRowNotAString)?` back for U4.1). Each individually re-confirmed: target GREEN, `git diff --stat` empty, before the next sub-plant was applied.

## 5. `§4.3 — every staged tie-break has a LIVE loser` (JavaScript reference)

**File:** `packages/framework-conformance-harness/test/assembled-map-composition-validation.spec.mjs`
**Target command (all seven sub-plants):** `npx vitest run test/assembled-map-composition-validation.spec.mjs -t "staged tie-break"` (run from `packages/framework-conformance-harness`)
**Unplanted control (all seven):** `cargo test -p verter_session --lib compile::map_tests::every_staged_tie_break_has_a_live_loser` — GREEN throughout every sub-plant below; production Rust is a separate compilation unit, untouched by any `.mjs` edit.

| Sub-code | Starting commit | File | Anchor | Plant | Verbatim RED |
|---|---|---|---|---|---|
| U1.8 | `d3d0f83f0` | `assembled-map-validate.mjs` | `if (document.hasDuplicateMember) return uncomposable("U1.8");` (~line 70) | prefix with `false && ` | `{code: "U2.3", family: "U2", ...}` vs expected `{code: "U1.8", family: "U1", ...}` |
| U5.1 | `45709da1b` | `assembled-map-validate.mjs` | `if (has("sections")) return uncomposable("U5.1");` (~line 87) | prefix with `false && ` | `{code: "U1.3", family: "U1", ...}` vs expected `{code: "U5.1", family: "U5", ...}` |
| U4.1 | `45709da1b` | `assembled-map-validate.mjs` | `if (typeof row !== "string") return uncomposable("U4.1");` (~line 139) | prefix with `false && ` | `{code: "U3.1", family: "U3", ...}` vs expected `{code: "U4.1", family: "U4", ...}` |
| U3.3 (accumulator) | `45709da1b` | `assembled-map-wire.mjs` | `if (fields.length !== 1 && fields.length !== 4 && fields.length !== 5) { return reject("U3.3"); }` (~line 105, phase B) | prefix the `if` condition with `false && ` | `{code: "U3.5", family: "U3", ...}` vs expected `{code: "U3.3", ...}` |
| U3.3 (index) | `45709da1b` | `assembled-map-wire.mjs` | same anchor as above | same plant | `{code: "U6.1", family: "U6", ...}` vs expected `{code: "U3.3", ...}` (both this row and the accumulator row fail together from the one plant — 2/2 failures in one run, independently confirmed) |
| U6.1 | `d3d0f83f0` | `assembled-map-validate.mjs` | `if (segment.srcIdx !== null && (segment.srcIdx < 0 \|\| segment.srcIdx >= sources.length)) { return uncomposable("U6.1"); }` (~line 163) | prefix the `if` condition with `false && ` | two failures — `U3.3's index loser` and `U6.1's loser` both compose/report `U7.2` where `U6.1`/`U3.3` was expected |
| U8.1 | `d3d0f83f0` | `assembled-map-validate.mjs`, `checkSourceRootAgreement` | the two `if (...) return uncomposable("U8.1");` lines (~lines 229–230) | prefix each condition with `false && ` | the tie-break input composes instead of returning `{outcome: "UncomposableInputMap", family: "U8", ...}` |

The U3.3 pair's combined control was run explicitly in the same pass as its own target: with the plant
live, `cargo test -p verter_session --lib compile::map_tests::every_staged_tie_break_has_a_live_loser`
returned `test ... ok` (GREEN) while the JS target above returned 2 failed / 5 passed (RED) — verbatim
transcript below, both commands run back to back against the same plant, no revert between them:

```
===TARGET (JS, expect RED)===
 Test Files  1 failed (1)
      Tests  2 failed | 5 passed | 61 skipped (68)
===CONTROL (Rust, expect GREEN, unaffected by a JS-only edit)===
test compile::map_tests::every_staged_tie_break_has_a_live_loser ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 6288 filtered out
```

**Restore (each):** remove the `false && ` prefix (or the guard, for the wire.mjs arity plant). Each
individually re-confirmed: target GREEN, control GREEN, `git diff --stat` empty, before the next sub-plant
was applied. Final check after the last revert: full JS suite (`npx vitest run`, no filter, from
`packages/framework-conformance-harness`) — 610/610 passing, confirming zero residue across all fifteen
plants this document covers.

## Summary

| Test | Sub-plants | Replayed with target RED + control GREEN + revert GREEN |
|---|---|---|
| `the_comparator_discriminates_every_compared_member` | 1 | 1/1 |
| `a_mappings_divergence_is_caught_even_when_the_decoded_segments_agree` | 1 | 1/1 |
| `the_chained_script_map_carries_both_rewrite_passes_in_sequence` | 1 | 1/1 |
| `every_staged_tie_break_has_a_live_loser` (Rust) | 5 | 5/5 |
| `§4.3 — every staged tie-break has a LIVE loser` (JS) | 7 | 7/7 |
| **Total** | **15** | **15/15** |

This document, together with `mutation-evidence-fields-and-staging.md` (method, historical narrative,
and the four genuinely evidence-only rows that are not permanent tests — §5 of that document, unchanged
by this one), closes AMD-008's mutation mandate for every PERMANENT test the mutation-testing work added:
starting commit identity, exact plant, exact target command, exact unplanted-control command
independently confirmed GREEN, and exact restore-and-reverify, for all 15 of 15 sub-plants.
