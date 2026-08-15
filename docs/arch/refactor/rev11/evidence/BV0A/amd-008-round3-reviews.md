# AMD-008 round-3 review record and architecture ruling

Three independent blind reviews (conformance, architecture, adversarial/governance)
were run against the amendment at commit
`e78512f23108d532be607361e774eda52b193001`, tree
`f98322bbe747e77cfc064713d78760bb076bc93f`. **All three returned `BLOCK`.**

Two findings were factual errors in the reviewed text and were corrected at
`3c2f44d73ca939343861cbcc7940f831f4e737e1` (the `__sfc__` rename is 7→9 bytes,
not 8→9; `map-version` does not early-return, so the probe report is a
path-dependent subset rather than a prefix). Every remaining finding is
design-dependent and is unresolved.

A subsequent independent architecture consult was asked for the correct design
rather than a repair. Its ruling is that the amendment's mechanism is the wrong
abstraction and must be REPLACED: adopt exact ordered reference-map equality,
use the real `CodeTransform` rewrite semantics, carry origin tags, and delete
the oracle-`detail` parsing, standalone probes, and injective violation
matching entirely. It rules that BF2 need NOT be reopened.

The reviews and the ruling are reproduced below verbatim.

---

## Review 1 — conformance

VERDICT: BLOCK

1. **BLOCKING — the rewrite transform neither matches `Chunk::Overwritten` nor defines `d` consistently.**

   Quoted amendment text:

   > “let `d` be the cumulative length delta of earlier splices”  
   > “if `s <= p < e` and `n > 0`, it survives at `s + d + min(p - s, n)`”  
   > “otherwise it survives at `p + d`”

   For a non-empty `Chunk::Overwritten`, the real source-map generator emits one token at the replacement’s generated start, mapped to the original range start; it does not preserve and clamp every pre-existing segment inside the overwritten range. An empty overwrite emits no token. Evidence: `e78512f:crates/verter_compiler/src/code_transform/source_map.rs:243-275`. Surviving `Original` chunks are mapped separately: `source_map.rs:181-195`.

   The formula is also internally ambiguous. For a single `[0,7) → 9-byte` splice and `p == 7`, `d` as “earlier splices” is zero, so `p+d` gives 7 instead of 9. The qualification that `d` includes splices with `end <= p` contradicts the earlier definition.

   Concrete correction: choose and state one normative model:

   - For CodeTransform equivalence, remove all old in-range mappings and emit the single replacement-start mapping for a non-empty overwrite.
   - For a deliberately different point-rebasing policy, say so explicitly and define:
     `p' = p + Σ_{i:e_i<=p}(n_i-(e_i-s_i))` outside every splice, and a separately defined in-range policy. The `min` clamp is coherent only for that custom point policy; it is not `Chunk::Overwritten` semantics.

2. **BLOCKING — “multiset” equality omits semantically significant equal-coordinate segment order.**

   Quoted amendment text:

   > “the exact composition of the script and template fragments’ own RAW V3 segment multisets”  
   > “Every surviving raw segment occurrence must appear EXACTLY ONCE … and no other raw segment or duplicate may appear.”

   V3 permits multiple segments at the same generated coordinate. The oracle preserves their decoded occurrence order, sorts only by generated column, and `resolveAt` selects the last applicable segment: `e78512f:packages/framework-conformance-harness/src/mapping-oracle.mjs:1167-1171`, `mapping-oracle.mjs:1039-1047`. Consequently, reordering a source-bearing and sourceless segment at the same coordinate can change inherited-provenance behavior while preserving the amendment’s multiset exactly.

   Concrete correction: require stable per-line segment sequence preservation, including an occurrence ordinal among equal generated coordinates. Define how clamp-induced coordinate collisions retain order.

3. **BLOCKING — mandatory byte conversion is undefined for coordinates the oracle can legitimately report.**

   Quoted amendment text:

   > “A V3 UTF-16 `(line, column)` is first resolved to a byte offset”  
   > table row: “`generated-position-bounds` … transformed-generated-coordinate”

   The oracle deliberately accepts arbitrary decoded generated coordinates and reports out-of-bounds positions rather than rejecting the map before attribution: `e78512f:packages/framework-conformance-harness/src/mapping-oracle.mjs:1173-1180`. Such a coordinate cannot be resolved to a byte offset.

   Additionally, oracle columns are JavaScript UTF-16 code-unit indices; a column between the two surrogate code units of an astral character is representable and considered in bounds, but has no Rust UTF-8 byte boundary: `mapping-oracle.mjs:75-100`.

   Concrete correction: either declare these fragment maps uncomposable and fail/rescope before transformation, or define a total coordinate representation covering out-of-bounds positions and intra-scalar UTF-16 positions. The current algorithm cannot execute its `generated-position-bounds` matching row for those inputs.

