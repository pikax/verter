# AMD-008 rounds 4 and 5 — review record

After the round-3 `BLOCK` (see
[`amd-008-round3-reviews.md`](amd-008-round3-reviews.md)), §2 was rebuilt on the
architecture ruling's design: exact ordered map-artifact equality against an
independent input-only reference, real `CodeTransform` rewrite semantics,
ingestion-time origin tags, and deletion of all oracle-violation attribution.

- **Round 4** reviewed commit `9427e0378b91254d1f05bdbee1c8a6c9b61f9981`,
  tree `109cbfebff31a6622bab187ca3da1c86e8c1f484`. All three mandates `BLOCK`.
- **Round 5** reviewed commit `2096ae221c3f8860299ac89ec67d31f9cec36149`,
  tree `289cb1c6c3e123bdb736dc5494d3beb8ee1dae0a`, after the round-4 findings
  were addressed. All three mandates `BLOCK`.

**The design thesis is not what is blocked.** Across both rounds no reviewer
challenged ordered-artifact equality, the `CodeTransform` rewrite model, the
deletion of violation attribution, or the decision not to reopen BF2. Round 4
conformance affirmatively confirmed the rewrite order, the 7-to-9 byte rename,
the `Chunk::Overwritten` token geometry, and the necessity of positional
comparison; round 5 architecture explicitly CLEARED both the circularity and
the supersession-scope axes.

**What is blocked is the attempt to specify the map-composition algebra in
prose.** Each round closed the named gaps and the next round found the next
layer in the same places — the chaining algebra's edge cases, the canonical
output schema's determinacy, the reference's auditable independence, the
totality of the uncomposable-input taxonomy, and per-field control coverage.
Round 5 conformance finding 7 and round 4 architecture finding 3 converge on
the same remedy: a named, immutable set of literal input/output vectors,
authored by neither implementation and audited independently of both, rather
than more prose.

The verdicts and findings are reproduced verbatim below.

---

## Round 4 — conformance

VERDICT: BLOCK

1. **BLOCKING — `chain(...)` is not a deterministic or occurrence-preserving rewrite algorithm.**

   Amendment:

   > “`M1 = chain(rename_transform_map, input_script_map)`”  
   > “Segments outside edited ranges keep their relative wire order.”

   The stated `Chunk::Overwritten` behavior is correct, but ordinary `CodeTransform` map generation cannot by itself preserve the complete input segment stream:

   - Explicit source-map locations are sorted and deduplicated before emission, collapsing equal-coordinate occurrences: [source_map.rs:128–139](crates/verter_compiler/src/code_transform/source_map.rs:128).
   - Original chunks emit their start, line starts, and registered locations—not every segment from an input map: [source_map.rs:181–195](crates/verter_compiler/src/code_transform/source_map.rs:181), [source_map.rs:484–569](crates/verter_compiler/src/code_transform/source_map.rs:484).
   - The accepted decoder genuinely preserves multiple occurrences at one coordinate because a zero generated-column delta pushes another segment without replacement: [sourcemap.mjs:77–103](packages/framework-conformance-harness/src/sourcemap.mjs:77).

   Therefore, a transform-map-first implementation can drop ordinary interior segments and collapse equal-coordinate script segments, contradicting positional artifact equality.

   The amendment also does not define chaining bias for:

   - multiple input segments at the queried coordinate;
   - a sourceless last-applicable segment;
   - a line with no applicable segment;
   - transition-token naming and table remapping.

   Correction: specify an ordered input-map rewrite API owned by `CodeTransform`, or an equivalent exact algorithm that consumes every tagged input segment occurrence. It must preserve every untouched occurrence and ordinal, drop all occurrences inside an overwrite, inject the single overwrite-start occurrence using a defined last-applicable lookup rule, and define sourceless/no-match behavior. Add a control with equal-coordinate segments in the **script** map across both passes.

   A smaller factual correction is also required. The text says:

   > “the following surviving original chunk supplies the transition”

   That is true only if such a chunk exists. Empty overwrite emits no token: [source_map.rs:243–250](crates/verter_compiler/src/code_transform/source_map.rs:243). The normal compiler footer is terminal: [process.rs:622–626](crates/verter_compiler/src/script/process.rs:622). State “if one exists”; terminal deletion has no old-end transition.

2. **BLOCKING — the reference does not receive all real assembler inputs, and its independence remains auditable only by judgment.**

   Amendment:

   > “the reference receives the original script and template code and maps plus the assembly meta/profile”

   The real function also receives `canonical_id` and the complete `RuntimeCompileOutput`: [compile.rs:21–26](crates/verter_session/src/compile.rs:21). Exact output depends on inputs omitted by the quoted contract:

   - style/custom-block inventory and `canonical_id`: [compile.rs:34–45](crates/verter_session/src/compile.rs:34);
   - template runtime and SSR imports: [compile.rs:48–72](crates/verter_session/src/compile.rs:48);
   - `scope_id`: [compile.rs:90–94](crates/verter_session/src/compile.rs:90);
   - development `__file__`, HMR, `ssr_module_id`, and runtime-module policy: [compile.rs:118–160](crates/verter_session/src/compile.rs:118).

   Thus the stated input signature cannot independently reconstruct expected code for the real write grammar.

   Separately, two copied implementations in the same Rust test module could “share no code” while retaining identical mistakes. The prior ruling explicitly required a cross-language harness reference and prohibited production placement/splice data: [amd-008-round3-reviews.md:494–511](docs/arch/refactor/rev11/evidence/BV0A/amd-008-round3-reviews.md:494); that enforceable boundary is absent from the current text.

   Correction: define a serialized preassembly input DTO containing `canonical_id`, every `RuntimeCompileOutput` field consumed by assembly, `FileMeta`, and `CompileProfile`. Require the reference to live in the JavaScript conformance harness with no dependency on Rust composition/rewrite code, and require an explicit dependency/import audit.

3. **MAJOR — `MapArtifact` is not complete, and table/global metadata policy is not uniquely specified.**

   Amendment:

   > “The compared subject is the complete decoded map ARTIFACT”  
   > `MapArtifact { version, source_root, sources[], sources_content[], names[], ordered_segments[] }`

   Real compiler fragment maps carry the optional V3 `file` field:

   - `SourceMapOptions` exposes it: [source_map.rs:10–18](crates/verter_compiler/src/code_transform/source_map.rs:10).
   - It is passed into the emitted map: [source_map.rs:123–124](crates/verter_compiler/src/code_transform/source_map.rs:123), [source_map.rs:383–392](crates/verter_compiler/src/code_transform/source_map.rs:383).
   - Both script and template compilation set it from `filename`: [compile/mod.rs:1002–1008](crates/verter_compiler/src/compile/mod.rs:1002), [compile/mod.rs:1258–1264](crates/verter_compiler/src/compile/mod.rs:1258).

   A corrupted or arbitrary assembled `file` therefore passes the purported complete-artifact comparison.

   The table policy also remains implicit. The repository’s existing JS composer deduplicates sources by spelling/content and names by value: [sourcemap.mjs:184–200](packages/framework-conformance-harness/src/sourcemap.mjs:184). The amendment’s “table base” mutation instead presupposes append-with-duplicates. The reference cannot independently derive one unique choice from the present prose.

   This also weakens origin tagging:

   > “Every source and name table row likewise carries `(origin, local_index)`.”

   One singular tuple cannot describe an output row deduplicated from both fragments.

   Correction: include `file` in `MapArtifact`, or explicitly require it absent and retract “complete.” Normatively define `file`, `sourceRoot`, table ordering, duplicate preservation, and absent/null/short `sourcesContent` handling. Either forbid deduplication and append script rows before template rows, or attach an ordered contributor set to each deduplicated output row.

