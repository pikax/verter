# AMD-008 round 7 — scoped-mechanism review

Reviewed commit `623c5e33265bc57ff93c1791ce52d09dd5ea57c7`, tree
`937806c87c92c166a46a921072e31cb1ec045714`. All three mandates `BLOCK`.

This round reviewed the amendment after two decisions: `file` is required
ABSENT in the assembled map, and the amendment ratifies the MECHANISM only,
with the complete frozen vector suite scoped to BV0A's implementation as a
separately reviewed acceptance deliverable.

**Both decisions were cleared.** Architecture verified the `file` basis
independently — fragment generation does set `file`, none of the 90 embedded
maps in the 144 golden records carries one, the oracle does not inspect it —
and accepted absence as a deliberate assembled-map policy. Circularity was
cleared again, and the `CodeTransform` widening was confirmed correctly bounded
to the two authorized script rewrites.

**The scoping also worked as intended.** Conformance and governance both now
explicitly classify the suite-completeness items — full DTO/schema coverage, a
discriminating CRLF vector, real synthetic-script bytes in F7, the missing
geometry cases, and per-variant fail-closed vectors — as BV0A ACCEPTANCE-GATE
work rather than independent ratification blockers.

**What still blocks is defects in the current text and seed, several of them
introduced by the immediately preceding revision:**

1. Contradictory freeze points — stale "frozen on ratification" text survives
   alongside the new "frozen at acceptance" rule (all three mandates).
2. Seed vector derivations that are wrong against real `CodeTransform`
   semantics (V2 per conformance, V6 per governance).
3. `UncomposableInputMap` declared "exactly" but not total over real
   decoder/preflight failures.
4. Mandatory controls that do not discriminate every field the ordered-artifact
   equality actually compares.
5. A self-contradictory source-spelling rule: sources are simultaneously a
   stable append of fragment rows and "rebased into assembled-relative
   spellings".
6. Encoded-output determinism dropped without replacement when raw `mappings`
   byte equality was removed.
7. Governance: the bundle-diff exception does not explicitly protect the new
   normative artifact, and cell applicability is not independently pinned.

Architecture additionally raised a soundness item: the acceptance suite requires
only "an assembly-scaffolding boundary" vector, while the real assembler has
roughly nine distinct write sites, so a sourceless transition omitted around an
unvectorized synthetic write could escape both the equality gate and the
oracle's `boundary: false` exemptions.

The verdicts are reproduced verbatim below.

---

## Round 7 — conformance

VERDICT: BLOCK

Reviewed commit `623c5e33265bc57ff93c1791ce52d09dd5ea57c7`; worktree was clean.

1. **BLOCKING — The amendment has contradictory freeze and normative-authority rules.**

   Amendment text:

   > “What this amendment ratifies is the MECHANISM, not a finished suite.”  
   > “The suite is FROZEN at BV0A’s acceptance.”  
   > “Ratifying this amendment ratifies and freezes that artifact alongside the text.”

   The first two rules place completion and freezing at BV0A acceptance ([AMD-008:210](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:210), [AMD-008:237](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:237)); §5.1 instead freezes the current seed when the amendment is ratified ([AMD-008:688](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:688)). Those are mutually exclusive change authorities.

   There is a second hierarchy contradiction:

   > “The prose that follows is explanatory…”  
   > “The normative algorithm consumes…”

   These occur only a few lines apart ([AMD-008:256](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:256), [AMD-008:264](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:264)).

   **Correction:** Replace §5.1’s freeze statement with: the current artifact is a factually reviewed seed; it may be completed during BV0A; the complete suite freezes only upon BV0A acceptance; post-acceptance changes require an amendment. Rename “The normative algorithm” to “The explanatory model.” Keep vectors as the sole normative algebra.

