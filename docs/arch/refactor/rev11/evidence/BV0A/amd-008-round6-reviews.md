# AMD-008 round 6 — first review of the vector-specified design

Reviewed commit `bf08e8ae8e4a28802b73a5d5e719adf3f170c31e`, tree
`bcbd0e481d26fb8ae24564714687698c36a501ce`. All three mandates `BLOCK`.

This round reviewed a changed SHAPE: the composition algebra moved out of prose
and into the frozen literal vector artifact
`packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json`,
which both the independent JavaScript reference and the production Rust
assembler must reproduce exactly. Reviewers were directed to audit the vectors
themselves against real source, since a wrong vector becomes a ratified bug.

**The approach was confirmed; the first vector draft was not.** Conformance
affirmatively re-verified positional comparison, the two-pass rewrite order, the
7-to-9 byte rename, `Chunk::Overwritten` token geometry including the
following-chunk transition, the carried-origin ownership model, the map-enabled
/ map-disabled exit split with BF2 running but not gating, and BV0's retained
36-cell exit. It then found real defects in the vectors, two of which were
outright wrong expected outcomes:

- **F5** supplied `"AC"`, which the accepted decoder rejects as a malformed
  two-field segment BEFORE any index check, so the vector tested the wrong
  category. Independently confirmed (`decodeMappings("AC")` throws
  `malformed mapping segment (2 fields)`); corrected to `"ACAA"`, which decodes
  to source index 1 against a one-row table.
- **V4** placed the template at generated line 1, but the assembler writes an
  unconditional separator newline before template code
  (`crates/verter_session/src/compile.rs`), so a newline-terminated script puts
  the template at line 2. Independently confirmed; corrected.
- **V1's derivation** described two `Original` chunks where the real transform
  splits only at edit boundaries and produces one spanning chunk. The emitted
  tokens — and therefore V1's expected output — are unchanged, but the
  derivation was corrected since derivations carry normative weight.

Those three, plus the amendment's stale §5.1 status text and its "all four
places" miscount, are corrected. The remaining findings are recorded here and
are NOT yet addressed.

The verdicts are reproduced verbatim below.

---

## Round 6 — conformance

VERDICT: BLOCK

1. **BLOCKING — fail-closed vector F5 has the wrong outcome.**

   Amendment text: “BOTH implementations … must reproduce EVERY vector exactly.” [AMD-008:202](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:202)

   F5 supplies `"mappings": "AC"` and expects `source-index-out-of-table`. [vectors:440](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:440)

   The accepted decoder permits only 1-, 4-, or 5-field segments. `AC` decodes to two fields and is rejected as `malformed mapping segment` before source-index validation. [sourcemap.mjs:80](packages/framework-conformance-harness/src/sourcemap.mjs:80) [sourcemap.mjs:82](packages/framework-conformance-harness/src/sourcemap.mjs:82)

   Correction: use `ACAA`, which decodes to generated delta 0, source delta +1, original line delta 0, original column delta 0, and therefore genuinely exercises source index 1 against a one-row table.

2. **BLOCKING — V4’s template placement contradicts the real assembler.**

   Amendment text: “the production Rust assembler … must reproduce EVERY vector exactly.” [AMD-008:202](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:202)

   V4’s script ends in `\n`, but it declares `templateStartLine: 1` and places the template segment on line 1. [vectors:220](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:220) [vectors:242](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:242) [vectors:270](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:270)

   The assembler writes the newline-terminated script unchanged, then unconditionally writes another newline before template code. The template therefore starts on line 2 in this configuration. [compile.rs:86](crates/verter_session/src/compile.rs:86) [compile.rs:97](crates/verter_session/src/compile.rs:97)

   The supplied `placement` also conflicts with the reference contract forbidding placement traces as inputs. [AMD-008:286](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:286)

   Correction: express V4 using the complete pre-assembly DTO, derive placement through the real write grammar, and expect template line 2. Include complete assembled code, including the blank separator and later scaffolding.