4. **BLOCKING — source/name remapping is not deterministic or total as specified.**

   Quoted amendment text:

   > “Source and name indices are compared after the assembler’s declared source/name-table remap”  
   > “their referenced source … and name string must remain the fragment entries they remap from.”

   No remap policy is actually declared: append versus deduplicate, first-seen ordering, duplicate source entries, absent versus `null` `sourcesContent`, and handling of differing `sourceRoot` values are unspecified. Yet `sourceRoot` participates in oracle identity: `e78512f:packages/framework-conformance-harness/src/mapping-oracle.mjs:1150-1154`.

   More seriously, the amendment permits attribution of `source-index-bounds` and `name-index-bounds`, but an invalid index has no referenced table entry to remap. The oracle expressly emits those violations: `mapping-oracle.mjs:1187-1200`.

   Concrete correction: prescribe an exact stable merge algorithm and `sourceRoot` policy. For invalid indices, either define a total numeric rebasing that guarantees they remain invalid, or classify such fragment maps as uncomposable and remove them from carry-forward attribution.

5. **BLOCKING — coordinate-only fragment ownership is undefined at fragment boundaries.**

   Quoted amendment text:

   > “for an assembled segment/range violation it is the uniquely owning transformed fragment span”  
   > “a segment at `p == e` … survives at the rebased boundary.”

   The oracle considers an end-of-line/end-of-text coordinate valid: `e78512f:packages/framework-conformance-harness/src/mapping-oracle.mjs:86-100`. The assembler then conditionally appends a newline after the script and inserts another separator before the template: `e78512f:crates/verter_session/src/compile.rs:86-101`. The amendment does not say whether a point exactly at a fragment’s end belongs to that fragment, adjacent scaffolding, or neither.

   Concrete correction: carry fragment provenance per transformed segment occurrence in the rewrite/placement manifest rather than re-deriving ownership solely from assembled coordinate containment. Also define endpoint ownership explicitly.

6. **BLOCKING — the control-flow account is factually incomplete, and the “prefix” conclusion is false.**

   Quoted amendment text:

   > “`map-presence` and `mappings-decode` return immediately”  
   > “A probe therefore reports a PREFIX of what would fail”  
   > “an assembled `map-presence`, `map-version`, or `mappings-decode` violation returns before any segment is examined”

   Actual control flow:

   - Whole-probe returns: all three `map-presence` cases and `mappings-decode`: `e78512f:packages/framework-conformance-harness/src/mapping-oracle.mjs:1096-1107`, `1113-1118`.
   - `map-version` does **not** return; decoding and segment examination continue: `mapping-oracle.mjs:1110-1118`.
   - A non-string `source-identity` entry returns only from the `forEach` callback, skipping later checks for that source: `mapping-oracle.mjs:1145-1163`.
   - Segment `continue`s occur after `generated-position-bounds`, `source-index-bounds`, `original-position-bounds`, and `segment-provenance`: `mapping-oracle.mjs:1173-1239`.
   - The amendment omits `anchor-source-text`’s `continue`, and an absent exact anchor later skips `anchor-relation`: `mapping-oracle.mjs:1246-1257`, `1268-1299`.

   Because a skipped check in one segment/anchor can be followed by failures from later segments/anchors, the report is not necessarily a prefix. It is a path-dependent subset of executed checks.

   Concrete correction: enumerate the control flow above, replace “prefix” with “path-dependent subset,” and state only `map-presence` and `mappings-decode` are whole-probe early returns. Keep `map-version` as an unconditional BV0A precondition without claiming it returns.

7. **MAJOR — the rename length is wrong.**

   Quoted amendment text:

   > “substituting 9 bytes for 8”

   The actual literals are `__sfc__` and `_sfc_main`: `e78512f:crates/verter_session/src/compile.rs:82-85`. They are 7 and 9 ASCII bytes respectively, so the delta is +2. The existing BV0A charter already states the correct 7-to-9 geometry: `e78512f:docs/arch/refactor/rev11/charters/BV0A.md:27-30`, `127-130`.

   Concrete correction: change “9 bytes for 8” to “9 bytes for 7, a +2-byte delta.” The conclusion that this longer replacement does not clamp/coalesce in-range offsets remains true under the amendment’s custom point policy.

### Verification record