2. **BLOCKING — Seed vector V2’s derivation is factually wrong under real `CodeTransform` semantics.**

   V2 says:

   > “Pass 1’s own generated line 2 has its single segment at column 6…”  
   > “Pass 2’s lookup at `(2,0)` therefore has no same-line predecessor…”

   ([vectors:156](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:156), [vectors:167](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:167)).

   After the second rename overwrite, the following `Original` chunk begins at original offset 41 and contains `";\n"` followed by the tail. Real emission:

   - processes every `Original` chunk through `emit_mapped_content` ([source_map.rs:181](crates/verter_compiler/src/code_transform/source_map.rs:181));
   - emits a token at the chunk start ([source_map.rs:510](crates/verter_compiler/src/code_transform/source_map.rs:510));
   - emits an additional token at each interior line start after `\n` ([source_map.rs:525](crates/verter_compiler/src/code_transform/source_map.rs:525)).

   Consequently, pass 1 has a generated token at line 2, column 0. It is sourceless after lookup against the input map because `resolveAt` finds no segment at or before column 0 when the first input segment is column 6 ([mapping-oracle.mjs:1033](packages/framework-conformance-harness/src/mapping-oracle.mjs:1033)). Pass 2 therefore sees an actual same-coordinate sourceless occurrence, not “no same-line predecessor.”

   This also means V2 reaches the rewrite-boundary coincidence that `knownGaps` claims is absent ([vectors:21](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:21)), without normatively settling multiplicity/order at that collision.

   **Correction:** Either make V2 a true removal-only vector by starting with `_sfc_main`, or include the pass-1 line-2:0 occurrence and explicitly specify the ordering and survival behavior at the pass-2 boundary collision.

   V6 also contains a smaller factual error:

   > “stripping `\r` would … shift every subsequent column”

   ([vectors:354](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:354)). Columns reset to zero after `\n`; later-line columns do not inherit the prior line’s missing CR ([source_map.rs:580](crates/verter_compiler/src/code_transform/source_map.rs:580)). Delete that claim and describe the vector’s current non-discriminating status literally.

3. **BLOCKING — `UncomposableInputMap` is declared exhaustive but is not total over real decoder/preflight failures.**

   Amendment text:

   > “The classification is exhaustive…”  
   > “`UncomposableInputMap` means exactly one of…”

   ([AMD-008:510](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:510)).

   Missing categories include:

   - A VLQ segment with an invalid field count. The decoder separately rejects arities other than 1, 4, or 5 ([sourcemap.mjs:81](packages/framework-conformance-harness/src/sourcemap.mjs:81)); this is not an undecodable or overflowing VLQ quantity. F5 itself acknowledges that `"AC"` is “a different failure category” ([vectors:454](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:454)).
   - Invalid `ignoreList` shape/index.
   - Accumulator underflow.
   - Invalid `sourceRoot` type.
   - Malformed table containers, as distinct from malformed rows.
   - Incompatible metadata combinations.

   The seed’s own `knownGaps` lists several of these omissions ([vectors:17](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:17)), while the amendment’s fail-closed sweep does not cover them ([AMD-008:443](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:443)).

   The distinction between structurally uncomposable input and mechanically composable but oracle-invalid output is otherwise clear ([AMD-008:531](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:531)).

   **Correction:** Define the closed raw-input DTO and exhaustive preflight error enum as part of the acceptance-time normative suite, with one vector/control for every variant. Until then, remove “exactly” and “exhaustive” from the ratified text and explicitly state that the final acceptance suite closes the taxonomy.

4. **BLOCKING — The required controls do not discriminate every field used by ordered artifact equality.**

   The artifact includes generated coordinates, mapped-versus-sourceless kind, and `source_index` ([AMD-008:114](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:114)). But the mandatory per-field sweep names only table rows, original coordinates, `name_index`, and table metadata ([AMD-008:433](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:433)).

   There is no isolated mutation for:

   - generated line;
   - generated column;
   - `source_index`;
   - mapped versus sourceless segment kind;
   - `ignoreList` presence, order, or duplication.

   A concrete false-positive implementation can compare every listed field while ignoring `source_index`. V4 expects a template mapping to use appended source row 1 ([vectors:282](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:282)); because both fragments may contribute identical source spellings, pointing it at row 0 can pass every currently listed per-field control.

   Also, “FIVE geometry mutations” actually describes five categories containing eight separately planted mutations ([AMD-008:394](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:394)).

   **Correction:** Require one isolated mutation for every compared segment and metadata field, including kind and every index. Change the count to “five geometry categories/eight mutations.”

5. **BLOCKING — The source-spelling policy gives two incompatible normative instructions.**

   Amendment text first requires:

   > “fragment source spellings are rebased into assembled-map-relative spellings”  
   > “`sourceRoot` is ABSENT”

   ([AMD-008:193](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:193)).

   It later requires:

   > “source spellings … are carried OPAQUELY and unchanged”

   ([AMD-008:288](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:288)).

   Rebasing a spelling through `sourceRoot` necessarily changes the literal spelling. No current vector resolves this, and the schema is expressly said to be defined by the amendment rather than by the reference ([AMD-008:157](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:157)).

   **Correction:** State that original coordinates and names are opaque, while source spellings undergo one precisely specified `sourceRoot` rebase. Alternatively, preserve spellings and retain `sourceRoot`; the current combination cannot satisfy both statements.