3. **BLOCKING — the frozen artifact has no deterministic schema and its expected artifacts are partial.**

   Amendment text: “every composed vector’s ordered segment sequence, tables, and code” must be reproduced exactly. [AMD-008:363](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:363)

   Contrary evidence:

   - The `$schema` points to `assembled-map-composition.schema.md`, which does not exist at this commit. [vectors:2](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:2)
   - V1, V2, V3, V5, V6, and V7 expect only `code` and `segments`; they omit `version`, `file`, `sourceRoot`, tables, and ignore data. [vectors:115](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:115)
   - V4 omits even `code`, `version`, `file`, `sourceRoot`, `sourcesContent`, and `ignoreList`. [vectors:267](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:267)
   - F7 declares only `outcome: composed`, without the composed code or artifact. [vectors:452](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:452)
   - Success inputs use decoded `segments`, while failure inputs use raw `mappings` or `rawMap`, without a schema defining the conversion and validation boundary.

   Exact comparison consequently has two incompatible readings: omitted fields must be absent, contradicting `MapArtifact`, or expected values are partial projections, contradicting “exactly”.

   Correction: add and freeze a strict schema defining every input form, validation order, expected outcome, and full expected `code + MapArtifact`; reject unknown and omitted required fields.

4. **BLOCKING — several normative derivations are false or non-discriminating, and load-bearing algebra remains uncovered.**

   Amendment text: “Each vector carries its own `derivation` … why its expected output follows from … the real `CodeTransform` chunk-emission semantics.” [AMD-008:206](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:206)

   Defects:

   - V1 claims pass-one chunks include separate `Original[13,19)` and `Original[19,34)` chunks. [vectors:94](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:94) The overwrite implementation splits only at edit boundaries, producing one `Original[13,34)` chunk; newline tokens are emitted while scanning that chunk. [code_transform.rs:617](crates/verter_compiler/src/code_transform/code_transform.rs:617) [source_map.rs:525](crates/verter_compiler/src/code_transform/source_map.rs:525)
   - V6 says stripping CR “would … shift every subsequent column.” [vectors:341](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:341) Columns reset after each LF. Every asserted position is either line 0 column 0 or on line 1 before the CR, so stripping CR leaves all expected segment coordinates unchanged. The real line table retains CR, but this vector does not test that fact. [mapping-oracle.mjs:64](packages/framework-conformance-harness/src/mapping-oracle.mjs:64)
   - F7 models synthetic script code as empty. [vectors:455](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:455) The compiler’s actual synthetic script is nonempty and contains `const __sfc__`, optional assignments, and `export default __sfc__`. [compile/mod.rs:1045](crates/verter_compiler/src/compile/mod.rs:1045)

   No vector pins a mid-line removal, a source-bearing old-end transition, two distinct segments strictly inside a rename, multiple same-line replacements, rewrite-boundary coincident-token ordering, or an assembly-scaffolding boundary. Those are expressly load-bearing in the positive fixture. [AMD-008:370](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:370)

   Correction: repair V1’s derivation; make V6 assert a CR-sensitive coordinate; use real synthetic bytes and a full expected artifact in F7; add literal vectors for the uncovered rewrite and assembly-boundary cases before freezing.

5. **BLOCKING — the canonical output schema is not actually canonical.**

   Amendment text: “`file` is the assembled module’s own identity” and fragment sources are “rebased into assembled-relative spellings”. [AMD-008:163](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:163) [AMD-008:183](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:183)

   The real main node has two rendered identities: `canonical_id` for the bundler and `canonical_id._VERTER_.bundle.ts` for LSP. [id.rs:183](crates/verter_session/src/id.rs:183) Neither the amendment nor a vector selects one.

   The source-root policy likewise gives no deterministic rebasing algorithm or base, yet uses the undefined judgment “cannot both be honoured.” Two independent implementations can choose different path normalization, URL, absolute-path, or relative-base behavior while appearing compliant.

   Correction: define the exact `file` value per production route and a mechanical source-root rebasing function, including normalization and incompatibility predicates; pin both in complete vectors.