- **A — exhaustive rule taxonomy:** verified. The exact 18 emitted rules are: `map-presence`, `map-version`, `mappings-decode`, `synthetic-range-derivation`, `source-identity`, `sources-content`, `generated-position-bounds`, `source-index-bounds`, `name-index-bounds`, `original-position-bounds`, `name-token-relation`, `segment-provenance`, `anchor-source-text`, `anchor-missing`, `anchor-span-coverage`, `anchor-relation`, `synthetic-provenance`, and `synthetic-boundary`. The amendment omits and invents none. Evidence: `mapping-oracle.mjs:1096-1329`.
- **B — detail fields:** every table row accurately identifies the dynamic values present in its literal template; no row claims a classified relation or another absent field. Evidence groups: contract rules `1096-1118`; derivation/source rules `1131-1163`; segment rules `1173-1237`; anchor rules `1243-1299`; range rules `1302-1329`.
- **C:** blocked by findings 1 and 7. The deletion half-open rule itself correctly drops `s <= p < e`; the non-empty `min` rule is not the real Overwritten model.
- **D:** verified. Rename precedes removal, and both Rust `replace` calls are whole-string/global non-overlapping replacement operations: `compile.rs:82-86`.
- **E:** refuted by finding 7.
- **F:** verified. Relations are derived by the ordered `RELATIONS` table and `classifySegment`, not stored in V3 segments: `mapping-oracle.mjs:378-513`, `1232-1240`. Reclassification after generated-token renaming is legitimate.
- **G:** verified. Exact templates are `generated plumbing ${object}.${property}`, `generated default export ${name}`, and `emitted declaration ${name}`: `mapping-oracle.mjs:871-885`, `942-958`, `970-1000`.
- **H:** verified for the stated Vue seed domain. The assembler emits render/SSR attachment, no-script `_sfc_main`, late SSR runtime import, and final export at `compile.rs:90-107`, `143-163`; the oracle derives the corresponding range classes from the artifact at `mapping-oracle.mjs:858-1000`.
- **I:** refuted by finding 6.
- **J:** verified. Every committed `FIXTURE_ANCHORS` row declares `region`, `line`, `column`, and `text`: `mapping-oracle.mjs:533-663`.
- **K / implementability:** findings 2–5 are additional blocking underspecifications. As written, the algorithm is not fully implementable or uniquely deterministic without inventing segment-order, table-remap, invalid-coordinate/index, and boundary-ownership policies.

---

## Review 2 — architecture

VERDICT: BLOCK

1. **BLOCKING — Raw V3 multiset equality is not sufficient to prove neutral composition.**

   Source-map segment order is observable when multiple segments share a generated coordinate. The oracle’s own lookup chooses the last applicable segment ([mapping-oracle.mjs](packages/framework-conformance-harness/src/mapping-oracle.mjs:1033)), while item 1 deliberately compares a multiset ([AMD-008](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:85)). This also contradicts BV0A’s unchanged obligation to preserve segment order ([BV0A.md](docs/arch/refactor/rev11/charters/BV0A.md:47)).

   I reproduced three counterexamples against the real oracle:

   - **Masking:** For generated code `  __expose()\n`, mappings `A,AAAA` and `AAAA,A` decode to the same two-record multiset: one sourceless and one source-bearing segment at `0:0`, in opposite order. The first reports `synthetic-boundary`; the second is clean. If standalone uses the first and assembly reorders to the second, item 1 passes and item 2 permits the unmatched standalone violation.
   - **Identity collision:** Two source-bearing mappings at the same generated `count` token—one valid and one mapping to authored `items`—produce the same single `segment-provenance` identity in either order. Reordering changes the consumer-resolved original position, while both item 1 and item 2 pass.
   - **No oracle violation:** Two mappings from one generated `count` to two authored `count` occurrences both satisfy `verbatim-carry`. Reversing them changes go-to-definition-style resolution while both oracle runs remain clean and the raw multiset remains equal.

   Coverage of the requested cases is therefore:

   | Case | Item 1 | Item 2 |
   |---|---|---|
   | (a) Mask by deleting/moving/changing a raw segment | Yes | No; unmatched standalone failures are allowed |
   | (a) Mask by equal-coordinate reordering | No | No |
   | (b) New defect colliding with an inherited key, but changing a raw field | Yes | No |
   | (b) Collision caused solely by equal-coordinate reordering | No | No |
   | (c) Silent original/source/name-coordinate corruption | Yes | No |
   | (c) Order or lookup-interval corruption with otherwise valid segments | No | No |
   | (d) A genuinely probe-exempt/map-disabled cell | No, unless item 1 is separately required there | No |

   Required change: replace “multiset equality” with order-sensitive segment-sequence equality per generated line, including occurrence ordinal among equal generated coordinates. Alternatively, declare duplicate generated coordinates uncomposable and reject them. Add the three controls above, including a consumer-resolution assertion rather than only decoded-record comparison.

2. **MAJOR — The two checks are complementary, but the document presents the weaker one as the exit thesis.**

   Item 1 is load-bearing. It is the only check that can detect raw corruption that remains oracle-valid, collides with an inherited violation, or never produces a violation. Item 2 is a directional semantic-delta check: it can catch assembly-context effects such as inherited provenance into scaffolding, but it cannot prove raw preservation and intentionally cannot detect masking.

   If item 1 is corrected to ordered equality, item 2 remains useful because standalone fragments do not contain the assembly-only intervals over which a preceding segment may remain active. It is therefore not wholly redundant. But “zero unattributed violation” is not the definition or sufficient proxy for neutral composition.

   Required change: state the exit as the conjunction of:

   1. ordered raw-map composition equality;
   2. explicit lookup-state neutrality across every fragment/scaffolding boundary; and
   3. zero new structured oracle violations.

   Make the first two primary. Describe violation attribution as an independent public-boundary backstop, not the proof of composition.