4. **BLOCKING — `UncomposableInputMap` is neither total nor crisp over real input shapes.**

   Amendment:

   > “a requested map that is absent; undecodable `mappings`; a source or name index with no corresponding table row; incompatible table metadata…”

   Missing cases include:

   - malformed source-map JSON—the production carrier stores maps as strings: [carrier_compiler.rs:520–547](crates/verter_compiler/src/framework_common/carrier_compiler.rs:520);
   - missing or wrong `version`; the oracle separately rejects non-3 versions: [mapping-oracle.mjs:1109–1118](packages/framework-conformance-harness/src/mapping-oracle.mjs:1109);
   - absent/non-string `mappings`; the accepted oracle converts an absent member to `""`, which decodes successfully rather than entering “undecodable mappings”: [mapping-oracle.mjs:1113–1118](packages/framework-conformance-harness/src/mapping-oracle.mjs:1113);
   - malformed source/name table row types: [mapping-oracle.mjs:1143–1163](packages/framework-conformance-harness/src/mapping-oracle.mjs:1143), [mapping-oracle.mjs:1217–1229](packages/framework-conformance-harness/src/mapping-oracle.mjs:1217);
   - unsupported V3 index/`sections` maps;
   - undefined “incompatible table metadata,” especially `file`, `sourceRoot`, and `sourcesContent`.

   “Requested but absent” is also ambiguous for template-only Vapor/SSR. The compiler intentionally creates a synthetic script block with an empty source map: [compile/mod.rs:1045–1066](crates/verter_compiler/src/compile/mod.rs:1045), while the assembler sees `Some(script)` and emits it: [compile.rs:74–89](crates/verter_session/src/compile.rs:74). This must be treated as synthetic sourceless code, not automatically as a missing required authored-fragment map.

   Correction: define an exhaustive typed preflight taxonomy covering JSON, version, map shape, table types, references, global metadata, sections, and generated-coordinate representability. Derive map-requiredness from the preassembly authored-fragment inventory—e.g. `FileMeta.has_script`—not merely source-map request state or `compiled.script.is_some()`.

5. **BLOCKING — the controls do not discriminate every claimed failure mode.**

   Amendment:

   > “One comprehensive positive fixture plus six mutations”

   Only five mutations are listed: order, rewrite, placement, table, and synthetic provenance. Item 1 is the positive fixture, not a mutation.

   Further deficiencies:

   - The rewrite mutation permits preserve-and-clamp **or** missing transition, so only one behavior must be planted.
   - The fixture does not require multiple input segments inside a rename range or a mapped survivor after the removed range; without those geometries, the proposed rewrite mutant can be non-discriminating.
   - The placement mutant does not require assembled code to remain unchanged, so it may fail at the preceding code comparison without testing map placement.
   - The fail-closed control covers absent **or** undecodable only, leaving invalid indices, metadata incompatibility, out-of-fragment coordinates, surrogate-half coordinates, wrong version, malformed JSON, and invalid table shapes unplanted.
   - No control exercises chain behavior for same-coordinate script segments, a sourceless last-applicable segment, or no applicable segment.
   - The table mutant assumes non-deduplicated bases although that policy is not declared.

   Correction: say “five mutations” or add a sixth, plant both rewrite defects separately with mandatory discriminating geometry, require placement-only mutation with identical code bytes, and add one plant for every fail-closed category and every chaining-bias arm.

6. **BLOCKING — §4 does not fully supersede the conflicting ratified `CodeTransform` language.**

   AMD-008 correctly identifies the two main locations:

   - AMD-007 owned scope: [AMD-007:129–136](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:129);
   - AMD-007 Required procedure: [AMD-007:242–249](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:242);
   - corresponding BV0A text: [BV0A.md:27–34](docs/arch/refactor/rev11/charters/BV0A.md:27), [BV0A.md:146–153](docs/arch/refactor/rev11/charters/BV0A.md:146).

   But AMD-008 says:

   > “This amendment supersedes exactly two things and no more.”

   It leaves two additional conflicts standing:

   - Both the inline AMD-007 charter and `BV0A.md` say the interim design “does not require a chunk IR”: [AMD-007:159–167](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:159), [BV0A.md:63–71](docs/arch/refactor/rev11/charters/BV0A.md:63).
   - AMD-007’s recorded maintainer ratification expressly says “no CodeTransform/chunk-IR mandate”: [AMD-007:512–526](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:512).

   AMD-008 also retains:

   > “no harness copy or duplicate assembly path”

   while §2(a) requires a test oracle that independently reproduces the full assembly write grammar. The intended production/test distinction is inferable but not stated.

   Correction: explicitly supersede all three additional phrases, including AMD-007 §8.1’s ratification wording. Narrow “no harness copy” to “no harness-synthesized BF2 candidate map and no duplicate production assembly path,” expressly excluding the mandated independent test oracle.

7. **MAJOR — the Required Exits do not uniquely define BF2 connectivity or applicability.**

   Amendment:

   > “BF2’s authored-source oracle remains unchanged and connected, but residual fragment-emitter violations are BV0’s acceptance responsibility, not BV0A’s.”

   The accepted comparison currently makes every non-clean mapping result part of the overall verdict: [compare.mjs:453–487](packages/framework-conformance-harness/src/compare.mjs:453). Therefore “runs but does not gate” needs an exact outer procedure: which accepted entry point is invoked, what proves the mapping axis ran, and which result fields are intentionally excluded from BV0A acceptance.

   Map-disabled behavior itself is mechanically clear—the oracle accepts `map == null` when `sourceMapRequested == false` and rejects an unexpected map: [mapping-oracle.mjs:1096–1102](packages/framework-conformance-harness/src/mapping-oracle.mjs:1096)—but “applicable” and “required input map” are not bound to a named 36-cell manifest or preassembly fragment inventory.

   Correction: bind applicability to the exact locked seed records; require the BF2 mapping axis to report `ran` once for every cell; independently assert map presence for every map-enabled cell and absence for every map-disabled cell; and specify the BV0A wrapper that records BF2’s unchanged report while excluding only its non-clean mapping verdict from the BV0A gate.

Confirmed non-findings:

- Positional comparison is necessary. The decoder preserves wire order: [sourcemap.mjs:70–105](packages/framework-conformance-harness/src/sourcemap.mjs:70). The oracle preserves equal-coordinate relative order while sorting by column and `resolveAt` selects the last applicable segment: [mapping-oracle.mjs:1165–1171](packages/framework-conformance-harness/src/mapping-oracle.mjs:1165), [mapping-oracle.mjs:1033–1047](packages/framework-conformance-harness/src/mapping-oracle.mjs:1033).
- Rewrite order is correct: rename, then global removal: [compile.rs:82–86](crates/verter_session/src/compile.rs:82).
- `__sfc__` is 7 ASCII bytes and `_sfc_main` is 9.
- Non-empty overwrite emits one replacement-start token; empty overwrite emits none: [source_map.rs:243–275](crates/verter_compiler/src/code_transform/source_map.rs:243).
- BV0 still retains the literal full 36-cell authored-oracle-clean exit: [BV0.md:52–64](docs/arch/refactor/rev11/charters/BV0.md:52).

---

## Round 4 — architecture

VERDICT: BLOCK