6. **BLOCKING — `UncomposableInputMap` is not exhaustive or mechanically classifiable.**

   Amendment text: “`UncomposableInputMap` … is exactly” the listed categories. [AMD-008:479](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:479)

   Missing or indeterminate cases include:

   - An entirely missing required fragment map is mentioned separately as fail-closed but has no typed category. [AMD-008:458](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:458)
   - Malformed `ignoreList` shape or an ignore index outside `sources`.
   - Malformed table containers, as distinct from malformed row values.
   - Accumulator underflow for original line/column coordinates.
   - Invalid `sourceRoot` types.
   - The exact predicate for a source-root pair that “cannot both be honoured”.
   - The exact condition under which `sourcesContent` “cannot be made index-parallel”; item 1(c) otherwise directs padding with `null`.

   The real oracle separately exposes map-presence, source/content identity, generated bounds, source/name bounds, and original-coordinate bounds failures. [mapping-oracle.mjs:1096](packages/framework-conformance-harness/src/mapping-oracle.mjs:1096) [mapping-oracle.mjs:1143](packages/framework-conformance-harness/src/mapping-oracle.mjs:1143) [mapping-oracle.mjs:1173](packages/framework-conformance-harness/src/mapping-oracle.mjs:1173) [mapping-oracle.mjs:1202](packages/framework-conformance-harness/src/mapping-oracle.mjs:1202)

   Correction: specify a total ordered preflight decision table or tagged enum, including missing-map, table/container, ignore-list, accumulator, and metadata cases. Explicitly classify valid-wire but authored-invalid original coordinates as BV0 carry-through cases.

7. **BLOCKING — the control set does not prove all claimed fields or failure classes.**

   Amendment text: “One mutation per compared artifact field” and “One plant per item 4 category.” [AMD-008:398](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:398) [AMD-008:408](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:408)

   The per-field list has no isolated mutation for `version`, a segment’s generated line/column, `source_index`, or the mapped-versus-sourceless variant. A comparator that ignores an in-bounds `source_index` has no named discriminating plant.

   The fail-closed list omits malformed content rows and both “incompatible table metadata” subcategories. The checked-in fail vectors omit still more: non-string mappings, name-index bounds, generated bounds, surrogate-half coordinates, incompatible metadata, and missing required maps.

   Correction: enumerate the artifact recursively and give each independently mutable field a named RED plant; give every final preflight enum variant one literal fail vector and one real-path plant.

8. **MAJOR — reference independence remains process-asserted rather than structurally auditable.**

   Amendment text: “No translation” and “The reference is written from this amendment’s normative algorithm, NOT transcribed from the production implementation.” [AMD-008:297](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:297)

   The only objective audit specified is dependency/import inspection. [AMD-008:280](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:280) The serialized DTO remains “at minimum” rather than a frozen schema, and the current vectors neither cover complete assembly nor complete artifacts. A line-for-line JavaScript transcription can therefore pass the structural audit and the incomplete vectors while sharing Rust placement/table defects.

   Correction: freeze the complete DTO schema, prohibit derived placement/map fields by schema, require full-assembly third-source vectors, and require review evidence tracing reference decisions to vectors/specification rather than production source.

9. **MAJOR — the amendment contradicts its own vector adoption and miscounts the superseded text.**

   Amendment text gives the vector artifact normative, frozen standing. [AMD-008:191](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:191) Yet its history says that adopting vectors remains “an architecture decision outstanding” and that “no ratification record may be written against this text.” [AMD-008:645](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:645) It also calls the old round-4 prose-only version “this design.” [AMD-008:632](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:632)

   Section 4 says CodeTransform/chunk-IR language appears in “all four places,” but there are six mirrored charter occurrences—three in BV0A and three in AMD-007—plus AMD-007’s recorded ratification phrase. [BV0A.md:32](docs/arch/refactor/rev11/charters/BV0A.md:32) [BV0A.md:63](docs/arch/refactor/rev11/charters/BV0A.md:63) [BV0A.md:148](docs/arch/refactor/rev11/charters/BV0A.md:148) [AMD-007:134](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:134) [AMD-007:159](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:159) [AMD-007:243](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:243) [AMD-007:520](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:520)

   The substantive supersession enumeration does cover those passages, AMD-007 §1/§7, §8.1, and owned-scope item 4; the defect is the contradictory status/history and erroneous count.

   Correction: record that the vector remedy has been adopted in this candidate, describe rounds 4–5 as prose-only predecessors, remove the “decision outstanding” prohibition, and say “six mirrored charter occurrences plus the ratification clause.”

