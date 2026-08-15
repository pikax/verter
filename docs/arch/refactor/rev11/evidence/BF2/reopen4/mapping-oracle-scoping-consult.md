# BF2 reopen #4 — mapping-oracle scoping consult

Codex xhigh, sandbox read-only, dispatched 2026-08-13 to investigate the blast radius of
a binding maintainer direction discovered while drafting AMD-007 (the interim BV0A
source-map block proposal): source-map correctness must be judged against the exact
authored SFC content, never against Vue/Svelte's own official compiler source map.

## Prompt

session id: 019ffd29-182c-7fe3-ba2a-6f5533298914
--------
user
You are investigating the blast radius of a binding maintainer direction for the Verter Revision 11 architecture-lock program, repo <repo-root>.

BINDING MAINTAINER DIRECTION (verbatim, not up for debate — investigate its implications, do not question it): 'we do not need to golden sourcemap from vue or sveltejs, we just need to guarantee that the sfc code that the user writes is mapped correctly, nothing to do with vue or svelte official compiler.'

Meaning: source-map correctness for Verter's generated output is a SELF-REFERENTIAL property against the user's own authored SFC source (does a mapped generated position trace back to the correct byte/line/column range in the ORIGINAL .vue/.svelte file content the user actually wrote) — NOT a byte/segment-identity comparison against Vue's or Svelte's own official compiler's source map output. Verter's assembled module structure legitimately differs from the official compilers' own internal assembly strategy, so matching their source-map shape is not a valid correctness oracle.

I just discovered packages/framework-conformance-harness/src/compare.mjs currently implements source-map comparison as candidate-vs-golden (official-compiler-produced) field-by-field/decoded-segment comparison (see compareMappings, mappingsFieldEqual, the 'mapping' axis in compare.mjs around lines 380-550) — this appears to violate the maintainer's direction. This harness's mapping axis is part of the BF2 block, which is currently ACCEPTED (its most recent acceptance: reopen #3, commit 4a0952ce450a5396c33a3f1c98598c65e6afd3f7, see docs/arch/refactor/rev11/evidence/BF2/BF2-reopen3-summary.md). I also just drafted (NOT yet ratified) an amendment AMD-007 at docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md whose required-exits text also frames mapping correctness in terms of 'the accepted BF2 candidate-versus-official comparator' — same wrong premise, needs rewriting before any review/ratification proceeds.

Read: packages/framework-conformance-harness/src/compare.mjs (the actual mapping axis implementation), docs/arch/refactor/rev11/charters/BF2.md (does its OWN charter text mandate official-comparison, or does it only say 'diagnostics, source-map, and TypeScript-observable product validation' generically — i.e. is this a charter-mandated design or an implementation choice BF2 made that exceeded/misread its charter?), docs/arch/refactor/rev11/charters/BV0.md (its required exits mention 'source-map differences after harness artifacts are removed' — same question), and docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md (the draft needing correction).

Answer:
1. Does BF2's OWN CHARTER (not just its implementation) mandate official-source-map comparison, or does the charter text actually leave room for a self-referential (candidate-vs-authored-source) oracle and BF2's implementation chose the wrong one unprompted?
2. Given the maintainer direction, is BF2's ALREADY-ACCEPTED mapping-axis implementation now a genuine defect requiring a 4th reopen, or can the self-referential oracle be added as new/additional evidence WITHOUT reopening BF2 (e.g. because BF2's charter never actually required matching Vue's official map, so nothing in BF2's charter is violated by leaving the existing official-comparison code in place as one signal alongside a new self-referential check, versus needing removal)?

## Decisive ruling

BF2’s charter is not defective; BF2’s accepted mapping implementation is. The candidate-versus-official source-map comparison must be removed from the acceptance path, BF2 must be formally reopened a fourth time, and AMD-007 review must pause until BF2 is reaccepted with an authored-source oracle.

### 1. What BF2’s charter actually requires

BF2’s charter does not mandate official-source-map equality.