3. **BLOCKING — The ownership split is not preserved at assembly-only mapping boundaries.**

   The oracle explicitly admits uncovered generated scaffolding and non-position-exact relations ([mapping-oracle.mjs](packages/framework-conformance-harness/src/mapping-oracle.mjs:31)), plus `boundary:false` regions where inherited provenance is accepted ([mapping-oracle.mjs](packages/framework-conformance-harness/src/mapping-oracle.mjs:799)). A source-bearing mapping can therefore bleed into assembly scaffolding without producing any violation. That is a BV0A defect, not a BV0 emitter defect, and BV0’s later “oracle clean” exit cannot close an oracle blind spot.

   Conversely, the rule that ambiguous joins or coordinates outside expected fragment spans become assembly-owned ([AMD-008](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:177)) can assign inherited fragment defects to BV0A. Examples include:

   - an out-of-generated-bounds fragment segment, which by definition may not lie in a fragment span after rebasing;
   - a source-table entry deduplicated from both fragments, for which no unique fragment owner exists;
   - a segment exactly at a half-open fragment boundary.

   Required change: give every expected composed segment and source/name-table entry an origin identity before composition. Attribution must follow that origin, not infer ownership from final coordinates or message text. Separately provide an exhaustive manifest of **all existing assembler-owned synthetic spans**, not only scaffolding newly introduced by BV0A code, and verify the effective lookup mapping throughout those spans is sourceless.

4. **BLOCKING — The baseline is not inherently circular, but its required independence is unspecified.**

   Applying a normative transform \(T\) to an input fragment and checking production output against \(T(input)\) is legitimate. It is not circular if \(T\) is an independently authored reference model.

   The amendment, however, does not define where the “independently witnessed assembled placement” comes from or forbid the baseline from reusing the production splice/placement helper. If expected and actual results share the same incorrect transform, item 1 becomes tautological. Mutation tests applied after that shared computation do not detect the correlated error.

   Required change: specify an independent reference transform that:

   - operates from the original fragment, fixed rewrite literals, and declarative write-order manifest;
   - shares no production transform/composition helper;
   - verifies the assembled fragment bytes equal the independently transformed bytes at the predicted span;
   - verifies placement separately from map composition.

   With that closure, the transform baseline is acceptable and the BV0A→BV0 DAG circularity is genuinely broken. Without it, the amendment substitutes a new self-reference.

5. **BLOCKING — Human-readable `detail` and range labels are an unsound acceptance API; the oracle-change prohibition is the wrong tradeoff.**

   The oracle exposes only `{rule, detail}` ([mapping-oracle.mjs](packages/framework-conformance-harness/src/mapping-oracle.mjs:1073)), so the amendment proposes reparsing seventeen families of diagnostic prose and even rewriting human-readable range labels. This duplicates the oracle’s private formatting grammar as an architectural contract.

   Fail-closed parsing prevents some false acceptance, but it still creates:

   - false BV0A failures after harmless wording or parser-library-message changes;
   - ambiguous joins where the message omits source index, actual relation, segment ordinal, or origin;
   - platform/version coupling through parser and decoder error text;
   - another large schema that cannot be compiler-checked against the oracle.

   Required change: reopen BF2 narrowly and make the oracle emit additive structured violations, for example `{ rule, payload, detail }`, where `payload` contains typed coordinates, source/name indices, anchor ID, range kind/stable range ID, and expected/actual relation where applicable. Preserve `detail` purely for diagnostics and preserve the existing verdict semantics. AMD-008’s prohibition on any oracle change ([AMD-008](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:318)) must be removed for this additive schema correction.

6. **MAJOR — The algorithm is not deterministic or fully implementable as written.**

   Implementers must invent policy for at least:

   - source/name-table remapping: concatenation versus deduplication, stable ordering, duplicate entries, and absent versus `null` `sourcesContent`;
   - ownership of a deduplicated source entry contributed by both fragments;
   - exact parsing and escaping grammar for every `detail`;
   - the authority for “independently witnessed” placement;
   - V3 UTF-16 columns that fall between the surrogate halves of an astral character and therefore have no Rust UTF-8 byte boundary;
   - overlapping textual occurrences—“every occurrence” does not say whether matching follows Rust’s non-overlapping `replace` behavior;
   - whether a map-disabled cell still runs the assembled probe with `sourceMapRequested: false`.

   The last point matters because the oracle explicitly detects an unexpected map, while a fully exempt cell detects nothing.

   Required change: specify these policies normatively. In particular, always run the assembled probe in map-disabled cells to enforce map absence; only fragment probes should be omitted when no fragment map exists.

7. **MAJOR — The interim design is over-engineered around the wrong abstraction and under-specified around the decisive one.**

   The per-message parser, injective matching machinery, label rewriting, and one-dimension mutant matrix are substantial permanent infrastructure for an interim gate. Yet segment ordering, effective lookup intervals, baseline independence, and source-entry ownership—the properties that determine actual composition—remain open.

   With structured violations, “injective match” reduces deterministically to per-key multiplicity containment. A representative equal-cardinality substitution test plus schema-exhaustiveness tests would be sufficient.

   Required change: simplify the design around three artifacts: an independent ordered-map reference model, an origin-preserving assembly manifest, and structured oracle violations. Retain focused adversarial controls for masking, equal-key substitution, and assembly-scaffolding provenance; remove the diagnostic-string parsing and its large mutant matrix.