Verified as conforming:

- Positional comparison is necessary: the decoder preserves zero-delta coincident segments, and `resolveAt` selects the last applicable segment. [sourcemap.mjs:70](packages/framework-conformance-harness/src/sourcemap.mjs:70) [mapping-oracle.mjs:1033](packages/framework-conformance-harness/src/mapping-oracle.mjs:1033)
- The two-pass rewrite order, global replacements, and 7-to-9 ASCII length are correct. [compile.rs:82](crates/verter_session/src/compile.rs:82)
- `Chunk::Overwritten` emits one token for nonempty replacement, none for empty replacement; a following `Original` chunk supplies the transition. [source_map.rs:243](crates/verter_compiler/src/code_transform/source_map.rs:243) [source_map.rs:505](crates/verter_compiler/src/code_transform/source_map.rs:505)
- Carried origin plus non-deduplicated tables is sufficient in principle for fragment/table ownership.
- The required exits clearly separate map-enabled/map-disabled cells and keep BF2 running without gating BV0A on BF2’s residual mapping verdict. [AMD-008:436](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:436)
- BV0’s literal 36-cell oracle-clean exit remains intact. [BV0.md:52](docs/arch/refactor/rev11/charters/BV0.md:52)

---

## Round 6 — architecture

VERDICT: BLOCK

Exact ordered equality is sound in the abstract if the reference is total and correct and the wire validator is complete. It would catch unilateral masking, table corruption, segment loss/duplication, and equal-coordinate reordering. The current amendment does not establish those premises: its proposed frozen specification contains a wrong vector, is not a complete executable specification, and leaves common-mode defects possible.

1. **CRITICAL — A frozen fail-closed vector is factually wrong.**

   [F5](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:440) uses `mappings: "AC"` and expects `source-index-out-of-table`. The accepted decoder permits only one-, four-, or five-field segments; `"AC"` decodes to two fields and fails as `malformed mapping segment (2 fields)` at [sourcemap.mjs](packages/framework-conformance-harness/src/sourcemap.mjs:81). It never produces a source index.

   The encoding for `[generated-column delta 0, source-index delta 1, source-line delta 0, source-column delta 0]` is `"ACAA"`.

   Why it matters: AMD-008 makes the vector govern over prose and freezes it by amendment. Ratification would therefore make the wrong failure category normative.

   Required change: replace `"AC"` with `"ACAA"`, independently rederive every fail-closed vector, and send the changed artifact through fresh review.

2. **CRITICAL — The vector artifact is not a total, executable algebra specification.**

   Its `$schema` points to `assembled-map-composition.schema.md`, but that file does not exist. More importantly, every successful `expected` object is partial:

   - V1–V3 and V5–V7 omit all tables and schema fields.
   - V4 omits `code`, `version`, `file`, `sourceRoot`, `sourcesContent`, and `ignoreList`.
   - There is no rule saying whether omitted expected fields mean “absent,” “inherited,” or “not asserted.”

   That is incompatible with the requirement that both implementations reproduce every vector “exactly” and with complete `MapArtifact` equality in [AMD-008 §2.1](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:130).

   The execution target is also ambiguous:

   - V4 specifies `templateStartLine: 1`, while the real assembler writes an explicit blank line before every template, so a newline-terminated line-0 script places the template on line 2 at [compile.rs](crates/verter_session/src/compile.rs:97).
   - If vectors instead target a lower composition primitive with caller-supplied placement, the amendment must name that primitive and stop saying the production assembler reproduces the vector’s complete code.
   - F7 supplies a `synthetic` script flag that does not exist on `RuntimeScriptBlock`; the real template-only route has `compiled.script == None`, as asserted in [compile.rs](crates/verter_session/src/compile.rs:531).

   Required change: add a closed schema defining complete inputs, complete outputs, omission semantics, validation precedence, and the exact production/lower-level entry point. Every successful vector must contain the full expected artifact and exact code.

