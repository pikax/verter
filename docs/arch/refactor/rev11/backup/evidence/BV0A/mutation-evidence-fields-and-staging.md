# Assembled-map composition — mutation evidence: per-field comparison and per-category staging

**Tree:** cycles run against `e29af733aff991a9b9a8993239c1d0127e246452` (branch
`work/bv0a-integration`), clean before and after each one.

This record closes the two remaining items of AMD-008 §2 item 2's mutation mandate — **every compared
artifact field individually**, and **a fail-closed control per `UncomposableInputMap` category, each
failing at its own preflight stage** — plus the two umbrella-list items the earlier mutation set did not
separately establish (`chain bias`, `placement`).

## Status: regression-guarded vs one-time verified

Twenty-nine mutations were cycled by hand. A hand-cycled mutation is **attestation**: it proves the
property held on one tree at one moment and leaves nothing behind that would catch a future
regression. Where the property can be pinned by permanent test code it now is, and this document
records only the residue.

| Property | Now guarded by | Where |
|---|---|---|
| Every compared artifact member is genuinely read and compared | `the_comparator_discriminates_every_compared_member` | `map_equality_tests.rs` |
| A `mappings` divergence the decoded segments cannot show | `a_mappings_divergence_is_caught_even_when_the_decoded_segments_agree` | `map_equality_tests.rs` |
| Both rewrite passes composed in sequence (chain bias) | `the_chained_script_map_carries_both_rewrite_passes_in_sequence` | `map_equality_tests.rs` |
| Each staged tie-break's loser is armed (production) | `every_staged_tie_break_has_a_live_loser` | `map_tests.rs` |
| Each staged tie-break's loser is armed (reference) | `§4.3 — every staged tie-break has a LIVE loser` | `assembled-map-composition-validation.spec.mjs` |

Each of those five was itself proved discriminating before landing — §4 records the plant that turns
each one red. §5 records what remains one-time evidence, and why.

**One thing this document previously left implicit, now stated.** The per-field mutations had two
halves: that the *comparator* catches a divergence in a member, and that each *implementation's
producer* for that member is exercised. Only the first half was unguarded. The second was already
permanent — a production assembler that stopped shifting ignore-list indices diverges from the
reference on `ignore-list-index-shift` in `table_composition_agrees_across_implementations`, which runs
on every gate. The hand mutations demonstrated that existing guard firing; they did not reveal a hole.
The hole was the comparator, and the new tests close it.

## 1. Method

Every plant was applied with an editor that fails loudly on a non-unique or absent anchor, then proved
**present**, **unique** and **new** before the run:

```
grep -rn "<marker>" crates packages --include="*.rs" --include="*.mjs"   # present, and in one file
git grep -l "<marker>" HEAD -- crates packages                           # empty ⇒ new
git diff --stat                                                          # one file, expected scope
```