tokens used

---

## Review 3 — adversarial / governance

VERDICT: BLOCK

Reviewed commit `e78512f23108d532be607361e774eda52b193001`, tree `f98322bbe747e77cfc064713d78760bb076bc93f`.

The ownership split is legitimate in principle, but AMD-008’s replacement gate is not safe to ratify as written. It contains concrete ways for a defective assembler or matcher to satisfy the narrowed exit.

1. BLOCKING — The “exact” rewrite transform is internally contradictory and can bless a wrong offset.

   Exact text:

   > “let `d` be the cumulative length delta of earlier splices”

   followed by:

   > “otherwise it survives at `p + d`”

   and:

   > “this one does not, substituting 9 bytes for 8”

   See [AMD-008 §2.1](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:104).

   The real rename is 7→9 bytes, as AMD-007/BV0A correctly state, not 8→9. More seriously, for the first splice and a segment at `p == e`, the declared `d` is zero because there are no earlier splices. The “otherwise” formula therefore leaves the segment at `p`, although the correct result is `p + 2`.

   Exploit: implement the assembler and the standalone expected-map transform using this same formula. Both place post-rename segments incorrectly, so the raw-multiset comparison passes. Any resulting oracle violation is reproduced by the standalone probe and can be classified as inherited. Differential shifted-coordinate mutants only prove sensitivity around the wrong baseline.

   Required correction: change 8 to 7 and define separate prefix deltas unambiguously. For an offset outside all replaced intervals, use the sum of every splice delta whose `end <= p`; for an offset inside a splice, use only preceding-splice deltas. Add boundary controls for `p=s`, `p=e-1`, `p=e`, after-splice positions, multiple occurrences, Unicode before the splice, and both ordered passes.

2. BLOCKING — The multiset criterion silently drops the unchanged segment-order invariant.

   AMD-008 defines correctness as:

   > “the exact composition of the script and template fragments’ own RAW V3 segment multisets”

   and:

   > “Every surviving raw segment occurrence must appear EXACTLY ONCE in the assembled multiset”

   See [AMD-008 §2.1](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:85).

   But the supposedly unchanged owned scope requires:

   > “preserving declared source identities, source contents, lines, columns, and segment order”

   See [BV0A owned scope](docs/arch/refactor/rev11/charters/BV0A.md:47).

   Concrete exploit: swap two same-line encoded segments at generated columns 5 and 10, emitting column deltas `+10, -5`. The pinned decoder accepts this. For example, `decodeMappings("U,L")` returns columns `[10, 5]`. The raw multiset is unchanged, while the oracle explicitly sorts its lookup inventory by generated column at [mapping-oracle.mjs:1166](packages/framework-conformance-harness/src/mapping-oracle.mjs:1166). Thus the malformed/reordered map is normalized back into passing order for validation. Equal-coordinate segment reversal is worse because it can change which original position a consumer resolves.

   This also disproves §4’s claim that the owned-scope list remains untouched: the replacement acceptance definition no longer enforces one of its affirmative requirements.

   Required correction: compare an ordered transformed segment sequence, require canonical nondecreasing generated columns per line, and preserve stable order among equal-coordinate occurrences. Add an adjacent-swap mutation that must fail.

3. BLOCKING — Expected placement and probe applicability are not bound to an independent authority.

   Exact text:

   > “rebased to the fragment’s actual write-time placement”

   > “rebased to the fragment’s independently witnessed assembled placement”

   > “a cell with no template fragment, no script fragment, or no map requested is exempt from the corresponding probe”

   See [AMD-008 §2.1–§2.3](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:125).

   “Independently witnessed” is asserted but no permissible witness is specified.

   Exploit: a buggy production cursor says the fragment begins at line 9 although it was written at line 10. Use the same cursor metadata to rebase the standalone expectation. The assembled map and expected multiset agree at line 9. Loose oracle relations may remain green if line 9 contains compatible punctuation/emitted identifiers; otherwise both probes can produce matching violations. Similarly, an implementation can derive “no fragment” from missing optional map state, turning an absent input map into an exemption.

   Required correction: define placement from an independent append recorder/output-length witness that cannot read the production map cursor, placement metadata, or assembled map. Add a mutation that corrupts only the production cursor while leaving code bytes unchanged. Derive map-request applicability from the locked BF2 manifest and fragment applicability from the pre-assembly compilation inventory; missing map data must never erase an expected probe.