3. **BLOCKING — Load-bearing composition cases remain unspecified.**

   The seven successful vectors do not cover:

   - the required mid-line removal and line join;
   - multiple edits on one line;
   - multiple input occurrences inside one overwrite;
   - an input occurrence exactly at an edit end, colliding with the following-chunk transition;
   - relative ordering between a surviving equal-coordinate occurrence and a newly introduced transition;
   - assembly-added newlines and their sourceless boundaries;
   - `file`, `sourceRoot` rebasing, `sourcesContent`, or `ignoreList`;
   - incompatible metadata cases.

   V6 also does not test its stated CR rule: there is no segment or edit after the CR on the same line, so stripping the CR from line accounting produces the same expected coordinates. V1’s derivation calls `[13,19)` and `[19,34)` separate `Original` chunks, but the real transform retains one `[13,34)` chunk and emits an interior line-start token while walking it.

   Concrete defect that can pass: production and reference can choose the same wrong order when a surviving segment at a deletion end collides with the deletion transition. V3 proves only identity-path equal-coordinate preservation, not edit-created collisions. Both artifacts remain exactly equal, and the later oracle can normalize/order lookup differently.

   Required change: add complete vectors for every case above, especially mid-line deletion, collision ordering, and all table fields. A frozen example set cannot be called the algebra until every branching rule has a discriminating vector.

4. **BLOCKING — Assembly-boundary neutrality still has a BV0-unclosable blind spot.**

   Suppose a script or template fragment does not end in `\n` and its last same-line segment is source-bearing. The assembler appends a synthetic newline at [compile.rs](crates/verter_session/src/compile.rs:87) or [compile.rs](crates/verter_session/src/compile.rs:100). Omitting a sourceless boundary at the fragment end makes a lookup over that assembly-owned newline inherit authored provenance.

   No vector exercises this. If both implementations omit it, artifact equality passes. BV0 cannot reliably close it: the accepted oracle expressly leaves some generated regions uncovered and applies boundary checks only to its enumerated ranges at [mapping-oracle.mjs](packages/framework-conformance-harness/src/mapping-oracle.mjs:1302). The raw assembly-added newline is not such a parsed range.

   Required change: freeze a literal write-boundary manifest covering every assembler write class and add vectors for non-newline-terminated script and template fragments. Assert effective lookup state at every synthetic byte boundary, not merely one mutation-selected example.

5. **BLOCKING — Independence is structurally partial, not enough to prevent shared defects.**

   §2.2(a)(1) and (2) are good, auditable rails: cross-language location, no Rust dependency, and one input-only DTO. Clause (3), “No translation,” is not structurally auditable. A manually ported wrong algorithm has exactly the permitted dependency graph. The erroneous F5 vector demonstrates that “hand-derived” is not itself a correctness guarantee.

   N-version implementation is useful as a secondary oracle, but it is not a sufficient primary acceptance authority when both versions derive behavior from the same incomplete vectors and prose.

   Required change: for this finite interim domain, freeze independently reviewed complete expected map artifacts—or canonical encoded digests—for all 36 cells, plus a complete primitive vector basis. Use the JS reference to generate diagnostics and broaden testing, not as the sole expected-value authority. Pin the reference source identity and require its changes to be reviewed separately from production changes.

6. **MAJOR — The criterion is simultaneously too strong and underdetermined.**

   Stable append without deduplication, retaining unused rows, and exact redundant sourceless-segment multiplicity are not required for source-map consumer equivalence. A correctly remapped deduplicating assembler can be semantically neutral yet fail this gate. That is extra canonicalization policy for an interim block.

   At the same time, the canonical choices are incomplete:

   - “assembled module’s own identity” does not specify the exact `file` string;
   - `sourceRoot` rebasing has no path/URI algorithm or base identity;
   - `ignoreList` ordering, duplication, accepted wire field name, row type, and bounds behavior are unspecified;
   - `UncomposableInputMap` is therefore not total for malformed or out-of-range ignore-list metadata.

   Required change: either compare a documented semantic canonical form and keep origin identity in a separate provenance ledger, or fully specify and vectorize every canonical field. For BV0A, the semantic form is the more appropriately sized choice.