Verified and not findings:

- Positional comparison is required. The decoder preserves wire order, including equal generated coordinates ([sourcemap.mjs:70](packages/framework-conformance-harness/src/sourcemap.mjs:70)); the oracle retains that order for equal-column segments ([mapping-oracle.mjs:1166](packages/framework-conformance-harness/src/mapping-oracle.mjs:1166)); `resolveAt` selects the last applicable occurrence ([mapping-oracle.mjs:1033](packages/framework-conformance-harness/src/mapping-oracle.mjs:1033)). Equal-coordinate order therefore changes resolution.
- Rewrite order and lengths are correct: global `__sfc__` → `_sfc_main`, then global removal ([compile.rs:74](crates/verter_session/src/compile.rs:74)); the ASCII lengths are 7 and 9 bytes. Nonempty overwrites emit one start token; empty overwrites emit none ([source_map.rs:243](crates/verter_compiler/src/code_transform/source_map.rs:243)).
- Origin/ordinal tagging plus non-deduplicated tables is sufficient for boundary and duplicate-row ownership once invalid coordinates are rejected.
- Map-enabled and map-disabled applicability is unambiguous, and BF2 runs without gating acceptance ([AMD-008:471](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:471)).
- The `file`-absent decision is supported: both fragment producers set `file` from `options.filename` ([compile/mod.rs:1002](crates/verter_compiler/src/compile/mod.rs:1002), [compile/mod.rs:1257](crates/verter_compiler/src/compile/mod.rs:1257)); the main node has distinct bundler and LSP identities ([id.rs:183](crates/verter_session/src/id.rs:183)); an independent scan found 144 golden records, 90 embedded maps, and zero `file` fields; BF2 does not inspect `map.file`.
- §4 identifies both AMD-007 and BV0A’s conflicting `CodeTransform`/chunk-IR language, including [AMD-007:126](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:126), [AMD-007:243](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:243), [BV0A.md:24](docs/arch/refactor/rev11/charters/BV0A.md:24), and [BV0A.md:148](docs/arch/refactor/rev11/charters/BV0A.md:148).

The declared incomplete-suite items—full DTO/schema coverage, a discriminating CRLF vector, real empty-synthetic-script behavior, missing geometry cases, and per-variant fail-closed vectors—are BV0A acceptance-gate work, not independent ratification blockers. The verdict is BLOCK because the current text additionally contains contradictory governance, incorrect seed derivations, a falsely exhaustive failure taxonomy, and non-discriminating mandatory controls.

---

## Round 7 — architecture

# VERDICT: BLOCK

The redesign is materially better than violation matching, and it breaks the original BV0A/BV0 circularity. However, the current amendment still has four ratification-blocking defects: an observable encoded-map gap, a factually wrong normative seed derivation, contradictory freeze semantics, and an independence/boundary mechanism that can admit common-mode composition defects.

## Ratification-blocking findings

1. **Critical — Decoded equality does not preserve deterministic encoded output**