4. BLOCKING — The collision controls do not cover every declared key dimension and need not exercise the real parsing/join pipeline.

   AMD-008 requires exhaustive one-field mutants only for:

   > “the exact `segment-provenance` key”

   and:

   > “the anchor-completeness core key `(rule, fragment, anchor-id)`”

   See [AMD-008 collision controls](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:260).

   The required set has no one-field control for, among others:

   - `synthetic-boundary.transformed-inherited-segment-coordinate`
   - `synthetic-provenance.range-label`
   - `name-token-relation.declared-name`
   - `source-index-bounds.invalid-source-index`
   - `source-index-bounds.source-count`
   - `map-version.observed-version`
   - `anchor-relation.expected-relations`

   The sentence:

   > “Every mutant must prove that the intended one-field projection change actually applied”

   does not require that proof to start from real `{rule, detail}` oracle output or pass through the production detail parser, map join, fragment attribution, and matcher. An implementer can thoroughly mutant-test a hand-constructed identity equality helper while the production detail parser omits fields.

   Concrete exploit: omit `transformed-inherited-segment-coordinate` from production `synthetic-boundary` identity construction. All mandated `segment-provenance` and anchor-core mutants still pass, while an equal-cardinality boundary substitution differing only in the omitted coordinate is consumed as inherited.

   Required correction: make the key table a machine-readable schema and generate one-field mutants for every independently variable dimension of every rule. Controls must begin with real oracle reports and execute the complete production parse/join/attribute/match path. For functionally determined dimensions, add explicit inconsistency controls rather than omitting coverage.

5. MAJOR — “False input map” is left undefined in exactly the place needed to resolve the circularity.

   AMD-008 permits:

   > “a violation an input fragment’s own transformed map already carries in isolation”

   but later says:

   > “`RESCOPE_REQUIRED` remains reserved for a genuinely absent, false, or uncomposable input map”

   See [AMD-008 §2.1 and §2.4](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:135).

   The diagnosed `const`→`<` source-bearing segment is literally a false mapping under the accepted oracle. The amendment simultaneously treats it as carry-forward-acceptable and as grounds for rescope. This lets the implementer decide after seeing a failure whether “false” means oracle-wrong or merely structurally untransformable.

   Required correction: replace “false” with precise categories. Explicitly distinguish mechanically composable but oracle-invalid fragment mappings, which remain mandatory BV0 bugs, from absent, malformed, undecodable, or mechanically untransformable maps, which block/rescope BV0A.

6. MAJOR — The declared “no invocation change” scope conflicts with the new three-probe procedure.

   AMD-008 requires:

   > “run BF2’s oracle three times”

   with standalone calls whose:

   > “`code`, `map`, and the anchor scoping differ”

   Yet it also says:

   > “oracle/invocation immutability stays unconditional”

   and claims AMD-007’s prohibition on:

   > “oracle/invocation changes”

   remains untouched. See [AMD-008 §2.2](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:145) and [§4](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:340).

   These can be reconciled only by interpreting “invocation” as “the already accepted assembled BF2 invocation,” while treating the standalone calls as new BV0A-only test invocations. The amendment never states that distinction.

   Required correction: explicitly authorize the additional test-only probes while forbidding any modification or replacement of BF2’s accepted assembled invocation and its applicability inputs.

7. MAJOR — The control-flow justification contains a direct factual error and overstates what truncation proves.

   Exact text:

   > “an assembled `map-presence`, `map-version`, or `mappings-decode` violation returns before any segment is examined”

   See [AMD-008 §2.2](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:232).

   The pinned oracle does not return after `map-version`; it records the failure and continues into decoding and segment validation at [mapping-oracle.mjs:1109](packages/framework-conformance-harness/src/mapping-oracle.mjs:1109).

   The related claim that a probe reports “a PREFIX of what would fail” is also inaccurate: returns occur at artifact level, while `continue` suppressions occur independently per segment and later anchor/range phases can still run.

   Required correction: replace the prefix argument with an exact rule-by-rule suppression graph derived from the actual control flow. Retain the unconditional assembled-map preconditions, but do not justify them with false return behavior.

8. BLOCKING — The ratification bundle can contain unreviewed bytes.

   AMD-008 requires reviews to bind:

   > “one exact reviewed amendment-package commit and tree”

   but the ratification action additionally binds a distinct:

   > “ratification-bundle commit `<bundle-full-sha>`, tree `<bundle-tree-oid>`”

   See [AMD-008 §5](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:350).

   Nothing requires the bundle delta to contain only review records and the §5.1 decision. “Any changed reviewed-package byte requires … fresh reports” does not prohibit adding unrelated or modified charter bytes in the bundle.

   This diverges from AMD-007 §8.1, which expressly records that its bundle “adds only the review-history evidence record” and that “the amendment text itself is byte-identical,” while AMD-006 used the same commit for package and bundle.

   Required correction: either have all three reviews bind the final ratification-bundle commit/tree, or require and record an exact diff proving the bundle adds only named review/ratification records and leaves the amendment and charter deltas byte-identical.

The document does contain real safeguards: “Every surviving raw segment occurrence must appear EXACTLY ONCE … and no other raw segment or duplicate may appear,” BV0 still owns bringing all fragment-attributed violations “to zero,” and §5 correctly separates amendment ratification from candidate acceptance. Section 5.1 being pending is correctly staged.