7. **MAJOR — Encoded-map determinism is no longer proved, although circularity and the basic scope split are corrected.**

   AMD-008 explicitly declines raw `mappings` equality at [§2.1(b)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:144). Thus varying JSON whitespace/member order, trailing empty mapping lines, or other valid alternative encodings can pass after decoding. Duplicate JSON members are not expressly rejected and can be interpreted differently by consumers. This drops AMD-007’s deterministic-map exit.

   Required change: require duplicate-member rejection and byte determinism across repeated runs. Either require one canonical encoding or separately assert stable encoded-map bytes while retaining logical equality as the correctness comparison.

   The underlying circularity is genuinely broken: BV0A no longer gates on oracle cleanliness, while BV0 still owes the full 36/36 clean oracle result. The BV0A/BV0 ownership thesis is also correct, and §4’s `CodeTransform` supersession is substantively bounded to the two local rewrites. However, the text still says both that vectors alone are normative and that the following prose is the “normative algorithm” at [AMD-008 §2.1(d)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:191). That hierarchy must be made singular.

In terms of the requested axes: **(1) soundness, (2) strength, (3) independence, (4) boundary closure, and (7) right-sizing are not cleared; (5) circularity is cleared; (6) the local `CodeTransform` scope widening is cleared in substance, but the normative hierarchy and vector execution target are not.**

---

## Round 6 — adversarial / governance

VERDICT: BLOCK

The oracle’s non-gating status is not inherently a waiver. The ownership split is principled in intent: AMD-008 says an equality failure “is BV0A’s OWN composition defect” and BV0 must bring residual violations “to zero.” But the replacement gate is not yet sound: its frozen normative artifact contains a wrong vector, is structurally incomplete, and leaves common-mode escape hatches.

1. CRITICAL — Normative vector F5 is factually wrong.

   Quoted text:

   > “Where the prose below and a vector could be read to disagree, the VECTOR governs.”

   F5 claims:

   > `"mappings": "AC"`  
   > `"category": "source-index-out-of-table"`

   The real decoder rejects `AC` as a malformed two-field segment; it never reaches source-table bounds checking. A mapped segment with generated column 0, source index +1, and original 0:0 is `ACAA`. I verified directly that `decodeMappings("AC")` throws `malformed mapping segment (2 fields)`, while `decodeMappings("ACAA")` yields `srcIdx: 1`.

   Governance failure: ratification would freeze an incorrect typed outcome as normative behavior, requiring both implementations to reproduce a bug.

   Required correction: replace `AC` with `ACAA`, add an actual derivation, rerun independent review on the changed vector artifact, and ensure malformed-arity has its own distinct vector/category. See [F5](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:440) and the [decoder arity check](packages/framework-conformance-harness/src/sourcemap.mjs:81).

2. CRITICAL — The “literal vector specification” has no determinate schema and its expected artifacts are partial.

   Quoted text:

   > “BOTH implementations … must reproduce EVERY vector exactly.”

   > “every composed vector’s ordered segment sequence, tables, and code”

   But the artifact references a nonexistent file:

   > `"$schema": "./assembled-map-composition.schema.md"`

   No such file exists at the reviewed commit. The expected outputs are also inconsistent projections:

   - V1–V3 and V5–V7 provide only `code` and `segments`.
   - V4 provides `sources`, `names`, and `segments`, but no `code`.
   - None provides complete `version`, `file`, `source_root`, `sources_content`, or `ignore_list` expectations.
   - F7 says only `outcome: "composed"` plus prose, with no exact composed artifact.

   Exploit: implement both runners as partial matchers that compare only keys present in `expected`; wrong `file`, `sourceRoot`, `sourcesContent`, `ignoreList`, or code in V4 passes. If omission instead means “must be absent,” the vectors contradict AMD-008’s mandatory fields. Both readings are legitimate.

   Required correction: add a committed, closed schema defining exact-versus-projection semantics; preferably make every composed vector contain a complete input DTO and complete expected `MapArtifact` plus code. Bind the Rust vector runner to the real production assembly path, not a test-only composition helper.