- Its objective is generic conformance infrastructure: [BF2.md:6](docs/arch/refactor/rev11/charters/BF2.md:6).
- Its owned scope says only “diagnostics, source-map, and TypeScript-observable product validation”: [BF2.md:20](docs/arch/refactor/rev11/charters/BF2.md:20).
- Its exits require “diagnostic/mapping discrimination,” not golden-map equality: [BF2.md:28](docs/arch/refactor/rev11/charters/BF2.md:28).

Nothing there requires identical `sources`, `sourcesContent`, `names`, or decoded segments relative to an official compiler map. The charter leaves full room for candidate-generated positions to be validated against the authored SFC.

The official comparison was an implementation and review interpretation added later. The accepted evidence explicitly describes it as restored behavior in [BF2-reopen3-summary.md:26](docs/arch/refactor/rev11/evidence/BF2/BF2-reopen3-summary.md:26).

BV0’s wording is more ambiguous. “Source-map differences after harness artifacts are removed” at [BV0.md:20](docs/arch/refactor/rev11/charters/BV0.md:20) does not identify the official map as the oracle, but reopen #3 interpreted it that way. That ambiguity should now be removed.

### 2. BF2 requires reopen #4

This is a genuine BF2-owned defect, not merely an opportunity for additional successor evidence.

The current implementation:

- defines official and candidate map fields as pairwise contractual operands: [compare.mjs:379](packages/framework-conformance-harness/src/compare.mjs:379);
- compares decoded segment sets: [compare.mjs:397](packages/framework-conformance-harness/src/compare.mjs:397);
- compares every map field candidate-versus-golden: [compare.mjs:430](packages/framework-conformance-harness/src/compare.mjs:430);
- makes that comparison acceptance-bearing: [compare.mjs:526](packages/framework-conformance-harness/src/compare.mjs:526).

That implementation never receives the authored SFC content or checks whether a candidate generated position maps to the correct authored token. It can therefore:

- reject a correct Verter map merely because Verter assembled different generated code;
- accept a wrong candidate map if its fields resemble the official map while its positions do not match the candidate’s actual generated code.

Adding an authored-source oracle while retaining official-map equality as another gating signal would not cure the defect. An unsound extra gate is not “stronger evidence”; it changes the accepted product contract and can falsely reject correct output.

The official comparator may survive only as clearly non-gating forensic telemetry outside the mapping axis. Given the maintainer direction, deletion is cleaner. It must not affect verdicts, required exits, seed-cell status, or evidence.

Program action:

1. Mark BF2 reopened as reopen #4 and block dependent acceptance.
2. Replace the mapping oracle and re-run BF2’s full three-mandate review.
3. Preserve reopen #3’s option-propagation evidence as unaffected historical evidence, but supersede its source-map ruling.
4. Only after BF2 is reaccepted should AMD-007 be rewritten, rebased, assigned fresh identities, and reviewed.

### 3. Correct authored-source oracle

The oracle needs these independent inputs:

- the exact generated code;
- the candidate map;
- whether the map product was requested;
- canonical source identities and the exact original `.vue`/`.svelte` content;
- independently authored expected anchor/range contracts;
- independently identified synthetic generated ranges.