I found no authorization for a TODO, runtime guard, typed refusal, waiver row, or tracking artifact in place of the eventual BV0 fixes. Nor does AMD-008 explicitly weaken BV0’s literal 36-cell exit. The block is because the proposed BV0A gate can still accept incorrect assembly through the transform, ordering, witness, control-coverage, and ratification holes above.
tokens used

---

## Architecture ruling (independent consult)

Reject AMD-008 as drafted. Its multiset comparison, splice formula, coordinate-based ownership, and diagnostic-attribution machinery are the wrong abstraction. Preserve the typed code-plus-map result and single write-order assembler; discard the custom point-clamping transform and all parsing/matching of oracle `detail`.

## A. Correct acceptance criterion

Yes: exact ordered-sequence equality against an independently computed reference artifact is the whole neutrality criterion. The oracle-violation-attribution machinery must be deleted entirely.

For every map-enabled applicable cell:

1. Production assembled code must remain byte-identical to the pre-BV0A contract.
2. Decode the actual V3 map without sorting or canonicalizing its segments.
3. Independently compute the expected assembled map from the input fragment code/maps, the two ordered rewrites, and the specified assembly write order.
4. Compare the complete logical map artifact for exact equality:

```text
MapArtifact {
  version,
  source_root_policy,
  sources[],
  sources_content[],
  names[],
  ordered_segments[]
}

Segment =
  Unmapped { generated }
  | Mapped {
      generated,
      source_index,
      original,
      name_index?
    }
```

The segment comparison is positional: same length, and segment `i` must equal expected segment `i`. It is not a set, multiset, sorted list, or consumer-classification comparison.

This is mandatory because BV0A’s unchanged charter explicitly preserves segment order [BV0A.md](docs/arch/refactor/rev11/charters/BV0A.md:47), while AMD-008 silently substitutes a multiset [AMD-008](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:85). The accepted decoder preserves wire order, including negative generated-column deltas [sourcemap.mjs](packages/framework-conformance-harness/src/sourcemap.mjs:70), and the oracle’s lookup selects the last applicable segment [mapping-oracle.mjs](packages/framework-conformance-harness/src/mapping-oracle.mjs:1033). Equal-coordinate order is therefore observable.

The reference must also specify assembly-owned sourceless boundary segments. At a synthetic-write boundary, append a one-field segment when the current same-line mapping state would otherwise make the synthetic bytes inherit source provenance. No source-bearing segment may be invented for assembly scaffolding.

Structural input failures—missing requested map, undecodable mappings, invalid source/name references, incompatible table metadata, or an unrepresentable generated coordinate—fail BV0A directly as `UncomposableInputMap`. Original authored coordinates are otherwise opaque and carried unchanged; whether they are truthful is BV0’s concern.

The BF2 oracle may still run once over the genuine production result to prove the BV0 path remains connected. Its non-clean verdict is not a BV0A acceptance condition and its violations are neither parsed nor attributed.

## B. BF2 reopening

Do not reopen BF2. Additive structured violations are unnecessary for this design.

Structured payloads would be better than human-readable parsing if violation attribution were legitimate, but attribution itself is unnecessary. BV0A proves a map transformation invariant directly. BV0 ultimately requires an empty violation list, so it also does not need violation identities for acceptance.

Therefore:

- Delete AMD-008’s 18-rule identity table.
- Delete generated-label rewriting.
- Delete standalone oracle probes.
- Delete injective violation matching.
- Keep BF2’s oracle and invocation unchanged.

A future structured diagnostic API would be an independent BF2 maintainability change, not a prerequisite for BV0A or BV0.

## C. Independent reference computation

Use an input-only, cross-language N-version reference in the conformance harness.

Its independence property is:

> The reference and production implementation may share only immutable pre-assembly inputs and the V3 wire specification. They share no code that computes rewrite matches, rewrite geometry, fragment placement, source/name remapping, boundary insertion, segment ordering, or encoding.

Concretely, the reference:

- receives the original script/template code and maps, `meta`, and `profile`;
- independently performs the exact assembly write grammar in JavaScript;
- independently applies the two rewrite passes;
- independently constructs expected code, placement, tables, boundaries, and ordered segments;
- compares expected code to production code before comparing maps;
- never reads the production map while constructing the expectation;
- never consumes production splice lists, placement traces, origin spans, or map-composition helpers;
- never supplies its reference map to BF2 as the candidate.

This test-only reference is not a competing production assembler. It is an oracle. AMD-007’s prohibition on a harness-synthesized candidate map remains intact because BF2 always receives the genuine production map.

The current phrase “independently witnessed assembled placement” is unacceptable because it names no witness. Reusing production placement or splice helpers would be circular.

## D. Fragment/origin ownership

Carry ownership at ingestion:

```text
OriginId = Script | Template

InputSegment {
  origin: OriginId,
  ordinal: u32,
  local_source_index?,
  local_name_index?,
  fields...
}
```