3. CRITICAL — The vector set neither satisfies its own derivation rule nor covers the load-bearing algebra.

   Quoted text:

   > “Each vector carries its own `derivation` … A vector without a derivation is not ratifiable.”

   All seven entries under `failClosedVectors` lack a `derivation` field.

   Two composed derivations are also defective:

   - V1 states:

     > `Original[13,19) | Original[19,34)`

     Real `CodeTransform` leaves one `Original[13,34)` chunk; the token at the embedded line start is produced by `emit_mapped_content` scanning the newline, not by a chunk split. See [chunk splitting](crates/verter_compiler/src/code_transform/code_transform.rs:617) and [newline emission](crates/verter_compiler/src/code_transform/source_map.rs:519).

   - V6 claims:

     > “Stripping the CR would under-count every line by one and shift every subsequent column.”

     Columns reset after LF. More importantly, V6 has no mapping or edit whose same-line column depends on the CR; a coordinate implementation that internally strips CR while preserving output bytes still reproduces its expected segments. The claimed CRLF control is non-discriminating.

   Load-bearing rules with no literal vector include:

   - global removal beginning mid-line and joining the following line onto a nonzero prefix;
   - multiple renames on one line;
   - multiple segments strictly inside a rename range;
   - rewrite-induced coordinate collisions and boundary/input-token precedence;
   - actual assembly scaffolding boundaries and the style/custom/import/template-blank/render/HMR/SSR/export branches;
   - `file`, `sourceRoot`, `sourcesContent`, and `ignoreList` policy;
   - most `UncomposableInputMap` categories.

   Required correction: independently derive complete vectors for every listed rule, correct V1/V6, and give every fail-closed vector a source-grounded derivation.

4. CRITICAL — Reference independence and the byte baseline remain candidate-gameable.

   Quoted text:

   > “The reference … has NO dependency … on Rust composition”

   > “The reference is written … NOT transcribed from the production implementation”

   These restrictions are one-way. They do not forbid production Rust, Rust fixtures, or a shared generator from being derived from the JavaScript reference. Nor is the complete assembly byte grammar specified independently: `render_ids`, import formatting, Rust debug-string escaping, and conditional separators/newlines remain available only by copying [the production assembler](crates/verter_session/src/compile.rs:34).

   The baseline is also chosen later:

   > “binds a named commit and tree plus independently captured per-cell output digests, recorded in the candidate’s evidence.”

   Exploits:

   - Generate Rust tables or test fixtures from the JS oracle. The JS still imports no Rust, so the letter passes while both share every error.
   - Backfill the “named” baseline and digests from a defective candidate predecessor, then show that a later mutation is detected. That proves sensitivity, not authenticity.
   - Use a shared declarative placement table that contains the same wrong branch offset in both implementations.

   Required correction: prohibit generation, transcription, shared executable helpers, shared expectation tables, and reverse dependency in both directions. Freeze the exact baseline commit/tree and a path/blob-bound 36-cell digest manifest in the reviewed amendment package. Either specify the byte grammar independently or freeze a reviewed write manifest.

5. CRITICAL — The canonical schema and `UncomposableInputMap` classification are still non-total and permit rescope laundering.

   Quoted text:

   > “`file` is the assembled module’s own identity”

   The code has two plausible identities: `canonical_id` and `canonical_id._VERTER_.bundle.ts` in [render_ids](crates/verter_session/src/id.rs:183). The amendment chooses neither.

   It also says both:

   > “Fragment source spellings are rebased into assembled-relative spellings”

   and:

   > “source spellings … are carried OPAQUELY and unchanged”

   No map base, path/URL resolution, Windows handling, normalization, or serialization rule defines that rebasing.

   The purported exact taxonomy omits or leaves subjective:

   - malformed/missing table containers, not merely row types;
   - malformed `file`, `sourceRoot`, and ignore-list members;
   - ignore-list type, bounds, order, duplication, and legacy spelling;
   - negative accumulated coordinates;
   - short versus extra `sourcesContent`;
   - unknown top-level members and `debugId`;
   - exact fragment endpoint legality;
   - an objective definition of a source-root pair that “cannot both be honoured.”

   Exploit: use the wrong source-map base, then classify the resulting failure as “incompatible table metadata” and request rescope before equality. The clause making equality failures BV0A-owned does not apply because comparison never runs.

   Required correction: define a closed raw-input schema and total preflight decision table over the immutable DTO; specify exact `file`, rebasing, allowed keys, metadata policies, endpoint rules, and typed outcome for every malformed state. No category may depend on production/reference output.