A green planted run was treated as a failed plant until proven otherwise. Each cycle required: the
**named** RED for the mutation's own category; an **unplanted control** on the side the mutation does
*not* target, green throughout (AMD-008: "the reference was unchanged where the mutation's own category
does not target it"); a revert; `git status`/`git diff` empty plus the marker absent; and a re-run green.

**Surfaces.** Production `cargo test -p verter_session --lib compile::map_`. Reference
`vitest --run test/assembled-map-composition-{validation,algebra,vectors}.spec.mjs`.

**Named assertions cited below.**

| Tag | Assertion |
|---|---|
| `CMP` | "the production assembler and the independent JavaScript reference disagree on N of M cases" |
| `SELF` | "{side}: its own emitted artifact does not validate against its own code" |
| `ORDER` | `map_tests::the_validation_order_decides_inputs_for_which_several_checks_hold` |
| `REACH` | `map_tests::every_uncomposable_sub_code_is_reachable_from_a_real_input` |
| `CLOSED` | `map_tests.rs:237` — "this input must fail closed" |

## 2. Every compared artifact field individually

`ComparedArtifact` is the compared object: `source_root`, `names`, `sources`, `sources_content`,
`ignore_list`, `mappings`, `segments`. `version` / `file` / `debugId` are asserted **per side** rather
than compared (§7.1/§7.2) and were covered by the earlier set.

Each mutation changed exactly ONE field and left every other byte-identical.

| # | Field | Side | Mutation | RED |
|---|---|---|---|---|
| 1 | `sources` | production | swaps the last two appended rows (§7.4 "no row is … reordered") | `CMP` 6/10 — `["b.vue","a.vue",…]` vs `["a.vue","b.vue",…]` |
| 2 | `sources` | reference | same recipe | `CMP` 6/10 — sides exactly inverted |
| 3 | `sourcesContent` | production | every contributed row pushed as `None` | `CMP` 1/10 — `None` vs `Some([…])` |
| 4 | `sourcesContent` | reference | declared row forced to `null` | `CMP` 1/10 — sides inverted |
| 5 | `ignoreList` | production | entries carried without the base shift | `CMP` 1/10 — `[1,0]` vs `[1,2]`, `mappings` identical |
| 6 | `ignoreList` | reference | same recipe | `CMP` 1/10 — `[1,2]` vs `[1,0]` |
| 7 | `sourceRoot` | production | member dropped at `finish` | `CMP` 2/10 — `None` vs `Some("/src")`, and `None` vs `Some("")` |
| 8 | `sourceRoot` | reference | value perturbed (`+ "/"`) | `CMP` 2/10 — `"/src"` vs `"/src/"`, `""` vs `"/"` |
| 9 | `names` | production | swaps the last two appended rows | `CMP` 1/10 |
| 10 | `names` | reference | same recipe | `CMP` 1/10 — sides inverted |
| 11 | `mappings` (string) | reference | `encodeMappings` emits a trailing `;` | `CMP` 10/10 — `"AACA;A"` vs `"AACA;A;"` |
| 12 | chain bias | production | pass two chains the ORIGINAL fragment map instead of pass one's output | `CMP` 6/8 in `rewrite_geometries_agree_across_implementations` |

Mutation 11 separates the `mappings` **string** from the decoded **segment sequence**: a trailing empty
group decodes to nothing, so `segments` stayed identical on both sides — the reference's own internal
cross-check (`artifact.segments == claimed`) passed under the plant — and the RED came solely from the
`mappings` member.

Mutation 12 is distinct from "rewrite geometry": the geometry of a single `Chunk::Overwritten` is
unchanged; what changes is the *basis* of the second pass, which AMD-008 fixes as "pass two on pass
one's output coordinate space".

**All twelve are now regression-guarded** by the three comparator tests in §4.

## 3. A fail-closed control per category, at its own stage

Coverage of the taxonomy was already established. These prove the **staging** — that each check fires at
its own §4.3 step, and that removing it relocates the rejection rather than merely losing it. Sampling is
**exhaustive over the eight families on the production side**; the reference side carries five.

### Production (`map_input.rs`)

| # | Family | Check removed | Own stage | After the plant |
|---|---|---|---|---|
| 13 | U1 | `has_duplicate_member` (`U1.8`) | 1.2 | **(a)** not rejected — `CLOSED` in `ORDER` + `REACH`; `CMP` 1/46 production `Composed` vs reference `U1.8`. `DECISION` D-2's hazard exactly: production's reader is first-wins, `JSON.parse` last-wins |
| 14 | U2 | `version != 3` (`U2.3`) | 1.6 | **(b)** `ORDER` `left: SectionsMemberPresent, right: VersionNot3` |
| 15 | U3 | segment arity (`U3.3`) | 1.21 phase B | **(b)** `ORDER` `left: AccumulatorOutOfRange, right: SegmentFieldCount`; **and (a)** for `"AC"` |
| 16 | U4 | `sources` row typing (`U4.1`) | 1.17 | **(b)** `ORDER` `left: VlqInvalidCharacter, right: SourceRowNotAString` |
| 17 | U5 | `sections` present (`U5.1`) | 1.7 | **(b)** `ORDER` `left: MappingsMemberAbsent, right: SectionsMemberPresent` |
| 18 | U6 | `srcIdx` bounds (`U6.1`) | 1.22 | **(b)** `ORDER` `left: GeneratedColumnOutOfFragment, right: SourceIndexOutOfTable` |
| 19 | U7 | `genCol` bounds (`U7.2`) | 1.24 | **(c)** failure relocated OUT of preflight INTO composition — panic at `map_compose.rs:99`: "the rename pass is an overwrite-only transform over the validated script: `SegmentPositionOutOfBounds { line: 0, column: 25 }`" |
| 20 | U8 | `sourceRoot` agreement (`U8.1`) | 2.1 | **(a)** not rejected — `source_root_agrees_or_fails_closed` "disagreeing roots fail closed", the module silently composing with the FIRST map's root. `CMP` 4/46 |

Row 19 is neither clause (a) nor (b) but a third outcome, recorded as observed: with the preflight check
gone, composition is reached with data it cannot handle. That is direct evidence for §4.1's ordering
requirement and for the precondition `map_compose.rs:99`'s `.expect()` documents.

### Reference (`assembled-map-validate.mjs`, `assembled-map-wire.mjs`)

| # | Family | Check removed | After the plant |
|---|---|---|---|
| 21 | U1 | `hasDuplicateMember` (`U1.8`) | **(a) and (b) together** — the plain duplicate composes; the tie-break input reports `U2.3`/`U2` |
| 22 | U3 | segment arity (`U3.3`) | **(b)** twice — `U3.3 → U3.5` (phase B → C) and `U3.3 → U6.1` (F5's distinction) |
| 23 | U6 | `srcIdx` bounds (`U6.1`) | **(b)** — `U6.1 → U7.2` |
| 24 | U7 | `genCol` bounds (`U7.2`) | **(a)** — `CMP` 1/46 production `U7.2` vs reference `Composed` |
| 25 | U8 | `sourceRoot` agreement (`U8.1`) | **(a)** — `CMP` 4/46 production `U8.1` vs reference `Composed` |

### Placement

| # | Mutation | Result |
|---|---|---|
| 26 | column offset applied to EVERY fragment line, not only line 0 | **No-op — no test fails.** |
| 27 | line offset dropped for fragment lines > 0 | RED as `attempt to subtract with overflow` in the vendored `oxc_sourcemap` encoder — the sequence stops being line-monotonic |
| 28 | line offset skewed +1 for lines > 0 | RED at `SELF` — `U3.6 generated-column-accumulator-decreased` |
| 29 | fragment placed one line early | RED at `SELF` — `U7.2 generated-column-out-of-fragment`, on 7 of 11 equality tests |

## 4. The permanent tests, and the plants that prove they discriminate

A test that passes proves nothing until it is shown to fail for the right reason. Each new test was
run against the defect it claims to guard; the production and reference *sources* were byte-identical
to `HEAD` before and after.

| Test | Plant | Named RED |
|---|---|---|
| `the_comparator_discriminates_every_compared_member` | `compared_artifact` stops reading `sourceRoot` (`let source_root = None`) | ``perturbing `source_root` must move exactly ["source_root"] — left: [], right: ["source_root"]`` |
| `a_mappings_divergence_is_caught_even_when_the_decoded_segments_agree` | `compared_artifact` normalises the member (`.trim_end_matches(';')`) | `left: "AACAC;A;SEQEC;A", right: "AACAC;A;SEQEC;A"` — the two wire strings collapse to one |
| `the_chained_script_map_carries_both_rewrite_passes_in_sequence` | pass two chained over the original map (mutation 12) | "pass one's rename must be carried into the chained map: the authored position at original column 14 belongs at generated column 16" |
| `every_staged_tie_break_has_a_live_loser` | `U1.8` disabled | `CLOSED` — "this input must fail closed" |
| ″ | `U5.1` disabled | ``indexed-map beats missing `mappings`: … left: MappingsMemberAbsent, right: SectionsMemberPresent`` |
| ″ | `U4.1` disabled | "row typing beats wire decoding: … left: VlqInvalidCharacter, right: SourceRowNotAString" |
| ″ | `U3.3` disabled | "arity beats accumulator range: … left: AccumulatorOutOfRange, right: SegmentFieldCount" |
| ″ | `U6.1` disabled | `CLOSED` — "this input must fail closed" |
| `§4.3 — every staged tie-break has a LIVE loser` (JS) | `U1.8` disabled | ``× U1.8's loser is armed — without the duplicate the document is U2.3`` |
| ″ | `U7.2` disabled | ``× U6.1's loser is armed — with the index in range the same input is U7.2`` |
| ″ | `U8.1` disabled | ``× U8.1's companion is armed — the same two fragments compose once the roots agree`` |
| ″ | `U3.3` disabled | ``× U3.3's accumulator loser is armed`` + ``× U3.3's index loser is armed`` |

The comparator test additionally keeps its own coverage complete by construction:
`differing_compared_members` destructures `ComparedArtifact` **without** a `..` rest pattern, so a new
compared member stops it compiling until listed, and the test then fails until that member has a
perturbation case. Completeness is held by the compiler, not by review.

On the staging tests' technique: they do not disable a check (that would need a shippable toggle in
production logic). They pair each tie-break input with the **same input minus the winner's own
trigger** and assert the loser then reports. Both halves together say the two checks are armed and the
order is what decides — deleting either check, or reversing the order, fails one half. §4's plants
confirm that empirically for five checks on each side.

## 5. What remains one-time evidence only, and why

Four items are not convertible to permanent tests without changes that should not ship.

1. **Row 19 — the U7.2 removal relocating the failure into composition.** Encoding this requires the
   production `U7.2` check to be *absent* at runtime, i.e. a toggle in `map_input.rs` whose only
   purpose is to let a test disable validation. The observation is real and valuable — it is why
   §4.1's "validation runs to completion before any composition work begins" is load-bearing — but it
   is a property of the *removal*, not of any input. **Family U7 itself is permanently guarded** as a
   live loser (`every_staged_tie_break_has_a_live_loser`'s final pair, and the JS `U6.1` pair); only
   this specific relocation observation is one-time.

2. **Row 26 — the vacuous placement column branch.** There is nothing to encode: the mutation is a
   no-op against every reachable input, by construction. Layer 1 §6.3 states the derived invariant
   itself — "By F-c, `columnOffset` is `0` at both T1 and T3. An implementation may observe this but
   must not assume it; the rule above is stated for all columns so it stays total if the write grammar
   changes" — and reading the assembler confirms it: every write preceding both `write_fragment` call
   sites (`compile.rs:151`, `:172`) terminates with LF. A test asserting "this branch is unreachable"
   would assert the write grammar, which `write_grammar_axes_agree_across_implementations` already
   does. Recorded so a future reviewer does not read the silence as a comparator hole.

3. **Rows 27–29 — placement is over-determined.** Every line-offset perturbation makes production's own
   emitted artifact ill-formed against the module it describes, so the failure surfaces at the
   artifact-decode gate before any field comparison. That gate is **already permanent and already runs
   on every case of all eleven equality tests** — `compared_artifact` puts both sides' artifacts through
   `validate_and_decode` against their own code on every single run. Placement is therefore
   regression-guarded by construction; a dedicated test would re-assert a gate that cannot be skipped.
   What is one-time is the *demonstration* that the gate is what catches placement.

4. **The producer-side half of rows 1–10 and 13–25 — i.e. the literal "edit the real source" form.**
   Each required an edit to `map_compose.rs`, `map_input.rs`, or the reference modules. Their permanent
   equivalents are the comparator tests (for the field half) and the live-loser pairs (for the staging
   half). What the hand form additionally showed — that each side's *producer* for a field is exercised
   — was already permanent before this work, via the cross-implementation equality suite: a producer
   that stops shifting ignore-list indices, drops `sourceRoot`, or reorders a table diverges from the
   other implementation on an existing case. The hand mutations demonstrated those guards firing.

## 6. Coverage against AMD-008's list

| AMD-008 item | Status |
|---|---|
| order | earlier set (equal-coordinate wire order) |
| rewrite geometry | earlier set |
| chain bias | §2 #12 — **permanent**: `the_chained_script_map_carries_both_rewrite_passes_in_sequence` |
| placement | §3 #26–29 — discriminated at the artifact-decode gate, which is permanent and unconditional; column component provably vacuous (§5.2, §5.3) |
| synthetic provenance | earlier set |
| every compared artifact field individually | §2 #1–11 — **permanent**: `the_comparator_discriminates_every_compared_member` (all seven members, completeness held by the compiler) and `a_mappings_divergence_is_caught_even_when_the_decoded_segments_agree` |
| a fail-closed control per category, at its own stage | §3 #13–25 — **permanent** for U1/U3/U4/U5/U6/U7 on both implementations plus U8 on the reference, via the live-loser pairs; U2 and U8 pairs pre-existed in `the_validation_order_decides_inputs_for_which_several_checks_hold` and `source_root_agrees_or_fails_closed`. Row 19's relocation observation is one-time (§5.1) |
| pinned-baseline CODE-only mutation | earlier set (`export default` rename) |