Every source and name table row likewise carries `(origin, local_index)`. These tags survive rewrite chaining, placement, and table remapping. Assembly-created sourceless boundaries carry `AssemblyBoundary`; rewrite-created boundaries carry the script origin plus pass/occurrence identity.

The final V3 wire map need not serialize these tags. They are composition-time provenance and failure-reporting data.

Never infer ownership from final coordinates, fragment spans, source spelling, or table deduplication. Exact ordered comparison makes coordinate-based violation attribution unnecessary anyway.

## E. Normative rewrite model

The real `CodeTransform` semantics are normative. A distinct point-rebasing/clamping policy is wrong.

The current assembler performs two post-generation string replacements [compile.rs](crates/verter_session/src/compile.rs:74). The correct implementation is two sequential code-and-map transforms:

```text
M1 = chain(rename_transform_map, input_script_map)
M2 = chain(removal_transform_map, M1)
assembled = place(M2) + place(template_map)
```

Pass order is fixed:

1. Globally overwrite every `__sfc__` with `_sfc_main`.
2. On the pass-one output, globally remove every `export default _sfc_main;\n`.

Each pass must drive both its output code and transform map. Because the second pass addresses the first pass’s coordinate space, these are sequential transforms, not one overlapping edit batch.

`Chunk::Overwritten` emits one token at replacement start mapped to the overwritten range’s original start; it does not preserve and clamp every segment inside the replaced range [source_map.rs](crates/verter_compiler/src/code_transform/source_map.rs:243). Removal emits no replacement token. The following surviving original chunk supplies the transition from the removed range’s old end.

Accordingly:

- Segments inside an overwritten rename range are not individually clamped and retained.
- The replacement receives the single `Overwritten` mapping defined by the transform and chained through the input fragment map.
- Segments inside a removed range disappear.
- Required transition/boundary segments are retained or introduced according to transform semantics.
- Segments outside edited ranges preserve their relative wire order.
- A generated coordinate outside the fragment or between UTF-16 surrogate halves is rejected as uncomposable; it is never forced through a byte conversion.

The project’s compiler/codegen and position-encoding rules reinforce this ruling: one transformation model must own both bytes and mappings, with UTF-8/UTF-16 conversion performed only at checked boundaries.

AMD-007/BV0A must therefore be amended where it says `CodeTransform` is merely optional. The custom `splice_offset`/clamping work is the wrong abstraction and should be removed, not repaired.

## F. Minimal discriminating controls

Use one comprehensive positive fixture and five production-path mutations:

1. **Positive exact artifact**

   A real script-plus-template assembly containing:

   - two rename occurrences;
   - a global removal beginning mid-line;
   - non-ASCII text before mappings;
   - multiline synthetic prelude;
   - duplicate source and name spellings across fragments;
   - at least two equal-coordinate segments in a deliberate order;
   - source-bearing and sourceless segments.

   Assert exact code, tables, and ordered segment sequence.

2. **Order mutation**

   Swap the two equal-coordinate segments. The exact comparator must fail while a multiset comparator would pass.

3. **Rewrite-semantics mutation**

   Replace `Overwritten` behavior with AMD-008’s preserve-and-clamp behavior, or omit the post-removal old-end transition. The reference must fail.

4. **Placement mutation**

   Add one to the template’s write-time starting line, through the real production assembler/composer path. The reference must fail.

5. **Table mutation**

   Remap one template segment through the script fragment’s table base where duplicate spellings make a value-only/deduplicating comparator appear plausible. The reference must fail.

6. **Synthetic-provenance mutation**

   Inject one source-bearing segment at an assembly scaffolding boundary. The reference must fail because the expected stream contains either no segment there or a sourceless terminator.

Also retain one fail-closed input control for a requested but absent/undecodable fragment map.

This small set discriminates the actual architectural risks: order loss, wrong rewrite semantics, wrong placement, wrong table ownership, fabricated provenance, and vacuous input handling. The current large per-rule mutant matrix proves the brittleness of a diagnostic parser, not assembly neutrality.

## G. Interim scope

Yes, AMD-008 is radically over-engineered for an interim predecessor.

The correct BV0A exit is only:

> For every applicable map-enabled BV0 seed cell, the genuine production assembler returns code and a V3 map together. The code remains byte-identical to the pre-BV0A output. After decoding in wire order, the complete map artifact—table policy, source/name remapping, sourceless boundaries, and ordered segment sequence—equals an independently implemented input-only reference under the two authorized sequential `CodeTransform` rewrites and exact write-time fragment placement. Map-disabled cells return no map. Missing or uncomposable required input maps fail closed. BF2’s authored-source oracle remains unchanged and connected, but residual fragment-emitter violations are BV0’s acceptance responsibility.

That is sufficient, honest, and materially stronger than violation attribution. BV0 still owes the literal 36-cell clean oracle verdict.

The architecture ruling is therefore: **replace AMD-008’s mechanism, do not patch it; keep the single typed assembler result and write-order design, adopt exact ordered reference equality, use real `CodeTransform` rewrite semantics, and delete all oracle-detail parsing and violation matching.**