6. HIGH — Map-disabled applicability is not bound tightly enough to prevent exemption drift.

   Quoted text:

   > “partitioned from the LOCKED BF2 seed manifest’s own `sourceMap` request input”

   > “the expected per-class counts asserted”

   The amendment names no manifest path, blob/digest, record IDs, or literal `18 map-enabled / 18 map-disabled` split. “Pre-assembly authored-fragment inventory” is likewise not given an immutable source.

   Exploit: run against a stale or substituted object called the “locked manifest,” or alter one troublesome record before partitioning. A representative applicability mutation can still pass while another cell is silently reclassified.

   Required correction: bind the exact manifest path and blob/hash, enumerate all 36 IDs, state 18/18 explicitly, and derive fragment requiredness from a named immutable authored-input field captured before candidate execution.

7. CRITICAL — The mutation protocol is internally impossible and the controls remain incomplete.

   Quoted text:

   > “Every mutation … must prove … the candidate artifact was actually emitted … the independent reference was UNCHANGED; [and] the NAMED equality assertion produced the RED”

   That cannot hold for:

   - malformed-map controls, which must fail wire/preflight before artifact emission;
   - applicability controls, which must fail partition accounting;
   - the code-baseline control, which must fail before map equality;
   - input mutations, which necessarily change the reference’s input.

   The “FIVE geometry mutations” are actually eight separately required plants across the five numbered categories. The per-field sweep still lacks direct controls for `version`, generated line, generated column, `source_index`, mapped-versus-sourceless kind, ignore-list presence/order/duplication, and rewrite-induced collision precedence. No control proves every real assembly boundary independently.

   Exploit: provide tests that satisfy the named list while the comparator ignores `source_index` or a production branch omits an SSR/HMR sourceless boundary.

   Required correction: partition controls into preflight, applicability, baseline, and equality protocols with stable IDs and named rejecting assertions. Add a generated matrix covering every artifact/segment field and every failure category. Restore persisted reversible recipes and independent no-sampling reruns.

8. HIGH — Supersession still silently removes authority and leaves a live textual conflict.

   AMD-008 says:

   > “BV0A’s Objective, Required procedure, Required exits, and Abort/rescope … [are] replaced”

   But §2 item 4 carries forward only oracle immutability and the input-map taxonomy. It drops the original mandatory stop if correctness requires B3/B4/BV1/B5 authority, a universal IR, or a new public contract.

   Separately, the original owned-scope clause forbids a “harness copy,” while AMD-008 reinterprets that phrase to permit the independent JS assembly oracle and then says:

   > “Everything else in both items is untouched”

   The supersession list does not explicitly supersede the “no harness copy” phrase. “All four places” is also factually wrong: there are six mirrored CodeTransform/chunk-IR passages, plus the separately handled §8.1 occurrence.

   Required correction: carry the original first Abort/rescope paragraph forward verbatim, explicitly supersede and redefine “no harness copy,” and correct the occurrence count.

   The CodeTransform widening itself is honestly declared, and BV0’s literal 36/36 oracle-clean exit is textually preserved. The weakness is that an assembly defect missed by both reference and incomplete vectors has no later owner.

9. HIGH — Ratification identity is mostly sound, but the bundle exception remains semantically enforceable rather than byte-enforceable.

   Good: §5 binds exact package and bundle commit/tree identities and clearly separates amendment-text ratification from candidate acceptance.

   Defect:

   > “the bundle’s diff … must contain ONLY the review-history and §5.1 ratification records”

   “Review-history” is not an exact path/blob allowlist. The explicit byte-identical clause names “this amendment’s text and its charter deltas” but not the newly normative vector blob.

   Exploit: include an arbitrary modified evidence file under the semantic label “review history,” or change the vector unless a reviewer independently notices that it falls outside the prose category.

   Required correction: require direct ancestry from the reviewed package; enumerate exact permitted paths and blob OIDs; record the diff hash; and explicitly require the AMD-008 blob, vector blob, and charter-delta blobs to remain byte-identical.

10. HIGH — The review history identities are accurate, but the narrative overstates both proof and convergence.