**Design problem:** Section 2 explicitly excludes raw `mappings` byte equality. A producer can alternate between semantically equivalent encodings—for example, the canonical zero VLQ `A` and the non-minimal but still decodable zero VLQ `gA`—while passing wire validation and exact decoded-artifact equality. ECMA-426’s VLQ grammar does not require the shortest representation. [ECMA-426](https://tc39.es/ecma426/)

**Why it matters:** Raw map bytes are observable in Verter. `RuntimeOutputDescriptor::generated` hashes the serialized map into `map_hash`, so this defect can cause nondeterministic artifact identity and cache behavior even though decoded coordinates compare equal: [carrier_compiler.rs:208](crates/verter_compiler/src/framework_common/carrier_compiler.rs:208). AMD-008 replaces BV0A’s prior deterministic-output exit without preserving an equivalent encoded determinism requirement.

This is a concrete assembly defect that the proposed criterion lets through. It is distinct from legitimate reference/production encoding differences: production need not byte-match the JS reference, but production must encode deterministically.

**Required change:** Add an independent production invariant requiring identical serialized assembled map bytes across repeated identical invocations, or mandate a canonical encoder. Keep decoded equality against the reference; do not require production bytes to equal reference bytes. The raw validator should also state its policy for duplicate JSON members and unrecognized semantic fields.

2. **Critical — Normative seed vector V6 contains a false derivation**

**Design problem:** V6 states that stripping `\r` from CRLF would “shift every subsequent column”: [assembled-map-composition.vectors.json:354](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:354). That is false.

The actual `CodeTransform` coordinate logic resets the generated column on `\n`. A preceding `\r` can affect positions on that same physical line, including an end-of-line coordinate, but cannot shift columns on subsequent lines: [source_map.rs:574](crates/verter_compiler/src/code_transform/source_map.rs:574).

The current V6 output is not itself arithmetically wrong; it simply contains no CR-sensitive coordinate and therefore cannot demonstrate the claim made by its derivation.

**Why it matters:** The amendment gives vectors precedence over prose. Even a seed vector therefore cannot carry a false account of the production semantics. Declaring V6 “non-discriminating” under `knownGaps` does not cure its affirmative factual error.

**Required change:** Correct the derivation before ratification. The BV0A acceptance suite must additionally include a genuinely CR-sensitive coordinate—such as a segment after `\r` but before the corresponding `\n`—if preservation of that geometry is load-bearing.

I independently recomputed V1–V5 and V7, including rename/removal offsets, V4’s two-line template placement, V5’s UTF-16/non-BMP columns, ordered equal-coordinate segments, table remapping, and sourceless lookup barriers. I found no other wrong expected output among those seed vectors. F4’s VLQ overflow and F5/F6’s rejection outcomes are also consistent with the current decoder contract. F7 remains a declared non-real synthetic placeholder rather than evidence about production behavior.

3. **Critical — The amendment contradicts itself about when the vector artifact becomes normative and frozen**

**Design problem:** The operative mechanism says the current file is an incomplete seed and the complete suite is delivered, reviewed, and frozen only at BV0A acceptance: [AMD-008:210](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:210) and [AMD-008:237](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:237).

The review-history tail instead says ratifying AMD-008 “ratifies and freezes that artifact alongside the text” and that the amendment cannot be ratified until the package containing both is review-clean: [AMD-008:688](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:688).

Both cannot be true.

**Why it matters:** This is not editorial trivia. It changes whether later additions are implementation work, amendments to ratified normative text, or forbidden changes to an already frozen artifact.

**Required change:** Remove or rewrite the stale review-history language so one rule is stated consistently everywhere. Explicitly distinguish:

- the ratified vector mechanism;
- the non-normative/incomplete seed at AMD ratification;
- the authority required to approve the final schema and semantics;
- the commit or digest frozen at BV0A acceptance.

4. **High — Independence is code-level, not specification-level, and the acceptance process permits common-mode defects**

**Design problem:** Section 2 item 2(a)’s cross-language and input-only restrictions are useful and auditable. Its anti-translation rule is only a process assertion: reviewers cannot establish from the artifacts that the JS author did not port the same incorrect reasoning as the Rust implementation.

More importantly, the reference, schema, complete vector suite, and production implementation can all be completed and accepted together. The vector-precedence rule then allows the future suite to decide previously unsettled semantics without a separate architecture decision. This is exposed by the supposedly exact `UncomposableInputMap` taxonomy at [AMD-008:516](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:516), while the seed itself acknowledges missing `ignoreList`, `sourceRoot`, schema, and other rejection rules.

**Why it matters:** A Rust implementation and JS reference can share the same wrong boundary or chaining rule in an unvectorized case. Exact equality then proves only agreement. If the eventual vector silently codifies that agreement, the vector-precedence clause turns the common error into the contract.

**Required change:** Separate specification authority from implementation acceptance. At minimum:

- The complete DTO schema, validation order, rejection taxonomy, collision ordering, and table/chaining policies must receive an architecture review and immutable identity before they gate the Rust implementation.
- Subsequent coverage-only vector additions may remain BV0A deliverables, but changes that settle semantics must require an amendment.
- Freeze independently reviewed complete expected artifacts or digests for the finite 36-cell domain, rather than treating a co-developed executable reference as the sole expected-value authority.
- Pin the final reference source and vector artifact by commit/blob digest.

N-version implementation is valuable as a secondary check. It is not a sufficient normative oracle when both versions and their specification are accepted in the same change window.

5. **High — The boundary proof is not closed, leaving a BV0A defect that BV0 may never detect**

**Design problem:** Exact artifact equality catches a unilateral boundary omission only when the reference or a frozen vector contains the correct boundary. The required complete suite currently calls for only an assembly-scaffolding boundary vector, rather than coverage of every byte-producing assembler branch.

A concrete common-mode counterexample is that both implementations omit a sourceless transition around an unvectorized synthetic write—for example, a non-newline-terminated fragment followed by an attachment, HMR/SSR declaration, or final export. Equality passes. The oracle can also pass because it deliberately exempts some generated declarations from boundary attribution through `boundary: false`: [mapping-oracle.mjs:799](packages/framework-conformance-harness/src/mapping-oracle.mjs:799) and [mapping-oracle.mjs:994](packages/framework-conformance-harness/src/mapping-oracle.mjs:994).

The real assembler has numerous distinct write sites—imports, fallback code, separator, template, custom blocks, `__file`, HMR, SSR, and final export—not one generic scaffolding case: [compile.rs:34](crates/verter_session/src/compile.rs:34).

**Why it matters:** This is precisely a composition defect that can escape both BV0A equality and BV0’s later oracle-clean exit. BV0 cannot close an intentional oracle blind spot.

**Required change:** Make the BV0A acceptance artifact include an exhaustive assembler write/boundary manifest. Every write class and every transition between mapped fragment content and synthetic content must have an independently derived expected lookup state, including non-newline-terminated fragments and coincident transitions.

## Conclusions on the requested questions

1. **Soundness:** Not fully cleared. Ordered decoded equality catches masking, lost or duplicated segments, equal-coordinate reordering, table corruption, and incorrect boundaries when the reference is correct. It does not catch nondeterministic encoded output or common-mode mistakes outside independently fixed expectations.

2. **Too strong:** Exact ordered decoded equality is not inherently too strong. Segment order is observable under the accepted decoder’s last-applicable lookup. Exact tables and redundant barriers are stronger than generic source-map semantic equivalence, but that is legitimate if the amendment deliberately defines a canonical Verter artifact. The unresolved schema currently makes that canonicalization brittle, so those choices must be fixed before implementation comparison becomes binding.

3. **Independence:** Partially sound but insufficient. Cross-language and input-only DTO requirements are good; anti-translation is not auditable, and co-development leaves shared-defect risk.

4. **Boundary:** The textual BV0A/BV0 ownership split is preserved, but the proof boundary is not closed. The common-mode synthetic-boundary example can escape both gates.

5. **Circularity:** Cleared. BV0A no longer requires emitters to be oracle-clean, while BV0 retains responsibility for the eventual 36-cell clean oracle result.

6. **Scope:** The `CodeTransform` widening itself is correctly limited to the two authorized script rewrites and does not authorize whole-module chunk IR. The supersession hygiene is not cleared because the stale freeze text remains and encoded determinism was removed without replacement.

7. **Right-sizing:** The central mechanism—decoded artifact comparison plus vectors—is appropriate for an interim block. The design is under-specified where it matters: raw determinism, independent normative authority, and exhaustive assembler boundaries. Some explanatory chaining prose should be cut once vectors govern; retaining both invites conflicting readings.

The `file`-absent decision is cleared. I verified that script/template fragment generation does set `file`, while none of the 90 embedded maps in the 144 committed golden records contains it; the BF2 oracle does not inspect it, and the main node has distinct bundler and LSP identities. The amendment accurately presents absence as a deliberate assembled-map policy rather than fragment preservation.

After the blockers above are resolved, the remaining declared seed gaps—complete artifacts, immutable DTO schema, discriminating geometry cases, real synthetic-script F7, per-variant fail-closed vectors, and derivations for every rejection vector—are appropriate BV0A acceptance-gate work rather than independent reasons to block amendment ratification.

---

## Round 7 — adversarial / governance

VERDICT: BLOCK

Reviewed at exact commit `623c5e33265bc57ff93c1791ce52d09dd5ea57c7`, tree `937806c87c92c166a46a921072e31cb1ec045714`, with a clean worktree.

The thesis is defensible: assembly correctness can be separated from pre-existing emitter correctness if BV0A proves exact composition against a genuinely independent reference and BV0 later retains the literal 36/36 oracle-clean exit. The text that saves the narrowing is:

> “the complete artifact … equals the independently implemented, input-only reference”

and:

> “Residual fragment-emitter violations are BV0's acceptance responsibility, not BV0A's.”

See [AMD-008 §2](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:479) and [§3](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:563).

The current package nevertheless remains unratifiable.

## Ratification-blocking findings

1. **CRITICAL — Two normative vectors contradict real `CodeTransform` emission, and V6 contains a false derivation.**

   Quoted rule:

   > “every Original chunk unconditionally pushes a token at its OWN start”

   [Vector derivation basis](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:47)

   The real implementation does exactly that: `emit_mapped_content` unconditionally pushes a chunk-start token and emits interior line-start tokens. See [source_map.rs](crates/verter_compiler/src/code_transform/source_map.rs:181) and [source_map.rs](crates/verter_compiler/src/code_transform/source_map.rs:505).

   Concrete defects:

   - V4’s leading `Original[0,6)` emits a token at generated `(0,0)`. Its input map has no applicable same-line segment before column 6, so after chaining this token is sourceless. V4’s exact expected sequence begins at `(0,6)` and omits it. See [V4 expected output](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:281).
   - V6’s leading original chunk crosses the CRLF and emits an interior token at generated `(1,0)`. The input’s first line-1 segment is at column 6, so the chained token is sourceless. V6 omits it. See [V6 expected output](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:366).
   - V6 says:

     > “Stripping the CR would under-count every line by one and shift every subsequent column.”

     [V6 derivation](packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:354)

     This is false. Columns reset after LF. Removing a CR affects later coordinates on that same line, not every subsequent line.

   Exploit: because AMD-008 says “the VECTOR governs,” a production assembler and JS reference that both omit these required sourceless transitions pass while implementing the wrong semantics. A wrong seed remains a ratification defect even though the suite is incomplete.

   Required correction: add sourceless `(0,0)` to V4 and sourceless `(1,0)` to V6 in the correct ordered positions; correct the CRLF explanation and make V6 genuinely CR-sensitive. Recompute all expected sequences from the actual chunk stream.

2. **CRITICAL — The amendment has mutually exclusive freeze points.**

   §2 says:

   > “It does NOT certify the seed artifact as complete.”

   > “The suite is FROZEN at BV0A's acceptance.”

   [AMD-008 §2(1d)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:210)

   §5.1 instead says:

   > “Ratifying this amendment ratifies and freezes that artifact alongside the text”

   [AMD-008 §5.1](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:688)

   Exploit/governance failure: a maintainer can either freeze the admitted incomplete seed at ratification or ignore §5.1 and permit changes to an allegedly frozen normative artifact. There is no single valid ratification action.

   Required correction: under the stated final-round design, §5.1 must say that ratification binds and reviews the seed’s current bytes for factual correctness but does **not** freeze or certify completeness; the completed exact artifact is bound and frozen only by the later BV0A acceptance decision.

3. **HIGH — The fail-closed boundary contains a subjective rescope escape and contradicts the source-table rule.**

   The amendment claims:

   > “`UncomposableInputMap` … is exactly … a `sourceRoot` pair that cannot both be honoured under item 1(c)'s rebasing rule”

   [AMD-008 §2(4)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:516)

   Yet §1(c) says source spellings are rebased, while §1(e) says:

   > “source spellings … are carried OPAQUELY and unchanged”

   [AMD-008 §2(1c)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:193), [§2(1e)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:288)

   Exploit: a defective rebaser can label an inconvenient input “cannot both be honoured” and rescope it before equality runs. Neither “honoured” nor the canonical base/path algorithm is defined.

   Required correction: make the eventual frozen schema a total, ordered classifier for every raw input form; define source-root resolution and rebasing literally; state that no candidate may rescope an input unless it matches one frozen category. Delete or narrow “source spellings … unchanged.”

4. **HIGH — The universal mutation-proof rule is impossible for several mandated controls, while material fields have no discriminating mutation.**

   The text requires every mutation to prove:

   > “the candidate artifact was actually emitted carrying the intended changed property; the independent reference was UNCHANGED; the NAMED equality assertion produced the RED, rather than an earlier compile, setup, or harness failure”

   [AMD-008 §2(2e)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:455)

   This cannot hold for:

   - malformed/fail-closed inputs, which must fail before an artifact is emitted;
   - applicability mutations, whose purpose is to fail partitioning;
   - baseline mutations, which must fail code comparison before map equality;
   - input mutations, which necessarily change the reference’s input.

   The field sweep also has no direct mutation for `version`, a segment’s generated position, `source_index`, mapped-versus-sourceless kind, or ignore-list presence/order/duplicates. A comparator that ignores `source_index` can satisfy the listed controls. The heading also says “FIVE geometry mutations,” but the list mandates eight separate plants across five categories.

   Required correction: define separate proof protocols for equality, validation, applicability, and baseline failures. Add one valid, in-range mutation for every `MapArtifact` and segment field/presence state, and count plants accurately.

5. **HIGH — Reference independence is one-way, and the byte baseline is selected too late.**

   The reference must have:

   > “NO dependency … on Rust composition”

   and must be:

   > “NOT transcribed from the production implementation”

   [AMD-008 §2(2a)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:311)

   That does not prohibit production Rust from being generated or transcribed from the JS reference, both implementations sharing an unreviewed generator, or production branching over the 36 reference fixtures.

   Separately:

   > “a named commit and tree plus independently captured per-cell output digests, recorded in the candidate's evidence”

   [AMD-008 §2(2b)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:348)

   lets the candidate select what counts as “pre-amendment.”

   Required correction: prohibit dependencies and transcription in both directions, shared generators, shared expected-output tables, and fixture-specific production branches. Bind the baseline source commit/tree and digest-manifest identity before implementation rather than allowing the candidate to nominate them afterward.

6. **HIGH — Cell applicability is not independently pinned and permits a tautological control.**

   Quoted text:

   > “partitioned from the LOCKED BF2 seed manifest's own `sourceMap` request input”

   > “the expected per-class counts asserted”

   [AMD-008 Required exits](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:471)

   No path, blob identity, literal 18/18 counts, or immutable cell list is bound here. A test can derive both its classification and its “expected” counts from the same changed or stale manifest and pass tautologically.

   Required correction: bind the manifest path and blob identity from the ratified package, enumerate or hash the 36 cell identities, and state the literal expected counts—currently 18 map-enabled and 18 map-disabled.

7. **HIGH — The supersession enumeration still leaves live conflicts and silently drops determinism.**

   AMD-008 says:

   > “Everything else in both items is untouched”

   [AMD-008 §2(5)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:547)

   But BV0A owned-scope item 4 still says:

   > “with no harness copy”

   [BV0A charter](docs/arch/refactor/rev11/charters/BV0A.md:54)

   AMD-008 reinterprets only “No harness copy in the Required Exits,” although the conflicting phrase also exists in owned scope. The mandatory original abort for requiring B3/B4/BV1/B5, a universal IR, a new public contract, or an oracle change is also removed when Abort/rescope is replaced in full; the later no-authority exit is not the same early-stop governance rule.

   Finally, AMD-007 required:

   > “The source map is non-empty, decodable, deterministic”

   [AMD-007](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:303)

   AMD-008 allows different raw `mappings` encodings and supplies no repeated-run wire-determinism proof.

   Required correction: explicitly supersede and restate owned-scope item 4’s “no harness copy” phrase; restore the original mandatory abort paragraph; retain deterministic emitted-map bytes as a separate exit while allowing logically equivalent encodings only between production and reference.

8. **HIGH — The bundle-diff exception does not explicitly protect the new normative artifact.**

   The blockquote itself correctly binds reviewed-package and bundle commit/tree identities and correctly separates text ratification from candidate acceptance. The exception, however, permits a diff containing:

   > “ONLY the review-history and §5.1 ratification records, leaving this amendment's text and its charter deltas byte-identical.”

   [AMD-008 §5](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:642)

   It does not explicitly require the vector artifact’s blob to remain identical, and “review-history” is not an exact path allowlist.

   Required correction: record the reviewed vector blob OID and use an exact allowed-path list for the bundle-only diff. Require every other blob—including the vector—to be identical.

9. **MEDIUM — §1 overstates evidence, although its description of the prior attribution failure is otherwise fair.**

   It says:

   > “BV0A's implementation attempt proves a real, valuable … claim”

   [AMD-008 §1](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:27)

   The same section admits that pooled matching could consume an assembly-introduced violation. That attempt therefore did not “prove” assembly innocence. Likewise, two wrong vectors are not:

   > “the strongest available evidence that this class of precision belongs in vectors rather than prose.”

   [AMD-008 §2(1d)](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:243)