A raw “take the substring at each segment” test is not sufficient by itself. Source-map V3 mappings are point anchors, not source ranges. JavaScript/CSS columns are zero-based UTF-16 code-unit columns, and a one-field segment represents unmapped generated code. See [ECMA-426 Source Map specification](https://tc39.es/source-map/).

The implementable checks are:

1. **Map contract and bounds**

   Require a map when requested and absence when not requested. Strictly decode it; require V3, valid source/name indices, deterministic ordering, and generated/original coordinates within the exact code and source. Convert typed byte offsets to UTF-16 columns only at the encoding boundary.

2. **Source identity**

   Every source-bearing segment must resolve to a declared authored source unit. For this bounded SFC pack, that normally means the exact fixture identity. If `sourcesContent` is present, it must equal the actual authored bytes; it is never trusted as the oracle’s copy of the source.

   `sourceRoot` is used to resolve identity, not compared textually with the official map. `names` indices are validated locally; the names array need not resemble the official compiler’s.

3. **Truthfulness of every source-bearing segment**

   At each generated/source pair, identify the relevant generated and authored token or independently declared range.

   - For copied or moved authored text, require exact token/range equality.
   - For wrappers such as `_ctx.count`, map the authored `count` lexeme, not `_ctx.`.
   - For a legitimate semantic rewrite where generated and source spelling differ, require an explicit independently authored relation identifying the exact generated token and exact original span. An arbitrary in-bounds mapping is not sufficient.
   - Every source-bearing segment must be covered by an exact-text relation or an explicit semantic relation. Unclassified mapped segments fail.

   The existing repository helper already demonstrates the right basic technique—lookup a generated token and require matching original text at the mapped column—in [sourcemap_e2e_helpers.rs:115](crates/verter_compiler/src/framework_common/sourcemap_e2e_helpers.rs:115), with a shifted-column discriminator at [sourcemap_e2e_helpers.rs:274](crates/verter_compiler/src/framework_common/sourcemap_e2e_helpers.rs:274).

4. **Completeness and round trips**

   Segment truthfulness alone permits an empty or incomplete map. Independently authored controls must identify required script and template anchors. For each:

   - generated-to-source lookup returns the exact source identity and source start;
   - source-to-generated lookup includes the intended generated anchor;
   - the mapped source bytes equal the expected authored range.

   This implements the existing Revision 11 requirement for bidirectional round trips in [mapping-products.md:44](docs/arch/refactor/rev11/contracts/mapping-products.md:44).

5. **Assembly composition**

   Validate script and template fragment mappings against the same authored SFC first. In the final module, each surviving fragment anchor must have its generated position translated by exact assembly placement while retaining its original source tuple. Assembly rewrites may remove affected mappings, but cannot re-anchor fragment-generated coordinates directly as SFC coordinates.

6. **Synthetic code**

   Framework/assembly scaffolding must have no source-bearing four- or five-field segment. Do not map it to the nearest SFC byte.

   A one-field unmapped segment is correct and sometimes necessary at the start of a synthetic range to stop a preceding mapped segment from bleeding across that range. Therefore the exact rule is “no source-bearing mapping,” not literally “no encoded segment whatsoever.”

   Validate the generated intervals induced by source-map lookup semantics and require every synthetic interval to resolve to `null` original provenance.

7. **Required mutations**

   At minimum: absent map, malformed VLQ, wrong source identity, stale/wrong `sourcesContent`, shifted generated line/column, shifted original line/column, dropped script anchor, dropped template anchor, fabricated mapping over synthetic scaffolding, missing unmapped boundary causing mapping bleed, and non-ASCII/emoji plus CRLF controls.

### 4. Required document and implementation changes

#### (a) BF2 `compare.mjs`

Replace the acceptance semantics at [compare.mjs:379](packages/framework-conformance-harness/src/compare.mjs:379) through [compare.mjs:450](packages/framework-conformance-harness/src/compare.mjs:450):

- retire `mappingsFieldEqual(goldenMappings, candidateMappings)`;
- replace `compareMappings(goldenMap, candidateMap)` with an authored-source validator over candidate code/map, exact sources, anchors, and synthetic ranges;
- stop treating official `sourceRoot`, `sources`, `sourcesContent`, `names`, or decoded segment sets as expected candidate values;
- pass fixture identity/content and anchor policy through `compareArtifacts`;
- at [compare.mjs:526](packages/framework-conformance-harness/src/compare.mjs:526), invoke only candidate-to-authored-source validation.

`check-candidate.mjs` must load/pass the exact fixture content rather than only the golden map. The map-on/off decision comes from the requested cell, not golden-map presence.

The mapping halves of `diagnostic-mapping-discrimination.spec.mjs`, `assembled-sourcemap.spec.mjs`, and mapping mutations must be rewritten. The decoder remains useful; the official assembled-map composer is no longer a candidate correctness oracle. The README’s “full-field source-map comparison” description at [README.md:62](packages/framework-conformance-harness/README.md:62) also becomes stale.

#### (b) BF2 charter

No substantive charter amendment is required. Its current language already authorizes the correct oracle.

Add a clarifying sentence, either in the charter or reopen #4’s binding interpretation:

> Source-map validation compares Verter’s generated artifact with its exact authored SFC/source units; official compiler source-map fields, segment shape, density, and assembly geometry are not correctness oracles.

That prevents another implementation-level misreading without changing BF2 ownership.

#### (c) BV0 charter

Rewrite [BV0.md:20](docs/arch/refactor/rev11/charters/BV0.md:20) from “source-map differences” to, for example:

> source-map correctness defects against the exact authored Vue SFC after harness artifacts are removed, including incorrect source identity/coordinates, missing required authored anchors, and fabricated provenance over synthetic output.

Also qualify:

- [BV0.md:34](docs/arch/refactor/rev11/charters/BV0.md:34): official rc.3 reproduction applies to framework semantics; mapping defects reproduce through BF2’s authored-source oracle.
- [BV0.md:40](docs/arch/refactor/rev11/charters/BV0.md:40) and [BV0.md:46](docs/arch/refactor/rev11/charters/BV0.md:46): name the authored-source mapping oracle explicitly.

#### (d) AMD-007 draft

AMD-007 must not proceed to review in its current form. Rewrite:

- §1 lines 14–24, 31–35, 50–53: remove the claim that candidate-versus-official mapping is accepted/correct and acknowledge BF2 reopen #4.
- BV0A objective, lines 110–114: require candidate-to-authored-SFC validation.
- Lines 158–160: say BV0A consumes the reaccepted BF2 authored-source oracle; it does not preserve old comparison semantics.
- Procedure lines 165–168: absence prevents an authored-source verdict.
- Lines 177–182: replace “candidate self-consistency” with “decode/bounds-only validation without authored-source provenance.”
- Lines 184–192 and 200–208: mutations must be detected by the BF2 authored-source path, not the official decoded-segment comparator.
- Required exits lines 217–228: retain the first paragraph’s correctness/no-fabrication rules; replace candidate-versus-official wording with exact authored-source anchors, round trips, composition, and synthetic-range non-provenance.
- Abort lines 239–243: refer to a required change beyond the reaccepted BF2 authored-source oracle, not an official-oracle change.
- §4 lines 253–266: amend BV0 to consume the authored-source validator.
- §5 lines 298–302: replace “mapping parity” with “authored-source mapping correctness.”
- §6 lines 304–308: delete the false conclusion that this is “not a new oracle defect” and “BF2 is not reopened.”
- §7 lines 330–333: preserve official compiler oracles for the other conformance axes while explicitly excluding official maps from mapping correctness.
- The ratification quotation at lines 341–351 must name the reaccepted authored-source oracle and must not promise that candidate-versus-official mapping remains unchanged.

Because the draft is explicitly unratified at [AMD-007:3](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:3), correcting it now requires no superseding amendment—but its prepared commit/tree identities and review package must be regenerated.

### 5. AMD-007’s core proposal survives

The core proposal remains valid.

`assemble_vue_main_module` still needs to return one real typed code-plus-map result; the map still must be paired with the exact assembled code; existing script/template maps still need exact composition; and synthetic assembly bytes still must not receive fabricated provenance.

Therefore BV0A remains the correct owner of the current production assembly capability gap. BF2 cannot fill that gap because its charter prohibits supplying production behavior.

Only AMD-007’s acceptance-oracle framing changes:

- old: “match the official compiler’s assembled map”;
- correct: “prove that Verter’s assembled output maps accurately to the exact SFC the user authored.”

The missing-map problem and the definition-of-correctness problem are separate defects. BF2 reopen #4 fixes the oracle; BV0A fixes the production artifact.