1. **BLOCKING — SOUNDNESS: the “complete decoded map artifact” is not complete, and decoding does not prove that the encoded map is valid.**

   Exact positional equality does correctly close the earlier masking, equal-coordinate reordering, listed-table corruption, and boundary-segment holes—provided the reference is correct. But the artifact in [AMD-008 §2.1](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:103) omits observable V3 state:

   - `file`;
   - `ignoreList` and its source-index remapping;
   - indexed-map `sections`;
   - any recognized semantic extension policy.

   Concrete passing bugs:

   - Production emits the expected regular `mappings` plus a wrong `sections` member. The amendment compares `mappings`; a conforming consumer selects the indexed map whenever `sections` is present.
   - Production remaps `sources[]` correctly but leaves `ignoreList` pointing at the pre-merge source index. Equality passes while debuggers classify the wrong source as third-party.
   - Production retains the script fragment’s `file` value instead of the assembled module identity. Equality passes.

   There is also an encoded-map counterexample in the accepted decoder. I verified that both `A` and `ggggggE` decode to the same sourceless segment at `0:0` through [decodeVlqSegment](packages/framework-conformance-harness/src/sourcemap.mjs:29). Mathematically, `ggggggE` encodes an out-of-range 2³² quantity; JavaScript’s 32-bit shifts wrap it to zero. ECMA-426 requires conforming generators to avoid decode errors and defines values beyond the 32-bit limit as invalid. [The specification also defines `file`, `ignoreList`, and indexed `sections` as observable source-map structure.](https://tc39.es/ecma426/)

   **Required change:** Define one allowed top-level output schema—preferably a flat regular V3 map—and reject `sections` and unsupported semantic extensions. Add exact policies for `file` and `ignoreList`. Parse the actual wire artifact with an independent, spec-conformant validator that checks JSON field types, VLQ range/overflow, accumulated non-negative indices and coordinates, and regular-versus-indexed shape before logical comparison. Do not require raw `mappings` byte equality; valid equivalent encodings need not be byte-identical.

2. **BLOCKING — STRENGTH/DETERMINACY: exact equality currently delegates normative representation choices to the reference implementation.**

   [§2.1 says the reference “pins” table policy](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:133), but the amendment does not itself define:

   - stable concatenation versus any permitted normalization;
   - handling of unused table rows;
   - absent versus empty `names`;
   - absent `sourcesContent` versus per-source `null`;
   - absent, `null`, and empty `sourceRoot`;
   - exact compatibility conditions between fragment metadata;
   - `file` and `ignoreList` policy.

   The intended “preserve each origin/local ordinal without deduplication” policy can be inferred, but an acceptance contract cannot make the as-yet-unreviewed reference source code the sole place where that policy becomes normative. Two semantically correct V3 outputs can otherwise differ and one will fail arbitrarily.

   Exact ordered equality is not inherently too strong for this block: preserving occurrences and wire order is a defensible neutrality contract. It becomes too strong when the canonical representation has not first been ratified.

   **Required change:** Specify the complete flat-map construction algorithm in the amendment: field presence, stable table append/remap by `(origin, local_index)`, duplicate and unused-row handling, `sourceRoot`, `sourcesContent`, `names`, `file`, and `ignoreList`. Define `UncomposableInputMap` against those exact rules.

3. **BLOCKING — INDEPENDENCE: §2.2(a) establishes code independence, not semantic independence.**

   The “no shared code/helpers/traces” property in [§2.2(a)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:160) is necessary and structurally auditable through imports and input DTOs. It does not prevent an implementer from translating the same mistaken algorithm into both implementations.

   The main common-mode opening is `chain(...)`. V3 defines mappings, but it does not supply the amendment’s required multi-level composition algorithm. [CodeTransform](crates/verter_compiler/src/code_transform/source_map.rs:243) defines the outer rewrite token geometry; the amendment does not define how that token is chained through:

   - multiple upstream segments at one coordinate;
   - sourceless barriers;
   - a gap with only an earlier mapping;
   - named versus unnamed upstream occurrences;
   - old-end transitions after removal.

   Concrete bug: an untouched input coordinate contains ordered mappings `(p→A), (p→B)`. Both implementations create one transform-map location at `p` and perform a conventional last-applicable lookup, emitting only `B`. They share no code, exact equality passes, but composition dropped `A` and violated the preserved-occurrence/order contract.

   Another common-mode bug is counting Unicode scalar values instead of UTF-16 units. The positive control requires only “non-ASCII,” which can be a one-unit BMP character and does not discriminate this error.

   **Required change:** Ratify the chaining algebra explicitly, including duplicate-coordinate expansion/order, sourceless state, names, gaps, and edit-boundary ownership. Add hand-enumerated literal vectors produced by neither implementation. Include astral-plane text, CRLF, duplicates inside and outside edits, and `p = start/end` cases. A cross-language N-version reference is useful supporting evidence, but it is not a sufficient oracle by itself.

4. **BLOCKING — BOUNDARY: the ownership split is conceptually right, but two acceptance blind spots remain.**

   For fields actually represented, the split is sound:

   - original authored tuples are carried opaquely to BV0;
   - rewrite, placement, remapping, ordering, and synthetic boundaries remain BV0A-owned;
   - a missing, added, or reordered boundary segment differs from the reference and therefore fails.

   However, the defects from finding 1—wrong `file`, wrong `ignoreList`, mixed `sections`, or invalid-but-leniently-decoded VLQ—are assembly-owned defects that both the artifact comparator and [BF2’s oracle](packages/framework-conformance-harness/src/mapping-oracle.mjs:1109) can miss. BV0’s later clean verdict cannot close those blind spots.

   Applicability is also not bound. The exit says “applicable map-enabled” cells and “map-disabled” cells in [§2.3](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:286), but does not name the immutable authority that partitions all 36 cells. A candidate can omit a troublesome map and derive “disabled/not applicable” from production state; BF2 still runs, but its `map-presence` failure is expressly non-gating.

   **Required change:** Partition all 36 cells from the locked BF2 request/options manifest, not from candidate map presence. Require an accounting assertion with no unclassified cell. Derive fragment applicability from the pre-assembly compilation inventory; missing maps must not erase applicability.

5. **CLEARED — CIRCULARITY: the original DAG circularity is genuinely broken.**

   BV0A no longer depends on emitter truthfulness or an oracle-clean verdict. It proves a transformation over mechanically composable inputs; BV0 subsequently corrects those inputs until BF2 is clean. Keeping BF2 connected but non-gating is consistent with that division.

   The common-mode reference risk above is a new verification-soundness problem, not the original `BV0A → BV0 → clean oracle needed by BV0A` cycle.

6. **BLOCKING — SCOPE: the supersession declaration does not cover all conflicting ratified text.**

   [AMD-008 §4](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:352) claims to supersede exactly the §3 acceptance sections and two CodeTransform-optional passages. Conflicting AMD-007 text remains outside that declaration:

   - [AMD-007 §1](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:41) still defines BV0A correctness under BF2’s authored-source contract.
   - [AMD-007 §7](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:449) still says mapping correctness is governed by BF2’s oracle.
   - The recorded ratification explicitly says [“no CodeTransform/chunk-IR mandate”](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:512).
   - [BV0A’s retained post-list paragraph](docs/arch/refactor/rev11/charters/BV0A.md:63) still says the interim does not require a chunk IR.

   **Required change:** Enumerate and supersede every conflicting sentence, including the relevant §8.1 ratification clause. Clarify whether production must literally use `CodeTransform` or merely reproduce its pinned semantics, and distinguish that local rewrite requirement from a forbidden whole-module/B4 chunk IR.

7. **MAJOR — RIGHT-SIZING: the redesign removed the wrong machinery, but remains over-specified in implementation shape and under-specified in correctness fundamentals.**

   Correct cuts: oracle-detail parsing, standalone probes, violation attribution, and the per-rule mutant matrix should stay deleted.

   What should be cut or relaxed:

   - Mandatory production origin tags are unnecessary if a canonical stable append/remap algorithm establishes ownership; origin tags can remain reference-side and diagnostic-only.
   - The N-version implementation should be bounded to the rewrite/composition/write algebra, not become a general second production assembler.

   What must be added:

   - strict independent V3 wire validation;
   - a complete output-field policy;
   - normative chaining and table algorithms;
   - immutable 36-cell applicability accounting;
   - literal hand-authored expected vectors;
   - astral UTF-16, invalid-VLQ, `sections`, `ignoreList`, and `file` mutations.

The ordered-reference thesis is directionally correct and the original circularity is resolved, but the current amendment can still accept observable assembly corruption and can reject a valid implementation based on policies that exist only inside the future reference code.

---

## Round 4 — adversarial / governance

VERDICT: BLOCK

Reviewed commit `9427e0378b91254d1f05bdbee1c8a6c9b61f9981`, tree `109cbfebff31a6622bab187ca3da1c86e8c1f484`.

1. BLOCKING — Map applicability remains candidate-gameable.

Quoted text:

> “For every applicable map-enabled cell of the 36 exact BV0 seed cells…”

> “Map-disabled cells return no map, and the assembled probe still runs there to prove absence…”

> “BF2’s … verdict is NOT a BV0A acceptance condition.”  
> [AMD-008 §2](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:236)

The amendment never identifies the authority deciding “map-enabled” or “applicable.” The accepted harness already has such an independent authority: `record.options.sourceMap` in the golden provenance. [check-candidate.mjs](packages/framework-conformance-harness/src/check-candidate.mjs:90)

Exploit: classify a failing cell from candidate map presence or candidate-produced metadata. Return no map, call the cell map-disabled, skip equality, and let BF2 report `map-presence`; that verdict is explicitly non-gating.

Required correction: bind applicability exclusively to the locked BF2 manifest/invocation’s `sourceMap` input, enumerate the expected map-on/map-off counts, and derive script/template applicability from immutable pre-assembly inventory—never from map presence or production output. Add a mutation proving candidate state cannot turn a requested map into an exemption.

2. BLOCKING — “Independent reference” is not objectively independent, and the byte baseline is unfrozen.

Quoted text:

> “The reference and the production implementation may share only immutable pre-assembly inputs…”

> “Concretely the reference … independently performs the assembly write grammar…”

> “compares expected CODE to production code…”  
> [AMD-008 §2(a)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:160)

The architecture ruling required an “input-only, cross-language N-version reference in the conformance harness.” [Round-3 ruling](docs/arch/refactor/rev11/evidence/BV0A/amd-008-round3-reviews.md:494) The current text silently drops both “cross-language” and “in the conformance harness.”

It also requires byte identity with “pre-amendment output” but binds no baseline commit, fixture outputs, or digests. Since the former detailed write-order procedure is replaced in full, “assembly write grammar” can be copied from the candidate implementation.

Exploit: copy the Rust assembler’s behavior into a separate Rust test implementation without importing shared helpers. Change both copies to emit an extra newline or use an incorrect global-removal geometry. They share no code at runtime, receive only inputs, and compare equal.

Required correction: require the cross-language harness implementation explicitly, prohibit translation/copying from the candidate diff, and pin the pre-BV0A code outputs to a named commit/tree plus independently captured fixtures or digests. A production-only code mutation must produce a named RED while the reference bytes remain unchanged.

3. BLOCKING — Critical chain and table semantics are delegated to the reference rather than normatively specified.

Quoted text:

> “the assembler’s remap policy is pinned by the reference rather than left to the implementer”

and:

> `M1 = chain(rename_transform_map, input_script_map)`  
> `M2 = chain(removal_transform_map, M1)`  
> [AMD-008 §2](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:133)

`chain` is undefined. V3 specifies map representation, not the complete policy for composing two maps. The text does not define lookup bias, equal-coordinate selection, sourceless termination, name propagation, or table merging/deduplication. “Pinned by the reference” leaves the normative decision to test code.

Exploit: when chaining through two equal-coordinate input segments, both implementations select the first rather than the last applicable segment. Exact equality passes, while the actual oracle consumer selects the last applicable segment. [mapping-oracle.mjs](packages/framework-conformance-harness/src/mapping-oracle.mjs:1033)

Likewise, production and reference may both deduplicate duplicate source/name rows or choose the same unjustified `sourceRoot` normalization.

Required correction: specify the chaining algorithm field by field, including equal-coordinate/wire-order bias, sourceless boundaries, names, and original coordinates. Specify exact source/name/source-content merge order, duplicate policy, index remapping, and `sourceRoot` policy in normative text.

4. BLOCKING — The control set can pass while the comparator omits contract fields.

Quoted text:

> “actual segment `i` must equal reference segment `i` field for field.”

But the mutations cover order, rewrite geometry, placement, one table-base error, and fabricated provenance. None proves comparison of:

- `original` line/column;
- `name_index`;
- `source_root`;
- `sources_content`;
- arbitrary segment omission or duplication;
- a required sourceless boundary being omitted;
- map applicability;
- the frozen code baseline.

A comparator that ignores `original` and `name_index` still detects every prescribed mutation. A production assembler can then corrupt original coordinates or names and pass.

The mutation-execution requirement is also insufficient:

> “must be proven to have actually applied — present, unique, and new in the source before the run…”  
> [AMD-008 §2(e)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:245)

A unique source mutation that causes compilation or setup failure satisfies “must FAIL” without proving the equality gate discriminated.

Required correction: add one-field mutations for every artifact field and sequence cardinality, plus applicability, code-baseline, chain-bias, and missing-boundary controls. Each mutation must prove the candidate artifact was emitted with the intended changed property, the independent reference stayed unchanged, the named equality assertion produced RED, nonzero work ran, and no earlier failure supplied the RED.

5. BLOCKING — The claimed supersession set leaves conflicting ratified authority alive.

Quoted text:

> “This amendment supersedes exactly two things and no more.”  
> [AMD-008 §4](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:352)

But AMD-007 §1 still says:

> “BV0A owns … a real, correct map — correct under BF2’s accepted authored-source contract…”  
> [AMD-007 §1](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:41)

BV0A owned-scope item 4 still says the result is:

> “validated through BF2’s reaccepted authored-source oracle”  
> [BV0A charter](docs/arch/refactor/rev11/charters/BV0A.md:54)

And AMD-007’s recorded ratification expressly authorized:

> “no CodeTransform/chunk-IR mandate”  
> [AMD-007 §8.1](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:512)

AMD-008 names only the duplicated owned-scope/procedure clauses, not these additional ratified statements.

Required correction: explicitly supersede AMD-007 §1’s BF2-clean definition, amend BV0A owned-scope item 4 to mean delivery to the unchanged oracle rather than a clean verdict, and supersede the conflicting phrase in AMD-007 §8.1.

6. MAJOR — `UncomposableInputMap` contains an undefined rescope escape.

Quoted text:

> “incompatible table metadata between fragments”

> “These FAIL BV0A closed (or rescope to their true owner)”  
> [AMD-008 §2 item 4](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:307)

“Incompatible table metadata” has no closed definition. It could include perfectly composable differing `sourceRoot` values, absent versus `null` content entries, or duplicate table rows. The text also does not require classification against immutable raw inputs before production rewriting.

Exploit: classify a difficult but valid table combination as incompatible, or validate coordinates after a defective rewrite and blame the resulting out-of-range coordinate on the input. The currently required fail-closed control covers only absent/undecodable maps.

Required correction: define closed, machine-checkable predicates for every category, run them against recorded immutable pre-assembly inputs, state explicitly that an uncomposable result halts BV0A and cannot alter applicability, and add a control for every category.

7. MAJOR — The “complete decoded map artifact” is not complete.

Quoted text:

> “The compared subject is the complete decoded map ARTIFACT”

But `MapArtifact` omits the standard optional `file` member, even though the Rust map generator carries it into `SourceMap::new`. [source_map.rs](crates/verter_compiler/src/code_transform/source_map.rs:123)

Exploit: production emits an incorrect or invented `file`; the declared exact comparator cannot observe it.

Required correction: either include all accepted correctness-bearing V3 members—including optional-member presence/null distinctions—or call this an exact logical projection and explicitly define omitted fields and their separate validation/rejection policy.

8. MAJOR — §1 overstates what the blocked implementation established.

Quoted text:

> “BV0A’s implementation attempt proves a real, valuable, but NARROWER claim…”

The same section later admits its pooled violation matching could consume an assembly-introduced violation using a spurious fragment entry. [AMD-008 §1](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:21) That mechanism did not prove the asserted neutrality claim.

Required correction: say the attempt “sought to prove” the narrower claim but failed to establish it. Also qualify “materially stronger than attribution” as stronger specifically for assembly-neutrality—not for authored-source correctness.

The oracle’s non-gating status is not inherently a waiver. The sentences that save the ownership split are:

> “Whether [authored coordinates] tell the truth … is BV0’s concern; BV0A’s objective is only that composition neither invents, drops, reorders, duplicates, nor perturbs them.”

and:

> “BV0’s own literal 36/36 fully-oracle-clean exit is unchanged…”  
> [AMD-008 §§1–2](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:82)

BV0’s charter still requires all 36 cells to pass the authored-source oracle. The redesign is therefore a principled scope split in intent, but the current BV0A gate remains exploitable.

Ratification hygiene itself is otherwise sound: §5 binds exact package and bundle commit/tree identities, separates amendment ratification from candidate acceptance, closes the prior bundle-diff hole, leaves §5.1 correctly pending, and accurately identifies the three BLOCK verdicts at `e78512f…`. No TODO, tracker, debt row, waiver, known-defect acceptance, or typed-success deferral is authorized as a substitute for BV0’s eventual fixes.

---

## Round 5 — conformance

VERDICT: BLOCK

1. BLOCKING — The rewrite algebra omits the post-overwrite transition for non-empty overwrites.

   Quoted text: “The following surviving original chunk supplies the transition …” is specified only for an “EMPTY overwrite” ([AMD-008:210](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:210)). For non-empty overwrites, the text specifies only one replacement-start token ([AMD-008:200](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:200), [AMD-008:302](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:302)).

   Real `CodeTransform` behavior is:

   - A middle or left-aligned overwrite creates an `Overwritten` chunk followed by an `Original` chunk ([code_transform.rs:617](crates/verter_compiler/src/code_transform/code_transform.rs:617)).
   - A non-empty `Overwritten` chunk emits one token at its start ([source_map.rs:243](crates/verter_compiler/src/code_transform/source_map.rs:243)).
   - The following `Original` chunk independently emits another token at its chunk start ([source_map.rs:181](crates/verter_compiler/src/code_transform/source_map.rs:181), [source_map.rs:505](crates/verter_compiler/src/code_transform/source_map.rs:505)).

   Thus every non-terminal rename has an old-end transition as well. The reference algorithm can legally omit it under the present text.

   Concrete correction: normatively require a transition for every overwrite—empty or non-empty—when a following `Original` chunk exists. Define it as the outer token at the overwritten range’s old end, chained through the input map’s line-scoped last-applicable lookup. Add a separate RED control that deletes the transition after a non-terminal rename.

2. BLOCKING — Ordering is not defined when rewriting makes previously different positions collide.

   Quoted text: surviving occurrences preserve only “its ORDINAL among equal-coordinate occurrences” ([AMD-008:197](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:197)). The text does not order:

   - surviving occurrences whose distinct input coordinates collapse after deletion;
   - an overwrite-start token against an existing occurrence at the same output coordinate;
   - an old-end transition against another shifted occurrence;
   - an assembly-boundary token against a fragment token.

   These ties are semantically observable. The decoder retains every zero-delta occurrence in wire order ([sourcemap.mjs:76](packages/framework-conformance-harness/src/sourcemap.mjs:76)), while `resolveAt` selects the last applicable segment ([mapping-oracle.mjs:1039](packages/framework-conformance-harness/src/mapping-oracle.mjs:1039)). The real chunk emitter’s order is its chunk traversal order ([source_map.rs:178](crates/verter_compiler/src/code_transform/source_map.rs:178)).

   Concrete correction: define the complete output occurrence order as the exact `CodeTransform` chunk-emission walk, including placement/boundary emission, with a deterministic intra-coordinate precedence. Add controls for rewrite-induced collisions, not only the current swap of two already-equal input occurrences.

3. BLOCKING — `source_root` composition is internally contradictory and not implementable deterministically.

   Quoted text:

   - “Fragment source spellings are rebased into assembled-relative spellings” ([AMD-008:181](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:181)).
   - “source spellings … are carried OPAQUELY and unchanged” ([AMD-008:218](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:218)).

   Both cannot hold. Nor does the amendment define the base identity, path/URL resolution rules, normalization, or serialization required by “rebased into assembled-relative spellings.” The actual fragment producer sets both `file` and `source` from `options.filename` ([compile/mod.rs:1002](crates/verter_compiler/src/compile/mod.rs:1002), [compile/mod.rs:1258](crates/verter_compiler/src/compile/mod.rs:1258)).

   Likewise, “`file` is the assembled module’s own identity” ([AMD-008:161](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:161)) does not choose between the actual main identities: `canonical_id` and the generated `._VERTER_.bundle.ts` identity ([id.rs:183](crates/verter_session/src/id.rs:183)).

   Concrete correction: state the exact `file` value, define a complete source-root resolution and reserialization function with explicit bases, and change “unchanged” to permit only that specified reserialization. Specify the accepted emitted ignore-list key spelling as well.

4. BLOCKING — The universal mutation protocol is impossible for fail-closed and baseline controls.

   Quoted text: “Every mutation” must prove that “the candidate artifact was actually emitted,” “the independent reference was UNCHANGED,” and “the NAMED equality assertion produced the RED” ([AMD-008:378](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:378)).

   But the amendment also mandates wire validation before comparison ([AMD-008:142](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:142)). Malformed JSON, invalid VLQ, indexed maps, and invalid table indices therefore must fail before the equality assertion. The applicability control may intentionally prevent an artifact, and the byte-baseline control must fail at code comparison before map equality.

   Concrete correction: divide controls into explicit protocols:

   - artifact-equality controls: emitted artifact, unchanged reference, named equality RED;
   - wire/preflight controls: named validator/preflight RED before comparison;
   - applicability controls: named partition/absence RED;
   - code-baseline controls: named code comparison RED.

5. BLOCKING — The controls do not prove all compared fields or all declared failure categories.

   Quoted text: “One mutation per compared artifact field” ([AMD-008:356](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:356)).

   The sweep omits direct mutations for:

   - `version`;
   - generated line and generated column;
   - `source_index`;
   - the `Sourceless` versus `Mapped` variant;
   - ignore-list presence/absence;
   - ordered-segment collision precedence.

   Its fail-closed sweep claims “One plant per item 4 category” ([AMD-008:366](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:366)) but omits malformed `sourcesContent` rows and the entire “incompatible table metadata” category. It also has no control for the missing non-empty-overwrite transition from finding 1.

   “FIVE geometry mutations” ([AMD-008:324](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:324)) is misleading: the five numbered categories require eight separately planted mutations ([AMD-008:340](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:340)).

   Concrete correction: enumerate every executable control with a stable ID and expected rejecting assertion; add direct one-field mutations for every `MapArtifact` and `Segment` field and every preflight category.

6. BLOCKING — `UncomposableInputMap` is neither exhaustive nor judgment-free.

   Quoted text: the taxonomy is “exhaustive” and `UncomposableInputMap` “is exactly” the listed cases ([AMD-008:428](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:428), [AMD-008:434](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:434)).

   Missing or ambiguous cases include:

   - malformed table containers rather than merely malformed row types;
   - malformed `file`, `sourceRoot`, or ignore-list members;
   - ignore-list indices outside `sources`;
   - negative accumulated generated/source/name coordinates;
   - extra versus short `sourcesContent` arrays;
   - conflicting ignore-list metadata;
   - unknown semantic fields and `debugId`.

   These are real shapes recognized by the pinned `oxc_sourcemap` dependency ([Cargo.lock:2149](Cargo.lock:2149)). Its schema contains `file`, `sourceRoot`, `sourcesContent`, `debugId`, and both ignore-list spellings ([oxc decode.rs:9](oxc_sourcemap-7.0.0/src/decode.rs:9)); it explicitly rejects invalid ignore indices ([oxc decode.rs:52](oxc_sourcemap-7.0.0/src/decode.rs:52)) and negative accumulated coordinates ([oxc decode.rs:175](oxc_sourcemap-7.0.0/src/decode.rs:175)).

   Meanwhile, the accepted oracle silently defaults absent `sources` and `names` to empty arrays ([mapping-oracle.mjs:1121](packages/framework-conformance-harness/src/mapping-oracle.mjs:1121)). The amendment does not state whether BV0A normalizes these or rejects them.

   “A `sourceRoot` pair that cannot both be honoured” and arrays that “cannot be made index-parallel” ([AMD-008:443](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:443)) are conclusions, not deterministic predicates.

   Concrete correction: define a closed input DTO schema and a deterministic preflight decision table for every member and malformed state. Define precisely which shapes normalize and which produce `UncomposableInputMap`.

7. BLOCKING — Independence remains one-way and permits a tautological implementation.

   Quoted text: the JavaScript reference has “NO dependency … on Rust composition” and must not be “transcribed from the production implementation” ([AMD-008:241](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:241), [AMD-008:262](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:262)).

   This does not forbid the reverse dependency: production code or generated Rust fixtures could be derived from the JavaScript reference. Nor are the “literal hand-authored vectors” assigned mandatory coverage, stable identities, or independently pinned expected artifacts. A token vector unrelated to the hard rewrite/table cases would satisfy the literal wording.

   Concrete correction: prohibit dependency, generation, transcription, and shared executable composition helpers in both directions. Require a named, immutable set of literal input/output artifacts covering every rewrite, collision, table, boundary, and failure rule; audit those artifacts independently of both implementations.

8. MAJOR — Origin tags are insufficient as specified for actual-output failure attribution.

   Quoted text: origin “survives rewriting, placement, and table remapping,” is “failure-reporting data,” but “the emitted wire map does not serialize” it and production may retain it only “where it needs” it ([AMD-008:314](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:314)).

   The compared `MapArtifact` has no provenance field ([AMD-008:111](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:111)). Therefore an extra actual segment, an omitted expected segment, or a tie-order mismatch cannot be attributed from the comparison artifact without returning to coordinate/position inference. Input tagging does correctly distinguish duplicate table rows, fragment boundaries, and invalid fragment-local coordinates, but it does not prove provenance for unmatched actual occurrences.

   Concrete correction: require a test-only provenance sidecar with one entry per emitted segment occurrence and table row, compared or reported alongside the logical artifact. Alternatively, delete the failure-attribution claim and state that all exact-equality failures are BV0A-owned without finer attribution.

9. MAJOR — The name-binding statement contradicts the overwrite rules.

   Quoted text: “a rewrite never … drops a name binding” ([AMD-008:215](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:215)). Yet the immediately preceding rules drop every segment occurrence inside either overwrite kind ([AMD-008:200](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:200), [AMD-008:210](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:210)); those occurrences may carry `name_index`.

   Concrete correction: say that unused `names[]` table rows are retained, while named segment occurrences inside replaced ranges are dropped and any synthesized transition/overwrite occurrence receives the name selected by the specified chaining lookup.

10. MINOR — The supersession count is factually wrong, though the substantive list is mostly complete.

   Quoted text: “The `CodeTransform`/chunk-IR language, in all four places it appears” ([AMD-008:505](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:505)).

   The paragraph itself identifies three mirrored passage classes in each of two documents—six textual locations:

   - AMD-007 owned-scope item 2 ([AMD-007:126](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:126));
   - AMD-007 retained chunk-IR paragraph ([AMD-007:159](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:159));
   - AMD-007 Required procedure ([AMD-007:243](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:243));
   - the corresponding three BV0A passages ([BV0A:32](docs/arch/refactor/rev11/charters/BV0A.md:32), [BV0A:63](docs/arch/refactor/rev11/charters/BV0A.md:63), [BV0A:148](docs/arch/refactor/rev11/charters/BV0A.md:148)).

   AMD-007 §8.1 is a seventh occurrence handled separately ([AMD-007:520](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:520)). Owned-scope item 4 is also correctly enumerated separately.

   Concrete correction: replace “all four places” with “the six mirrored passages,” retaining §8.1 as the separately enumerated seventh occurrence.

Confirmed conforming points:

- §2’s positional-comparison rationale is correct: equal generated coordinates are representable, wire order is retained, and consumer resolution selects the last occurrence.
- The production rewrite order is correctly stated: rename first, removal second ([compile.rs:82](crates/verter_session/src/compile.rs:82)). `__sfc__` is 7 ASCII bytes and `_sfc_main` is 9.
- A non-empty `Chunk::Overwritten` emits one start token; an empty overwrite emits none. UTF-16 generated columns are also correctly described ([source_map.rs:574](crates/verter_compiler/src/code_transform/source_map.rs:574)).
- The revised exits clearly require exact equality for map-enabled cells, asserted absence for map-disabled cells, and BF2 execution without using its mapping verdict as the BV0A gate ([AMD-008:394](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:394)).

---

## Round 5 — architecture

VERDICT: BLOCK

The ordered-artifact approach is directionally correct and genuinely removes violation-attribution circularity. The current amendment is still unsound because its normative rewrite algebra omits behavior performed by the real `CodeTransform`, and its acceptance controls cannot prove the comparator described by the text.

1. CRITICAL — SOUNDNESS: non-empty overwrites omit the old-end transition

Design problem: AMD-008 says a non-empty overwrite replaces all in-range occurrences with exactly one occurrence at the replacement start ([AMD-008 §2.1(d)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:197), [§2.2(c)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:302)). It mentions the following-original-chunk transition only for empty overwrites.

That is not the real `CodeTransform` behavior. `Chunk::Overwritten` emits the replacement-start token, then a following `Chunk::Original` unconditionally emits another token at its chunk start ([source_map.rs](crates/verter_compiler/src/code_transform/source_map.rs:243), [emit_mapped_content](crates/verter_compiler/src/code_transform/source_map.rs:510)).

Concrete counterexample:

```text
input code:     p__sfc__qz
rename range:    [1, 8)
input segments: col 0 -> A
                col 7 -> B   (inside the overwritten range)
                col 9 -> C
```

Real transform geometry produces:

```text
output col 0  -> input col 0 -> A
output col 1  -> input col 1 -> A      replacement start
output col 10 -> input col 8 -> B      following Original chunk start, GLB
output col 11 -> input col 9 -> C
```

The written algebra drops `B`, emits the col-1 replacement token, and does not require the col-10 transition. Consequently `q` inherits `A`, not `B`. Production and the JavaScript reference can implement that same written rule, compare exactly equal, and pass.

The same section also lacks a complete generated-position transform for surviving occurrences and a deterministic merge order when a newly generated boundary collides with surviving equal-coordinate occurrences.

Why it matters: this is a real assembly-composition defect admitted by the proposed acceptance criterion, not an emitter defect.

Required change: specify the entire outer chunk token stream for every overwrite:

- Replacement-start token for non-empty overwrites.
- Following-original-chunk start token for every nonterminal overwrite, empty or non-empty.
- GLB/last-applicable chaining of each boundary through the input map.
- Exact generated-coordinate transformation for every surviving occurrence.
- Collision/coalescing and equal-coordinate ordinal rules.

Add a non-empty-overwrite old-end mutation and hand-authored vector. The current geometry control tests that transition only for removal ([AMD-008](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:343)).

2. HIGH — STRENGTH: canonical equality is underdefined where it removes legitimate freedom

Design problem: exact equality is acceptable only after every canonical choice is deterministic. Two choices remain unspecified:

- `file` is “the assembled module’s own identity,” but that does not choose between `canonical_id`, the virtual `.bundle.ts` identity, a basename, or another generated-module identifier.
- `sourceRoot` is removed by rebasing sources to “assembled-relative spellings,” but no source-map base URL, target base URL, or URL/path resolution algorithm is defined.

ECMA-426 deliberately leaves the interpretation of `file` to the generator and resolves `sourceRoot` plus `sources` relative to the source-map base URL. It also requires consumers to ignore unrecognized extensions. [ECMA-426](https://tc39.es/ecma426/)

Why it matters: two semantically correct implementations can choose different `file` or URL-equivalent source spellings, causing a false failure. Conversely, production and reference can choose the same incorrect ad hoc rebasing rule and pass.

The “exhaustive” `UncomposableInputMap` taxonomy is also not total. It does not clearly classify a valid JSON non-object, missing/non-array `sources`, malformed `file` or `sourceRoot`, malformed `ignoreList`, duplicate members, or legacy `x_google_ignoreList`. “Any semantic extension” is not a mechanically auditable wire rule.

Required change:

- Define the exact output `file` value as a function of a named DTO field.
- Define the input and output source-map base URLs and use a named URL-resolution algorithm.
- Replace “semantic extension” with an exact allowed-member policy, including `ignoreList` versus `x_google_ignoreList`.
- Make input validation total, preferably with a catch-all typed `SchemaInvalid { field, reason }`.
- Explicitly call this a stricter BV0A canonical profile where it intentionally rejects otherwise conforming V3 freedom.

Raw `mappings` byte equality itself is not needed: after strict validation, alternative valid VLQ encodings of the same ordered occurrence stream are not a mapping-semantic difference.

3. HIGH — INDEPENDENCE: structural independence is good, common-mode independence is unproven

Design problem: JavaScript location, no Rust dependency, and a single input-only DTO are appropriate and auditable ([AMD-008](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:241)). “Not transcribed from production” is a review instruction, not a structurally enforceable property. Cross-language translation can preserve the same misunderstanding—as finding 1 demonstrates.

Why it matters: exact equality proves only agreement. Without a third authority, N-version implementations can agree exactly on the same defect.

Required change: retain the input-only reference, but make the real authority:

- A complete normative edit/chaining algebra.
- Frozen, literal expected artifacts authored independently of both implementations.
- Vectors covering every edit boundary, collision, sourceless state, and table remap.
- Metamorphic invariants such as identity composition, unaffected-prefix preservation, and correct post-edit lookup.
- Recorded dependency audit and pinned vector digests.

N-version implementation is useful corroboration, but it is not sufficient as the primary proof.

4. CRITICAL — BOUNDARY: the BV0A/BV0 split is conceptually preserved, but not closed

Design problem: the amendment correctly says BV0A carries mechanically composable emitter mappings opaquely and BV0 later judges their authored truth ([AMD-008](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:452)). However, finding 1 permits a BV0A composition defect that both later rails can miss.

Choose `A` and `B` in the counterexample so both are in-bounds and satisfy an allowed relation—or place the affected generated token in one of the oracle’s acknowledged loose/uncovered regions ([mapping-oracle.mjs](packages/framework-conformance-harness/src/mapping-oracle.mjs:37)). The incorrect lack of a post-rename transition then:

- Matches the equally defective reference artifact.
- Remains oracle-clean in BV0.
- Is nevertheless not neutral under real transform semantics.

Why it matters: BV0’s later 36-cell clean exit cannot close a common-mode composition hole outside the oracle’s discrimination.

Required change: close findings 1–3 and require controls over every assembly boundary independently of BF2’s probe coverage.

5. INFO / CLEARED — CIRCULARITY

The logical circularity is genuinely broken. BV0A no longer needs emitter-owned mapping violations to be clean; it must deliver a faithfully composed artifact, while BV0 retains the full 36-cell oracle-clean exit ([BV0.md](docs/arch/refactor/rev11/charters/BV0.md:52), [AMD-008](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:413)).

Required change: none to the DAG or ownership principle. The new BV0A proof must first be made sound.

6. INFO / CLEARED — SCOPE

The widening is now explicitly declared. §4 supersedes the conflicting Objective/procedure/exits/abort text, AMD-007 §§1, 7, and 8.1, all four chunk/`CodeTransform` passages, and BV0A owned-scope item 4 ([AMD-008 §4](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:490)). The mandate remains local to the two script rewrites and does not confer whole-module chunk-IR or B4 authority.

Required change: none, provided the corrected rewrite algebra remains limited to these two operations.

7. HIGH — RIGHT-SIZING AND ACCEPTANCE CONTROLS

Design problem: the blanket mutation rule is internally impossible. Every mutation supposedly must emit a candidate artifact, leave the reference unchanged, and fail specifically at the equality assertion ([AMD-008](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:378)). But:

- Malformed input-map mutations must fail preflight before emitting an artifact.
- The code mutation must fail the byte-baseline assertion before map equality.
- The applicability mutation must fail applicability accounting.
- Input mutations necessarily change the reference’s input.

The per-field sweep also omits generated line, generated column, `source_index`, mapped-versus-sourceless variant, and several presence/order cases. A comparator that ignores `source_index` could therefore pass every required listed comparator mutation.

Why it matters: a correct implementation cannot satisfy the literal procedure, while an incomplete comparator can satisfy its enumerated evidence.

Required change: partition mutations by expected failure stage:

- Schema/preflight controls → named typed preflight error.
- Applicability controls → named manifest-accounting assertion.
- Code controls → pinned byte-baseline assertion.
- Artifact controls → named equality assertion with emitted candidate and unchanged reference.

Add exhaustive segment-field and presence/order controls, bind applicability to the exact manifest path and commit/tree digest, and state the literal expected split of 18 map-enabled and 18 map-disabled cells.

The design should keep exact ordered artifact comparison and the independent input-only reference. It should cut the impossible blanket mutation ceremony and add the missing transform-boundary algebra and vectors.

---

## Round 5 — adversarial / governance

VERDICT: BLOCK

Reviewed exact commit `2096ae221c3f8860299ac89ec67d31f9cec36149`, tree `289cb1c6c3e123bdb736dc5494d3beb8ee1dae0a`.

The ownership narrowing is defensible in principle. These sentences save the design intent:

> “A mechanically composable but oracle-INVALID fragment mapping … is … carried forward faithfully and is a mandatory BV0 bug.”

> “An exact-equality failure against the reference is BV0A’s OWN composition defect by definition.”

> “BV0 owns bringing every residual violation … to zero.”

But the current equality specification is incomplete enough that production and reference can share a defective interpretation and pass.

1. **BLOCKING — the chaining algebra omits a required post-overwrite transition and does not define survivor geometry completely.**

Quoted text:

> “Every occurrence whose generated position lies OUTSIDE every edited range survives…”

> “Every occurrence whose generated position lies inside a non-empty overwrite’s replaced range is DROPPED and replaced by exactly ONE occurrence at the replacement’s generated start.”

> “The following surviving original chunk supplies the transition from the removed range’s old end IF ONE EXISTS…”

The last rule is stated only for an empty overwrite. In the real implementation, a non-empty `Chunk::Overwritten` emits its replacement-start token and advances the cursor; the following `Chunk::Original` then emits another token at its generated start, mapped from the old range end. See [source_map.rs](crates/verter_compiler/src/code_transform/source_map.rs:181), [source_map.rs](crates/verter_compiler/src/code_transform/source_map.rs:243), and [source_map.rs](crates/verter_compiler/src/code_transform/source_map.rs:510).

Concrete exploit: both production and reference omit the rename-end transition when no input segment exists exactly at the old end. The replacement token’s provenance then leaks into following original bytes until a later segment. Exact equality passes because both implementations follow the same incomplete prose. The controls require a removal transition mutation, but no corresponding rename transition mutation.

The text also never defines:

- the transformed generated coordinate of every outside survivor;
- whether edit ranges are explicitly half-open;
- how a transition selects source/name/sourceless state at the old end;
- whether a transition is added, coalesced, or ordered around input occurrences already at that coordinate.

Required correction: specify a total per-pass algorithm for both empty and non-empty overwrites, including full UTF-16 coordinate transformation, transition creation at every resumed `Original` chunk, last-applicable lookup at the old end, name handling, and equal-coordinate collision order. Add independent rename-end transition and transition-payload mutations.

2. **BLOCKING — reference independence remains declarative and is impossible to audit against the incomplete normative assembly grammar.**

Quoted text:

> “The reference lives in the JavaScript conformance harness … and has NO dependency … on Rust composition…”

> “The reference is written from this amendment’s normative algorithm, NOT transcribed from the production implementation or its diff.”

> “The reference compares expected CODE to production code…”

Cross-language and no-import rules do not establish N-version independence. Production and reference can be generated from the same JSON/DSL, copy the same mistaken newline rule, or be manually translated from one common description without importing Rust.

More fundamentally, the amendment does not normatively specify the complete code-byte grammar needed to compute expected code. The real assembler relies on `render_ids`, `format_import_specifier`, Rust debug-string escaping, exact punctuation, and conditional newline behavior in [compile.rs](crates/verter_session/src/compile.rs:34). The amendment specifies write order, not those byte rules. The JavaScript reference must therefore copy or infer production behavior despite the “No translation” clause.

The baseline is also not pinned tightly enough:

> “‘Byte-identical to the pre-amendment output’ binds a named commit and tree plus independently captured per-cell output digests, recorded in the candidate’s evidence.”

That allows the candidate package to choose and record the “named” baseline later. The header’s prepared-against SHA is not explicitly declared to be the baseline, and the digest manifest is not part of the reviewed amendment identity.

Concrete exploit: backfill the JavaScript reference and baseline digests from a defective candidate, then demonstrate that one later production-only mutation is detected. That proves sensitivity around the defective common baseline, not correctness.

Required correction:

- State the stronger ruling verbatim: production and reference may share only immutable pre-assembly inputs and the V3 specification—no shared generator, tables, helper, fixture-derived expectation, or declarative code computing placement, rewrites, remapping, boundaries, ordering, or encoding.
- Specify the complete assembly byte grammar normatively, or separate code-byte verification from map placement using a pre-reviewed immutable write manifest.
- Pin the exact baseline commit/tree and an exact per-cell digest manifest in the reviewed amendment package.
- Freeze independently authored literal vectors before either implementation and cover every assembly branch, not merely one fixture.

3. **BLOCKING — the purported canonical map schema is not uniquely determined.**

Quoted text:

> “One canonical output schema, defined here and not in the reference.”

> “`file` is the assembled module’s own identity…”

> “Fragment source spellings are rebased into assembled-relative spellings at composition time…”

“Own identity” does not specify an exact byte string. The codebase exposes both the canonical main ID and a `._VERTER_.bundle.ts` identity in [id.rs](crates/verter_session/src/id.rs:183). Both production and reference can choose the same wrong one.

The `sourceRoot` rule is not an algorithm. It provides no source-map URL/base, URL/path normalization rules, or treatment of absolute URLs, Windows spellings, queries, or `null` source rows. Under the source-map specification, relative sources are resolved relative to the source-map URL after applying `sourceRoot`; `file` does not provide that base and is deliberately context-defined. [ECMA-426](https://tc39.es/ecma426/2024/)

Other unresolved schema points include:

- ordering and duplicate policy for `ignoreList`;
- invalid/out-of-range `ignoreList` indices;
- treatment of legacy `x_google_ignoreList`;
- exact valid row/container types;
- whether every unlisted top-level property is forbidden, rather than the subjective category “semantic extension.”

Concrete exploit: both implementations strip `sourceRoot` and retain the old relative spelling, or choose the `.bundle.ts` identity. Equality passes; BF2 may report the resulting wrong source identity, but its verdict is explicitly excluded, leaving an assembly-owned defect for a BV0 block that does not own it.

Required correction: define the exact `file` value, source-map base URL for each fragment and output, a total source rebasing algorithm, closed allowed JSON keys, and complete `ignoreList`/legacy-extension behavior.

4. **BLOCKING — the control set’s completeness claims are false.**

Quoted text:

> “Per-field sweep. One mutation per compared artifact field…”

The listed sweep has no direct mutation for:

- `Segment.source_index`;
- generated column;
- independently generated line versus placement-base corruption;
- mapped-versus-sourceless kind in an unaffected fragment region;
- separate original-line and original-column mutations—the wording permits one combined plant;
- ignore-list order, duplication, or bounds.

The missing `source_index` mutation is especially material: the architecture ruling explicitly required remapping one template segment through the script table base. That control has not been carried over. A comparator can ignore `source_index` while detecting every listed table-row and geometry mutation.

The fail-closed claim is also false:

> “One plant per item 4 category…”

Its list omits item 4’s malformed content-table row, invalid/truncated VLQ distinct from overflow, incompatible `sourceRoot` pair, non-parallel `sourcesContent`, and the broader “otherwise non-flat” case.

Applicability is improved, but still not exact: “the LOCKED BF2 seed manifest” is not bound by path/blob/digest, and the “expected per-class counts” are not stated as the known `18 map-enabled / 18 map-disabled`.

The mutation-application proof is strong as far as causality goes, but unlike AMD-007 it does not require persisted reversible recipes or independent confirmers to rerun every recipe without sampling.

Required correction: generate a machine-readable field/category matrix from the normative schema, require an independently isolated plant for every field and subfield, restore the missing table-base control, state the exact manifest identity and `18/18` counts, and restore AMD-007’s persisted-recipe plus independent-rerun requirement.

5. **MAJOR — `UncomposableInputMap` is neither total nor objectively classifiable.**

Quoted text:

> “`UncomposableInputMap` … is exactly…”

> “incompatible table metadata … means exactly a `sourceRoot` pair that cannot both be honoured under item 1(c)’s rebasing rule…”

The second predicate is circular because item 1(c) does not define a total rebasing rule. Missing or malformed `sources`, `names`, `sourcesContent`, `ignoreList`, `file`, or `sourceRoot` containers are also not exhaustively classified. Endpoint legality for “outside its own fragment” is not defined.

Concrete exploit: when source rebasing fails because the assembler chose the wrong base, classify the pair as inherently “incompatible table metadata” before equality is attempted and request rescope.

This does not directly create a false PASS—the document commendably says equality failures remain BV0A-owned—but it creates a governance escape from ownership and reintroduces the circularity through `RESCOPE_REQUIRED`.

Required correction: define a closed typed preflight function over the immutable raw DTO, with objective predicates for every top-level field and exact endpoint rules. Prohibit any category from depending on production/reference composition output. Add one control for every enum variant.

6. **BLOCKING — the supersession list still conflicts with text it claims remains untouched.**

The original owned-scope item 4 requires:

> “validated through BF2’s reaccepted authored-source oracle, with no harness copy or synthetic candidate map…”

The amendment says:

> “‘No harness copy’ … means no harness-synthesized BF2 candidate map and no duplicate PRODUCTION assembly route; it does not forbid this mandated test oracle.”

But §2 item 5 then says:

> “Everything else in both items is untouched…”

An input-only JavaScript implementation of the exact assembly grammar is plainly a harness-side assembly copy in the ordinary reading. The architecture ruling permits it as an oracle, but the amendment must explicitly supersede and redefine the original phrase. Claiming the phrase is untouched leaves two legitimate readings.

There is a second omission: §4 says the entire Abort/rescope section is replaced, while §2 item 4 describes replacing only the vague input-map trigger. The original mandatory stop conditions for needing B3/B4/BV1/B5, a universal IR, or a new public contract are not carried into the replacement text. Final no-authority exits remain, but the mandatory stop procedure has been silently removed.

Required correction: enumerate the `no harness copy` reinterpretation as an explicit supersession, and carry forward the first Abort/rescope paragraph verbatim or explicitly disposition its deletion.

The `CodeTransform` widening itself is honestly declared, and I found no weakening of BV0’s literal 36/36 oracle-clean exit.

7. **MAJOR — the ratification bundle restriction is improved but not fully byte-enforceable.**

Quoted text:

> “The ratification bundle may not carry unreviewed bytes.”

> “the bundle’s diff … must contain ONLY the review-history and §5.1 ratification records…”

Unlike AMD-006’s same-commit pattern and AMD-007’s named single-record delta, “review-history” is a semantic category rather than an exact path/blob allowlist. An arbitrary new or modified evidence file can be labelled review history while still carrying unreviewed material.

Required correction: require direct ancestry from the reviewed package and record an exact path allowlist, blob OIDs, and diff hash for the permitted §5.1/review-record changes.

Otherwise, §5 correctly binds full commit and tree identities, correctly separates amendment-text ratification from candidate acceptance, and correctly requires fresh three-mandate candidate review. §5.1 is properly pending, and its round-3 history reference is accurate.

8. **MAJOR — the document overstates what it proves.**

Quoted text:

> “BV0A’s implementation attempt proves a real, valuable, but NARROWER claim…”

The prior mechanism had demonstrated false-pass exploits, so “proves” is too strong. “Attempted to establish” or “produced evidence toward” would be accurate.

The following confidence claims are also contradicted by the defects above:

> “One canonical output schema, defined here…”

> “an exhaustive typed preflight taxonomy…”

> “One mutation per compared artifact field…”

Section 1 otherwise represents the failure of violation attribution fairly: it identifies the diagnostic-string dependency, ordering loss, and divergence from real `CodeTransform` behavior without disguising that this is a new design.

Required correction: remove the proof/exhaustiveness claims until the transition algebra, schema, taxonomy, controls, and reference provenance are actually total.

There is no authorized TODO, tracking row, waiver artifact, typed refusal, or “known defect” record substituting for an eventual fix. The amendment deliberately stages mechanically composable emitter bugs into BV0, but BV0’s mandatory zero-violation exit remains. That staging is not itself a waiver; the block is that the present specification can also let an assembly defect masquerade as shared reference behavior or “uncomposable” input.

Do not ratify commit `2096ae2`. The corrections require a new commit/tree and fresh three-mandate review.
