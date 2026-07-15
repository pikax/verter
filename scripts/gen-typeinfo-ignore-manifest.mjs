#!/usr/bin/env node
/**
 * Regenerate the two-table typeinfo manifest ledger (§10).
 *
 * Emits three checked-in, generated-not-hand-maintained files:
 *
 * 1. `crates/verter_session/tests/cases/manifest_data/typeinfo_ignored_test_manifest_rows.rs`
 *    — the 362 `IgnoredTestRow`s, each with the full 13-column schema.
 * 2. `crates/verter_session/tests/cases/manifest_data/typeinfo_additional_proof_rows.rs`
 *    — the CLOSED set of 7 coverage-only `AdditionalProofRow`s.
 * 3. `crates/verter_session/tests/cases/manifest_data/typeinfo_parity_blocks.rs`
 *    — the `TYPEINFO_PARITY_BLOCKS` DAG (every block + prereqs +
 *    dominant mechanism + consumed mechanisms).
 *
 * Each `IgnoredTestRow`'s `block_id` is COMPUTED here from the
 * authoritative §10.4.1 row→block partition in
 * `docs/arch/native-typeinfo-parity.md` joined with the live
 * `#[ignore = "..."]` discovery + the Capability Map — NOT hand-typed
 * 362 times. The `AdditionalProofRow` table (file 2) and the
 * `TYPEINFO_PARITY_BLOCKS` DAG (file 3, with each block's
 * `required_guards`/`verification_labels`/prereqs/mechanisms) are
 * authored in this generator's own data maps (`buildAdditionalRows`,
 * `emitBlockRows`, `BLOCK_TO_REQUIRED_GUARDS`, `BLOCK_VERIFICATION_LABELS`,
 * the prereq/mechanism maps), NOT derived from §10.4.1. The Rust
 * guard tests only diff/fail; they never write the generated source (repo
 * rule: generators are scripts, not tests).
 *
 * Run after adding / removing / renaming an ignored test, or after the
 * §10.4.1 partition changes:
 *
 *     node scripts/gen-typeinfo-ignore-manifest.mjs
 *     # or via pnpm:
 *     pnpm gen:typeinfo-manifest
 *
 * Commit the regenerated rows alongside the source changes that prompted
 * the regeneration.
 *
 * Pass `--check` (or `--verify`) to regenerate in memory and EXIT NON-ZERO
 * if any committed file diverges, WITHOUT writing — the drift gate (the
 * Rust guard tests only diff/fail, never write tracked source):
 *
 *     node scripts/gen-typeinfo-ignore-manifest.mjs --check
 *     # or via pnpm:
 *     pnpm gen:typeinfo-manifest:check
 *
 * Byte-identity note (cross-platform): this generator ALWAYS writes `\n`
 * newlines, and on READ it normalizes CRLF/CR → LF for every input file
 * (`fs.readFileSync` does NOT translate newlines the way Python's
 * universal-newline `Path.read_text()` does). In `--check` mode the
 * committed file is likewise CRLF-normalized before the byte-compare, so a
 * CRLF working-tree checkout on Windows still passes while real content
 * drift is still caught — mirroring what makes the Rust freshness guard
 * pass cross-platform.
 */

import { mkdirSync, readdirSync, readFileSync, writeFileSync, existsSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

/** Normalize CRLF/CR → LF, mirroring Python universal-newline read. */
function normalizeNewlines(text) {
  return text.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
}

/** Read a UTF-8 file with newline normalization (mirrors `Path.read_text()`). */
function readTextNormalized(path) {
  return normalizeNewlines(readFileSync(path, "utf8"));
}

// ── Python-faithful Unicode primitives ──
//
// These mirror the CPython `re`/`str` semantics the original generator relied
// on (the port previously used JS-native operators/regex that are ASCII /
// UTF-16-bound). They are exact no-ops on the current all-ASCII inputs; the
// fidelity matters only if a future test/doc author introduces non-ASCII
// identifiers, doc text, or filenames. The regexes are built from ASCII escape
// STRINGS so no raw line-terminator bytes (LS U+2028 / PS U+2029) ever enter
// this source file (those bytes are JS source line terminators).

/**
 * Single-character line boundaries of Python `str.splitlines()`:
 *   \n \v \f \r FS(0x1C) GS(0x1D) RS(0x1E) NEL(0x85) LS(0x2028) PS(0x2029),
 * plus the `\r\n` digraph as ONE break (alternated first so it is not split
 * into two). NOTE: US (0x1F) is deliberately ABSENT — `splitlines()` does not
 * break on it (it differs there from `\s`, which does include 0x1F).
 */
const SPLITLINES_RE = new RegExp("\\r\\n|[\\n\\r\\v\\f\\x1c\\x1d\\x1e\\x85\\u2028\\u2029]", "g");

/**
 * Faithful Python `str.splitlines()` (keepends=False). Unlike `String.split`,
 * this does NOT emit a trailing empty element for a trailing boundary, and
 * `splitLines("")` is `[]` (String.split would give `[""]`). Verified
 * byte-identical to CPython `str.splitlines()` across the boundary set and the
 * trailing/empty/`\r\n`-digraph edge cases.
 */
function splitLines(text) {
  const out = [];
  let last = 0;
  SPLITLINES_RE.lastIndex = 0;
  let m;
  while ((m = SPLITLINES_RE.exec(text)) !== null) {
    out.push(text.slice(last, m.index));
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

/**
 * Python `re` `\w` (the py3 `re.UNICODE` default), as a `u`-flagged class.
 * Python `\w` == `str.isalnum() || '_'` == categories L* ∪ N* plus the
 * underscore `_` (U+005F). The faithful structural class is therefore
 * `[\p{L}\p{N}_]` with a LITERAL underscore — NOT `[\p{L}\p{N}\p{Pc}]`: the
 * connector-punctuation class `\p{Pc}` OVERMATCHES Python `\w`. CPython rejects
 * the non-`_` Pc connectors (U+203F ‿, U+2040 ⁀, U+2054 ⁔, U+FE33/FE34, U+FE4D–
 * FE4F, U+FF3F) as `\w`, whereas `\p{Pc}` accepts them; only `_` itself is a Pc
 * char Python treats as word. This class also does NOT include `\p{M}`
 * (combining marks): CPython `\w` rejects e.g. U+0301 (a Mn mark), which `\p{M}`
 * would wrongly accept. Residual delta: characters whose Unicode general
 * category differs between Node's ICU and the CPython build's frozen `\w` tables
 * (a Unicode-VERSION skew, not a class-definition mismatch — no static `\p{}`
 * class can track one interpreter build's frozen snapshot, and matching it would
 * diverge from a different Python). The `\p{L}`/`\p{N}` cores plus a literal `_`
 * are the faithful definition modulo that irreducible version skew.
 */
const PY_WORD_SRC = "[\\p{L}\\p{N}_]";

/**
 * Python `re` `\s` (`re.UNICODE` default), as a `u`-flagged class. Python `\s`
 * == `[\t\n\v\f\r ]` ∪ Unicode whitespace ∪ FS/GS/RS/US (0x1C–0x1F). Unicode's
 * `White_Space` property OMITS 0x1C–0x1F, so they are added explicitly. Verified
 * EXACT parity (zero divergence) against CPython `\s` over the full code space.
 */
const PY_SPACE_SRC = "[\\t\\n\\v\\f\\r \\x1c\\x1d\\x1e\\x1f\\p{White_Space}]";

/**
 * Python `str.strip()` / `lstrip()` faithful to CPython, built on the Python
 * `\s` class (`PY_SPACE_SRC`) with the `u` flag so `\p{White_Space}` is a real
 * Unicode property and not a literal char class. This differs from JS
 * `String.prototype.trim()` in two ways that matter for a faithful port:
 *   - `.trim()` does NOT strip 0x1C–0x1F (FS/GS/RS/US) or 0x85 (NEL); Python
 *     `strip()` DOES (they are in `PY_SPACE_SRC`).
 *   - `.trim()` DOES strip U+FEFF (BOM/ZWNBSP); Python `strip()` does NOT
 *     (U+FEFF is not in `White_Space`), so a leading/trailing BOM is kept.
 * The patterns must carry `u`; without it `\p{White_Space}` degrades to the
 * literal set `{ p, W, h, i, t, e, _, S, a, c, ... }` and corrupts the strip.
 */
const PY_STRIP_LEAD = new RegExp("^(?:" + PY_SPACE_SRC + ")+", "u");
const PY_STRIP_TRAIL = new RegExp("(?:" + PY_SPACE_SRC + ")+$", "u");
function pyLstrip(s) {
  return s.replace(PY_STRIP_LEAD, "");
}
function pyStrip(s) {
  return s.replace(PY_STRIP_LEAD, "").replace(PY_STRIP_TRAIL, "");
}

/**
 * Python-faithful code-POINT lexicographic string comparison (NOT UTF-16
 * code-unit). JS `<` and the default `Array.sort()` compare by UTF-16 code
 * unit, which diverges from Python for non-BMP (astral) code points: an astral
 * char's high surrogate (0xD800–0xDBFF) sorts BELOW BMP chars like U+E000 that
 * Python (code-point) sorts ABOVE it. Iterating with the string iterator yields
 * whole code points, replicating Python `str` ordering.
 */
function codePointCompare(a, b) {
  const ai = a[Symbol.iterator]();
  const bi = b[Symbol.iterator]();
  for (;;) {
    const x = ai.next();
    const y = bi.next();
    if (x.done && y.done) return 0;
    if (x.done) return -1;
    if (y.done) return 1;
    const cx = x.value.codePointAt(0);
    const cy = y.value.codePointAt(0);
    if (cx !== cy) return cx < cy ? -1 : 1;
  }
}

// ── Per-file -> substrate mapping (carried forward; the `substrate`
//    column is preserved). ──
const FILE_TO_SUBSTRATE = new Map([
  ["apparent_types.rs", "ApparentTypes"],
  ["basic.rs", "MacroResolution"],
  ["branded_types.rs", "ApparentTypes"],
  ["cache_invalidation.rs", "CacheInvalidation"],
  ["call_resolution.rs", "CallResolution"],
  ["class_features.rs", "ClassFeatures"],
  ["conditional_infer.rs", "ConditionalInfer"],
  ["const_type_param.rs", "TypeParameterFeatures"],
  ["contextual_typing.rs", "ContextualTyping"],
  ["cross_file.rs", "CrossFileResolution"],
  ["decorators.rs", "ClassFeatures"],
  ["deep_path.rs", "PathProjection"],
  ["demand_boundary.rs", "DemandBoundary"],
  ["enums.rs", "EnumResolution"],
  ["expansion_boundaries.rs", "ExpansionBoundaries"],
  ["flow_invalidations.rs", "FlowNarrowing"],
  ["flow_return_catalog.rs", "FlowNarrowing"],
  ["footprint.rs", "AuditFootprint"],
  ["function_advanced.rs", "CallResolution"],
  ["index_signatures.rs", "IndexSignatures"],
  ["indexed_utilities.rs", "UtilityComposition"],
  ["jsx.rs", "JsxResolution"],
  ["mapped_modifiers.rs", "MappedTypes"],
  ["mapped_template.rs", "MappedTypes"],
  ["menu_like.rs", "CompositeSurfaces"],
  ["message_list_like.rs", "CompositeSurfaces"],
  ["mode_boundary_invariants.rs", "ModeBoundary"],
  ["modern_ts_features.rs", "ModernTsFeatures"],
  ["module_features.rs", "ModuleFeatures"],
  ["narrow_discriminated_union.rs", "FlowNarrowing"],
  ["narrow_equality.rs", "FlowNarrowing"],
  ["narrow_in_operator.rs", "FlowNarrowing"],
  ["narrow_instanceof.rs", "FlowNarrowing"],
  ["narrow_truthiness.rs", "FlowNarrowing"],
  ["narrow_typeof.rs", "FlowNarrowing"],
  ["no_infer.rs", "ConditionalInfer"],
  ["recursive_conditional.rs", "ConditionalInfer"],
  ["relation_semantics.rs", "RelationSemantics"],
  ["substitution_types.rs", "TypeParameterFeatures"],
  ["table_like.rs", "CompositeSurfaces"],
  ["template_literal_inference.rs", "TemplateLiteralInference"],
  ["tuple_labels.rs", "TupleFeatures"],
  ["typescript_rules.rs", "TypeScriptRules"],
  ["union_key_access.rs", "UnionDistribution"],
  ["unique_symbol.rs", "UniqueSymbol"],
  ["utility_composition.rs", "UtilityComposition"],
  ["utility_edge.rs", "UtilityComposition"],
  ["utility_top_bottom.rs", "UtilityComposition"],
  ["value_inference.rs", "ValueInference"],
  ["variadic_tuples.rs", "TupleFeatures"],
  ["wide_deep.rs", "PathProjection"],
]);

// ── Block-id text (from §10.4.1, e.g. `U2.RELATION_INFER`) -> the Rust
//    `TypeInfoParityBlockId` variant. ──
const BLOCK_TEXT_TO_VARIANT = new Map([
  ["U0.MANIFEST_SUBSTRATE", "U0ManifestSubstrate"],
  ["U2.QUERY_VALUE_DOMAIN", "U2QueryValueDomain"],
  ["U8.WIRE_SURFACE_CLOSURE", "U8WireSurfaceClosure"],
  ["U12.EXPORTER", "U12Exporter"],
  ["U13.PROJECTION", "U13Projection"],
  ["U2.RELATION_INFER", "U2RelationInfer"],
  ["U2.UTILITIES", "U2Utilities"],
  ["U2.INDEXED_ACCESS", "U2IndexedAccess"],
  ["U2.MAPPED_TEMPLATE", "U2MappedTemplate"],
  ["U2.CLASS_SURFACES", "U2ClassSurfaces"],
  ["U2.ENUMS", "U2Enums"],
  ["U2.MODULE_AUGMENTATION", "U2ModuleAugmentation"],
  ["U2.JSX_FOUNDATIONS", "U2JsxFoundations"],
  ["U6.FLOW_RETURN_SUBSTRATE", "U6FlowReturnSubstrate"],
  ["U6.NARROW_TYPEOF", "U6NarrowTypeof"],
  ["U6.NARROW_EQUALITY", "U6NarrowEquality"],
  ["U6.NARROW_TRUTHINESS", "U6NarrowTruthiness"],
  ["U6.NARROW_IN", "U6NarrowIn"],
  ["U6.NARROW_INSTANCEOF", "U6NarrowInstanceof"],
  ["U6.NARROW_DISCRIMINATED", "U6NarrowDiscriminated"],
  ["U6.NARROW_SUBSTITUTION", "U6NarrowSubstitution"],
  ["U6.NARROW_INVALIDATION", "U6NarrowInvalidation"],
  ["U6.PREDICATE_ASSERTION", "U6PredicateAssertion"],
  ["U6.CALL_RESOLVE", "U6CallResolve"],
  ["U6.CONTEXTUAL_CALLBACK", "U6ContextualCallback"],
  ["U6.VALUE_INFERENCE", "U6ValueInference"],
  ["U6.ASYNC_GENERATOR", "U6AsyncGenerator"],
  ["U6.CROSS_FILE", "U6CrossFile"],
  ["U6.LOOP_CLOSURE", "U6LoopClosure"],
  ["U3.CACHE_FACT_MODEL", "U3CacheFactModel"],
  ["U10.RESULT_DB", "U10ResultDb"],
  ["U11.PUBLIC_RELATION_SESSION", "U11PublicRelationSession"],
  ["U14.MACRO_ADAPTER", "U14MacroAdapter"],
  ["U15.FINAL_LIFT", "U15FinalLift"],
]);

// ── Each block_id -> its dominant `MechanismId` (one per block;
//    mechanism↔block is a 1:1 ownership bijection — `mechanism_owning_block`
//    is its inverse). This is the BLOCK->mechanism direction, used ONLY for
//    `BlockContractRow.mechanism_id` and to derive a block's
//    `consumed_mechanisms` from its prereqs. A ROW's `mechanism_id` is NOT
//    derived from this map (that would make DAG-guard check 2 tautological);
//    rows derive their mechanism from capability/`file::function` instead —
//    see `CAPABILITY_TO_MECHANISM` + `ROW_MECHANISM_OVERRIDE` +
//    `mechanismForRow` below. ──
const BLOCK_TO_MECHANISM = new Map([
  ["U0ManifestSubstrate", "LedgerCoverageGate"],
  ["U2QueryValueDomain", "QueryValueDomainFoundation"],
  ["U8WireSurfaceClosure", "WireSurfaceClosure"],
  ["U12Exporter", "ExporterPublication"],
  ["U13Projection", "StructuralProjectionDecode"],
  ["U2RelationInfer", "RelateCoinductiveScc"],
  ["U2Utilities", "UtilityGraphReduction"],
  ["U2IndexedAccess", "IndexedAccessUnionDistribution"],
  ["U2MappedTemplate", "MappedTemplateRemap"],
  ["U2ClassSurfaces", "ClassSurfaceProjection"],
  ["U2Enums", "EnumValueTypeDuality"],
  ["U2ModuleAugmentation", "ResolveDeclarationAugmentation"],
  ["U2JsxFoundations", "ResolveAmbientNamespaceJsx"],
  ["U6FlowReturnSubstrate", "ReturnPathPeekerTwoFrontier"],
  ["U6NarrowTypeof", "FlowNarrowingFrameTypeof"],
  ["U6NarrowEquality", "FlowNarrowingFrameEquality"],
  ["U6NarrowTruthiness", "FlowNarrowingFrameTruthiness"],
  ["U6NarrowIn", "FlowNarrowingFrameIn"],
  ["U6NarrowInstanceof", "FlowNarrowingFrameInstanceof"],
  ["U6NarrowDiscriminated", "FlowNarrowingFrameDiscriminated"],
  ["U6NarrowSubstitution", "FlowNarrowingFrameSubstitution"],
  ["U6NarrowInvalidation", "FlowNarrowingFrameInvalidation"],
  ["U6PredicateAssertion", "PredicateAssertionEffect"],
  ["U6CallResolve", "ResolveCallDispatch"],
  ["U6ContextualCallback", "ContextualCallbackInference"],
  ["U6ValueInference", "ValueInferenceWidening"],
  ["U6AsyncGenerator", "AsyncGeneratorCarrier"],
  ["U6CrossFile", "CrossFileRouteFact"],
  ["U6LoopClosure", "LoopClosureFixedPoint"],
  ["U3CacheFactModel", "CacheFactModelAdmission"],
  ["U10ResultDb", "ResultDbModeDemandExactness"],
  ["U11PublicRelationSession", "PublicSessionFootprintInvalidation"],
  ["U14MacroAdapter", "MacroSurfaceAdapter"],
  ["U15FinalLift", "CompositeSurfaceFinalLift"],
]);

// Inverse of BLOCK_TO_MECHANISM — mirrors the Rust `mechanism_owning_block`.
const MECHANISM_OWNING_BLOCK = new Map([...BLOCK_TO_MECHANISM].map(([b, m]) => [m, b]));

// ── Each block_id -> its direct prerequisite block_ids (from the
//    subplan `Prerequisites:` statements). Edges restricted to parity
//    block_ids in the enum (U1/U4/U5/S5 non-parity prereqs omitted). For
//    blocks the subplans describe as depending on "the whole U2 / U6
//    parent", the edge points at the DEEPEST child block of that parent
//    (`U2.JSX_FOUNDATIONS` transitively pulls every U2 reducer +
//    `U2.QUERY_VALUE_DOMAIN`; `U6.LOOP_CLOSURE` transitively pulls every
//    U6 block) so the transitive closure equals "whole parent". ──
const BLOCK_PREREQS = new Map([
  ["U0ManifestSubstrate", []],
  ["U2QueryValueDomain", ["U0ManifestSubstrate"]],
  ["U2RelationInfer", ["U2QueryValueDomain"]],
  ["U2IndexedAccess", ["U2QueryValueDomain", "U2RelationInfer"]],
  ["U2MappedTemplate", ["U2QueryValueDomain", "U2RelationInfer", "U2IndexedAccess"]],
  ["U2Utilities", ["U2QueryValueDomain", "U2RelationInfer", "U2IndexedAccess", "U2MappedTemplate"]],
  ["U2ClassSurfaces", ["U2QueryValueDomain", "U2RelationInfer", "U2IndexedAccess"]],
  ["U2Enums", ["U2QueryValueDomain", "U2RelationInfer", "U2IndexedAccess", "U2MappedTemplate"]],
  ["U2ModuleAugmentation", ["U2QueryValueDomain", "U2RelationInfer", "U2IndexedAccess"]],
  [
    "U2JsxFoundations",
    [
      "U2QueryValueDomain",
      "U2RelationInfer",
      "U2IndexedAccess",
      "U2Utilities",
      "U2ClassSurfaces",
      "U2ModuleAugmentation",
    ],
  ],
  ["U6FlowReturnSubstrate", ["U2QueryValueDomain", "U2RelationInfer"]],
  ["U6NarrowTypeof", ["U6FlowReturnSubstrate"]],
  ["U6NarrowEquality", ["U6FlowReturnSubstrate"]],
  ["U6NarrowTruthiness", ["U6FlowReturnSubstrate"]],
  ["U6NarrowIn", ["U6FlowReturnSubstrate"]],
  ["U6NarrowInstanceof", ["U6FlowReturnSubstrate"]],
  ["U6NarrowDiscriminated", ["U6FlowReturnSubstrate"]],
  ["U6NarrowSubstitution", ["U6FlowReturnSubstrate"]],
  ["U6NarrowInvalidation", ["U6FlowReturnSubstrate"]],
  ["U6CallResolve", ["U6FlowReturnSubstrate", "U2RelationInfer", "U2ClassSurfaces"]],
  [
    "U6PredicateAssertion",
    ["U6FlowReturnSubstrate", "U6NarrowInvalidation", "U6NarrowSubstitution", "U6CallResolve"],
  ],
  ["U6ContextualCallback", ["U6CallResolve", "U6FlowReturnSubstrate", "U6NarrowDiscriminated"]],
  ["U6ValueInference", ["U6FlowReturnSubstrate", "U6CallResolve"]],
  ["U6AsyncGenerator", ["U6FlowReturnSubstrate"]],
  ["U6CrossFile", ["U6ValueInference", "U6CallResolve"]],
  ["U6LoopClosure", ["U6CallResolve", "U6PredicateAssertion"]],
  // Wire surface depends on U0 + the whole U2 + U6 parents.
  ["U8WireSurfaceClosure", ["U0ManifestSubstrate", "U2JsxFoundations", "U6LoopClosure"]],
  ["U3CacheFactModel", ["U8WireSurfaceClosure", "U2JsxFoundations", "U6LoopClosure"]],
  [
    "U10ResultDb",
    ["U3CacheFactModel", "U8WireSurfaceClosure", "U2JsxFoundations", "U6LoopClosure"],
  ],
  [
    "U12Exporter",
    [
      "U10ResultDb",
      "U8WireSurfaceClosure",
      "U3CacheFactModel",
      "U2JsxFoundations",
      "U6LoopClosure",
    ],
  ],
  [
    "U11PublicRelationSession",
    [
      "U12Exporter",
      "U3CacheFactModel",
      "U8WireSurfaceClosure",
      "U2JsxFoundations",
      "U6LoopClosure",
    ],
  ],
  ["U13Projection", ["U12Exporter", "U8WireSurfaceClosure", "U2JsxFoundations", "U6LoopClosure"]],
  [
    "U14MacroAdapter",
    [
      "U13Projection",
      "U11PublicRelationSession",
      "U10ResultDb",
      "U8WireSurfaceClosure",
      "U2JsxFoundations",
      "U6LoopClosure",
    ],
  ],
  [
    "U15FinalLift",
    [
      "U14MacroAdapter",
      "U13Projection",
      "U12Exporter",
      "U11PublicRelationSession",
      "U10ResultDb",
      "U3CacheFactModel",
      "U8WireSurfaceClosure",
      "U2JsxFoundations",
      "U6LoopClosure",
    ],
  ],
]);

// ── block_id -> (owning_u_block, organ). ──
const BLOCK_TO_UBLOCK = new Map([
  ["U0ManifestSubstrate", "U0"],
  ["U2QueryValueDomain", "U2"],
  ["U8WireSurfaceClosure", "U8"],
  ["U12Exporter", "U12"],
  ["U13Projection", "U13"],
  ["U2RelationInfer", "U2"],
  ["U2Utilities", "U2"],
  ["U2IndexedAccess", "U2"],
  ["U2MappedTemplate", "U2"],
  ["U2ClassSurfaces", "U2"],
  ["U2Enums", "U2"],
  ["U2ModuleAugmentation", "U2"],
  ["U2JsxFoundations", "U2"],
  ["U6FlowReturnSubstrate", "U6"],
  ["U6NarrowTypeof", "U6"],
  ["U6NarrowEquality", "U6"],
  ["U6NarrowTruthiness", "U6"],
  ["U6NarrowIn", "U6"],
  ["U6NarrowInstanceof", "U6"],
  ["U6NarrowDiscriminated", "U6"],
  ["U6NarrowSubstitution", "U6"],
  ["U6NarrowInvalidation", "U6"],
  ["U6PredicateAssertion", "U6"],
  ["U6CallResolve", "U6"],
  ["U6ContextualCallback", "U6"],
  ["U6ValueInference", "U6"],
  ["U6AsyncGenerator", "U6"],
  ["U6CrossFile", "U6"],
  ["U6LoopClosure", "U6"],
  ["U3CacheFactModel", "U3"],
  ["U10ResultDb", "U10"],
  ["U11PublicRelationSession", "U11"],
  ["U14MacroAdapter", "U14"],
  ["U15FinalLift", "U15"],
]);

const BLOCK_TO_ORGAN = new Map([
  ["U0ManifestSubstrate", "LedgerSubstrate"],
  ["U2QueryValueDomain", "QueryValueDomain"],
  ["U8WireSurfaceClosure", "WireSurface"],
  ["U12Exporter", "Exporter"],
  ["U13Projection", "Projection"],
  ["U2RelationInfer", "RelationInferenceEngine"],
  ["U2Utilities", "TypeConstructorReducers"],
  ["U2IndexedAccess", "TypeConstructorReducers"],
  ["U2MappedTemplate", "TypeConstructorReducers"],
  ["U2ClassSurfaces", "TypeConstructorReducers"],
  ["U2Enums", "TypeConstructorReducers"],
  ["U2ModuleAugmentation", "TypeConstructorReducers"],
  ["U2JsxFoundations", "TypeConstructorReducers"],
  ["U6FlowReturnSubstrate", "FlowCallSolver"],
  ["U6NarrowTypeof", "FlowCallSolver"],
  ["U6NarrowEquality", "FlowCallSolver"],
  ["U6NarrowTruthiness", "FlowCallSolver"],
  ["U6NarrowIn", "FlowCallSolver"],
  ["U6NarrowInstanceof", "FlowCallSolver"],
  ["U6NarrowDiscriminated", "FlowCallSolver"],
  ["U6NarrowSubstitution", "FlowCallSolver"],
  ["U6NarrowInvalidation", "FlowCallSolver"],
  ["U6PredicateAssertion", "FlowCallSolver"],
  ["U6CallResolve", "FlowCallSolver"],
  ["U6ContextualCallback", "FlowCallSolver"],
  ["U6ValueInference", "FlowCallSolver"],
  ["U6AsyncGenerator", "FlowCallSolver"],
  ["U6CrossFile", "FlowCallSolver"],
  ["U6LoopClosure", "FlowCallSolver"],
  ["U3CacheFactModel", "CacheFactModel"],
  ["U10ResultDb", "ResultDb"],
  ["U11PublicRelationSession", "PublicSession"],
  ["U14MacroAdapter", "FrameworkAdapter"],
  ["U15FinalLift", "FinalLift"],
]);

// ── Per-block REQUIRED GUARDS (§9 / §11.5) ──
//
// `BlockContractRow.required_guards` is the set of verbatim-named structural
// / done-predicate guards from the "Required new guards" (or equivalently-
// named guard list) section of the block's OWNING SUBPLAN DOC that are OWNED
// AND LANDABLE AT THIS BLOCK'S SCOPE. It is NOT a claim of completeness or
// verbatim totality over the doc's full guard list. Every list leads with the
// universal `typeinfo_parity_block_dag_…` guard, then the block-specific
// guards from its contract.
const _DAG = "typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs";
const _NARROW_SHARED = [
  "narrowing_facts_compose_in_predicate_keyed_frames",
  "narrowing_facts_are_program_analysis_not_graph_type_nodes",
  "array_isarray_narrowing_reads_lib_intrinsic_not_text",
];
const BLOCK_TO_REQUIRED_GUARDS = new Map([
  [
    "U0ManifestSubstrate",
    [
      _DAG,
      "ignored_test_row_table_holds_exactly_362_rows",
      "additional_proof_row_table_holds_exactly_7_rows",
      "semantic_query_name_mirror_matches_live_tag_set",
      "every_block_contract_row_carries_required_guards",
      "typeinfo_manifest_files_are_byte_equal_to_regenerated_generator_output",
      "key_owning_block_owner_mapping_is_pinned_closed_set",
    ],
  ],
  [
    "U2QueryValueDomain",
    [
      _DAG,
      "every_semantic_query_key_maps_to_exactly_one_value_domain",
      "flow_contextual_keys_return_program_analysis_value",
      "augmentation_keys_return_declaration_analysis_value",
      "declaration_augmentation_facts_not_type_nodes",
      "relate_query_value_carries_relation_proof_and_budget_state",
      "reserved_checker_queries_are_non_live_typeinfo_does_not_whole_body_check",
      "global_augmentation_query_has_declaration_analysis_identity",
      "declaration_augmentation_target_is_env_free_env_comes_from_context",
      "declaration_augmentation_doc_wire_query_placement_match",
      "resolve_class_surface_key_covers_side_demand_type_args_and_context",
      "apparent_type_key_covers_lib_env_demand_and_context",
      "template_literal_reduce_key_covers_context",
      "relate_key_covers_relation_kind_policy_freshness_and_context",
      "relate_same_nodes_different_relation_kind_policy_or_env_do_not_warm_hit",
      "relate_same_nodes_different_inference_context_do_not_warm_hit",
      "semantic_query_key_spec_table_equals_enum",
      "query_modes_are_presets_over_projection_demand_eval_policy",
      "skeleton_is_typeparamshells_plus_carrier_stop_not_special_mode",
      "cache_key_axes_are_minimal_and_normalized",
    ],
  ],
  [
    "U8WireSurfaceClosure",
    [
      _DAG,
      "node_taxonomy_complete",
      "no_non_type_value_smuggled_into_graph_type_node",
      "flow_contextual_facts_not_graph_type_nodes",
      "program_analysis_graph_exposes_flow_contextual_queries",
      "flow_contextual_doc_and_wire_placement_match_program_analysis_graph",
      "relation_proofs_not_graph_type_nodes",
      "typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node",
      "no_infer_not_type_parameter_metadata",
      "diagnostics_only_on_typeinfo_graph_payload",
      "typeinfo_graph_response_payload_arm_is_additive_not_retyped",
      "framework_surface_payload_graph_payload_is_additive_not_retyped",
      "all_public_semantic_type_graph_embeddings_are_payload_wrapped",
    ],
  ],
  [
    "U12Exporter",
    [
      _DAG,
      "no_non_type_value_smuggled_into_graph_type_node",
      "program_analysis_graph_exposes_flow_contextual_queries",
      "relation_proofs_not_graph_type_nodes",
      "typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node",
      "all_public_semantic_type_graph_embeddings_are_payload_wrapped",
    ],
  ],
  ["U13Projection", [_DAG, "capability_rows_map_to_expected_query_fact_mechanisms"]],
  [
    "U2RelationInfer",
    [
      _DAG,
      "relation_cycle_assumptions_are_scoped_to_full_relate_identity",
      "relation_coinductive_scc_discharges_on_outgoing_obligations",
      "relation_cycle_sentinel_is_never_warm_admitted",
      "relation_proofs_not_graph_type_nodes",
      "typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node",
      "relation_budget_exceeded_admits_nothing",
      "inference_runs_in_checker_transaction_not_per_surface_matcher",
      "only_completed_deterministic_sessions_are_admitted",
      "inference_candidate_combination_matches_priority_and_variance",
      "variance_is_measured_by_marker_probe_fixed_point_not_assumed",
      "reverse_mapped_inference_is_relation_owned_in_session",
      "freshness_tracks_per_property_spread_taint",
      "relation_negative_and_unknown_paths_are_fast",
    ],
  ],
  ["U2Utilities", [_DAG, "keyspace_budget_exceeded_admits_nothing"]],
  ["U2IndexedAccess", [_DAG, "keyspace_budget_exceeded_admits_nothing"]],
  [
    "U2MappedTemplate",
    [
      _DAG,
      "mapped_minus_optional_strips_only_optional_origin_undefined",
      "mapped_minus_optional_preserves_explicit_undefined_on_required_property",
      "template_literal_reduce_models_ts_numeric_bigint_lexing",
      "reverse_mapped_inference_is_relation_owned_in_session",
      "keyspace_budget_exceeded_admits_nothing",
    ],
  ],
  [
    "U2ClassSurfaces",
    [
      _DAG,
      "decorator_identity_method_preserves_declared_return",
      "accessor_decorator_publishes_public_property",
      "decorated_method_literal_union_return_projects",
      "accessor_decorator_identity_target_return_keeps_public_property",
      "apparent_type_budget_exceeded_admits_nothing",
    ],
  ],
  ["U2Enums", [_DAG, "resolve_enum_do_not_warm_hit"]],
  [
    "U2ModuleAugmentation",
    [
      _DAG,
      "global_augmentation_query_has_declaration_analysis_identity",
      "declaration_augmentation_target_is_env_free_env_comes_from_context",
      "declaration_augmentation_facts_not_type_nodes",
      "augmentation_keys_return_declaration_analysis_value",
      "declaration_augmentation_doc_wire_query_placement_match",
      "declaration_merge_records_binder_overload_augmentation_order_as_facts",
      "session_overlay_augmenter_isolated_from_base_index",
      "session_overlay_augmentation_isolated_from_base_meta",
      "node_taxonomy_complete",
    ],
  ],
  [
    "U2JsxFoundations",
    [
      _DAG,
      "jsx_resolution_uses_existing_semantic_queries",
      "jsx_intrinsic_elements_project_via_indexed_access",
      "jsx_no_dedicated_graph_type_node",
    ],
  ],
  [
    "U6FlowReturnSubstrate",
    [
      _DAG,
      "function_flow_graph_built_once_per_function_skeleton",
      "flow_slice_is_graph_reachability_not_procedural_walk",
      "flow_graph_effect_edges_stay_live_past_value_writes",
      "flow_graph_build_is_shallow_interned_no_lowering_lazy_regions",
      "flow_return_routes_through_project_semantic_dispatch",
      "flow_slice_lowered_body_does_not_compute_slice_hash",
      "flow_slice_keys_on_body_sensitive_hash_not_parse_stable_hash",
      "flow_return_key_covers_env_dimensions",
      "flow_return_key_covers_input_context_and_projection_demand",
      "flow_solver_never_slices_source_text",
      "no_flow_slot_in_published_type_surface",
      "flow_slice_budget_exceeded_admits_nothing",
      "program_analysis_fact_domain_validates_flow_slice",
      "flow_slice_ir_detaches_from_oxc_arena",
      "substitution_env_canonical_hash_is_order_independent",
    ],
  ],
  ["U6NarrowTypeof", [_DAG, ..._NARROW_SHARED]],
  ["U6NarrowEquality", [_DAG, ..._NARROW_SHARED]],
  ["U6NarrowTruthiness", [_DAG, ..._NARROW_SHARED]],
  ["U6NarrowIn", [_DAG, ..._NARROW_SHARED]],
  ["U6NarrowInstanceof", [_DAG, ..._NARROW_SHARED]],
  ["U6NarrowDiscriminated", [_DAG, ..._NARROW_SHARED]],
  ["U6NarrowSubstitution", [_DAG, ..._NARROW_SHARED]],
  ["U6NarrowInvalidation", [_DAG, ..._NARROW_SHARED]],
  [
    "U6PredicateAssertion",
    [
      _DAG,
      "predicate_signature_without_body_audits_signature_only_outcome",
      "predicate_assertion_effect_is_signature_metadata_not_published_type_node",
    ],
  ],
  [
    "U6CallResolve",
    [
      _DAG,
      "call_resolution_budget_exceeded_admits_nothing",
      "flow_call_resolves_callee_via_typed_ir_not_text",
      "resolve_call_key_covers_args_this_contextual_type_overload_policy_and_context",
      "resolve_call_same_expr_different_flow_or_substitution_does_not_warm_hit",
      "checker_reentry_graph_spans_flow_call_contextual_narrowing",
      "cross_engine_cycle_discharge_admits_only_stable_deterministic_results",
    ],
  ],
  [
    "U6ContextualCallback",
    [
      _DAG,
      "callback_contextual_typing_does_not_pollute_caller_frame",
      "contextual_callback_input_signature_differentiates_cache_candidates",
      "this_type_contextual_object_literal_binding_in_contextual_type_at",
    ],
  ],
  [
    "U6ValueInference",
    [
      _DAG,
      "satisfies_does_not_widen_returned_value",
      "flow_return_spread_reduces_left_to_right_later_write_wins",
      "freshness_tracks_per_property_spread_taint",
    ],
  ],
  [
    "U6AsyncGenerator",
    [
      _DAG,
      "lib_env_hash_drives_generator_return_resolution",
      "async_return_wraps_in_promise_via_builtin_utility",
    ],
  ],
  [
    "U6CrossFile",
    [
      _DAG,
      "cross_file_flow_routes_via_resolver_core",
      "cross_file_recursion_terminates_with_audit_event",
      "value_type_namespace_split_does_not_leak",
      "flow_cycle_sentinel_is_never_admitted_as_cache_entry",
      "flow_cycle_sentinel_does_not_hide_real_base_return_contributor",
    ],
  ],
  [
    "U6LoopClosure",
    [
      _DAG,
      "no_caching_of_partial_or_budget_exceeded_results",
      "closure_capture_barrier_widens_captured_mutable_slots",
      "predicate_call_does_not_trigger_closure_barrier",
      "divergent_loop_models_as_void",
      "flow_policy_differentiates_cache_candidates",
    ],
  ],
  [
    "U3CacheFactModel",
    [
      _DAG,
      "relation_budget_exceeded_admits_nothing",
      "keyspace_budget_exceeded_admits_nothing",
      "call_resolution_budget_exceeded_admits_nothing",
      "apparent_type_budget_exceeded_admits_nothing",
      "program_analysis_fact_domain_validates_flow_slice",
      "cache_candidate_cap_is_per_family_not_uniform",
      "family_eviction_prefers_invalid_then_lru_valid_hit",
      "cache_keys_cover_ts_jsx_moduleresolution_decorator_lib_dimensions",
      "instantiation_depth_policy_in_identity_and_facts",
      "persistent_caches_never_admit_overlay_only_results",
      "architecture_minimizes_fallback_entry_not_fallback_cost",
    ],
  ],
  [
    "U10ResultDb",
    [
      _DAG,
      "cache_satisfaction_is_materialized_point_not_nominal_demand",
      "backfill_writes_only_recorded_materialized_points",
      "result_db_published_boundary_serves_only_recorded_materialized_points",
    ],
  ],
  [
    "U11PublicRelationSession",
    [
      _DAG,
      "relate_query_value_carries_relation_proof_and_budget_state",
      "relation_proofs_not_graph_type_nodes",
      "typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node",
    ],
  ],
  ["U14MacroAdapter", [_DAG, "component_meta_is_thin_framework_adapter_no_second_resolver"]],
  [
    "U15FinalLift",
    [
      _DAG,
      "all_typeinfo_parity_rows_lifted_except_stop_gates",
      "svelte_adapter_stop_gate_is_registered_out_of_scope",
      "react_adapter_stop_gate_is_registered_out_of_scope",
      "bench_result_row_reports_cache_mode_sourcemap_batch_thread_hit_fallback",
      "architecture_minimizes_fallback_entry_not_fallback_cost",
      "ignored_test_row_table_holds_exactly_362_rows",
      "no_landed_typeinfo_block_has_live_ignored_rows",
      "no_vacuous_parent_u_block_landing",
      "every_manifest_row_has_non_placeholder_mechanism_and_executable_proof",
      "capability_rows_map_to_expected_query_fact_mechanisms",
      "external_corpus_paths_not_present_outside_gated_tests",
    ],
  ],
]);

// ── Per-block VERIFICATION COMMAND LABELS (§9) ──
const BLOCK_VERIFICATION_LABELS = [
  "cargo test -p verter_session --test typeinfo_ignored_test_manifest",
  "cargo nextest run --workspace",
  "cargo test -p verter_session --tests",
  "cargo clippy --workspace -- -D warnings",
  "cargo fmt --all --check",
  "pnpm test",
  "pnpm install --frozen-lockfile",
];

// -- ROW-LEVEL mechanism, INDEPENDENT of the `block_id` column --
//
// A row's dominant `mechanism_id` is its ROW-LEVEL mechanism: the row's
// `file::function` mechanism is what FIXES its owning block, NOT the other
// way round. The mechanism is derived from `(capability [, file::function
// override for capabilities that SPLIT across blocks])`, neither of which
// reads the `block_id` column, so DAG-guard check 2 genuinely discriminates.

/** Tuple-key encoder for `(file, function)` maps (identifiers, no NUL). */
function tkey(file_, fn_) {
  return `${file_} ${fn_}`;
}

const CAPABILITY_TO_MECHANISM = new Map([
  ["ApparentTypes", "ClassSurfaceProjection"],
  ["AuditFootprint", "PublicSessionFootprintInvalidation"],
  ["CacheInvalidation", "PublicSessionFootprintInvalidation"],
  ["ClassFeatures", "ClassSurfaceProjection"],
  ["CompositeSurfaces", "CompositeSurfaceFinalLift"],
  ["ConditionalInfer", "RelateCoinductiveScc"],
  ["ContextualTyping", "ContextualCallbackInference"],
  ["CrossFileResolution", "CacheFactModelAdmission"],
  ["EnumResolution", "EnumValueTypeDuality"],
  ["ExpansionBoundaries", "ResultDbModeDemandExactness"],
  ["IndexSignatures", "IndexedAccessUnionDistribution"],
  ["JsxResolution", "ResolveAmbientNamespaceJsx"],
  ["MacroResolution", "MacroSurfaceAdapter"],
  ["MappedTypes", "MappedTemplateRemap"],
  ["ModeBoundary", "ResultDbModeDemandExactness"],
  ["ModuleFeatures", "ResolveDeclarationAugmentation"],
  ["PathProjection", "IndexedAccessUnionDistribution"],
  ["RelationSemantics", "RelateCoinductiveScc"],
  ["TemplateLiteralInference", "MappedTemplateRemap"],
  ["TupleFeatures", "UtilityGraphReduction"],
  ["UnionDistribution", "IndexedAccessUnionDistribution"],
  ["UniqueSymbol", "ClassSurfaceProjection"],
  ["UtilityComposition", "UtilityGraphReduction"],
  ["ValueInference", "ReturnPathPeekerTwoFrontier"],
]);

// Capabilities whose rows SPLIT across blocks by their `file::function`
// mechanism -- every such row MUST carry a `ROW_MECHANISM_OVERRIDE` entry.
// These do NOT appear in `CAPABILITY_TO_MECHANISM`.
const SPLIT_CAPABILITIES = new Set([
  "CallResolution",
  "DemandBoundary",
  "FlowNarrowing",
  "ModernTsFeatures",
  "TypeParameterFeatures",
  "TypeScriptRules",
]);

// Per-`(file, function)` mechanism for every row of a split capability.
const ROW_MECHANISM_OVERRIDE = new Map([
  [
    tkey("modern_ts_features.rs", "satisfies_array_literal_widens_to_primitive_array"),
    "RelateCoinductiveScc",
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_distributive_conditional_expands_each_union_arm"),
    "RelateCoinductiveScc",
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_awaited_recursively_unwraps_promises"),
    "UtilityGraphReduction",
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_indexed_access_reduces_terminal_property"),
    "IndexedAccessUnionDistribution",
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_keyof_materializes_literal_key_union"),
    "IndexedAccessUnionDistribution",
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_tuple_rest_element_resolves_array_element_type"),
    "IndexedAccessUnionDistribution",
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_key_remap_exclude_filters_and_renames_keys"),
    "MappedTemplateRemap",
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_template_intrinsic_evaluates_union"),
    "MappedTemplateRemap",
  ],
  [
    tkey(
      "call_resolution.rs",
      "call_resolution_abstract_constructor_instance_type_projects_class_shape",
    ),
    "ClassSurfaceProjection",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_call_construct_hybrid_constructor_parameters_uses_construct_signature",
    ),
    "ClassSurfaceProjection",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_call_construct_hybrid_instance_type_uses_construct_signature",
    ),
    "ClassSurfaceProjection",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_call_construct_hybrid_parameters_uses_call_signature",
    ),
    "ClassSurfaceProjection",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_call_construct_hybrid_return_type_uses_call_signature",
    ),
    "ClassSurfaceProjection",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_class_method_prototype_extraction_projects_parameters",
    ),
    "ClassSurfaceProjection",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_class_method_prototype_extraction_projects_return",
    ),
    "ClassSurfaceProjection",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_constructor_parameters_publishes_constructor_arg_tuple",
    ),
    "ClassSurfaceProjection",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_instance_type_publishes_constructor_return_shape",
    ),
    "ClassSurfaceProjection",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_return_type_of_overloaded_function_uses_last_overload",
    ),
    "ClassSurfaceProjection",
  ],
  [
    tkey(
      "modern_ts_features.rs",
      "variance_annotation_in_substitution_through_consumer_consume_parameters",
    ),
    "ClassSurfaceProjection",
  ],
  [
    tkey(
      "substitution_types.rs",
      "substitution_types_sb14_default_type_arg_ignored_by_return_type",
    ),
    "ClassSurfaceProjection",
  ],
  [
    tkey("substitution_types.rs", "substitution_types_sb15_recursive_generic_substitution"),
    "ClassSurfaceProjection",
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_class_instance_type_includes_fields_and_methods"),
    "ClassSurfaceProjection",
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_constructor_parameters_resolve_tuple"),
    "ClassSurfaceProjection",
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_instance_type_resolves_constructed_object"),
    "ClassSurfaceProjection",
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_typeof_const_preserves_readonly_literals"),
    "ClassSurfaceProjection",
  ],
  [
    tkey("modern_ts_features.rs", "import_attribute_simulated_resolves_imported_json_shape"),
    "ResolveDeclarationAugmentation",
  ],
  [
    tkey("modern_ts_features.rs", "import_attribute_simulated_string_literal_indexed_member"),
    "ResolveDeclarationAugmentation",
  ],
  [
    tkey("narrow_typeof.rs", "narrow_typeof_nt01_string_on_binary_union"),
    "FlowNarrowingFrameTypeof",
  ],
  [
    tkey("narrow_typeof.rs", "narrow_typeof_nt02_number_on_triple_union"),
    "FlowNarrowingFrameTypeof",
  ],
  [tkey("narrow_typeof.rs", "narrow_typeof_nt03_boolean_on_union"), "FlowNarrowingFrameTypeof"],
  [
    tkey("narrow_typeof.rs", "narrow_typeof_nt04_object_on_union_keeps_no_null"),
    "FlowNarrowingFrameTypeof",
  ],
  [tkey("narrow_typeof.rs", "narrow_typeof_nt05_function_on_union"), "FlowNarrowingFrameTypeof"],
  [tkey("narrow_typeof.rs", "narrow_typeof_nt06_undefined_on_union"), "FlowNarrowingFrameTypeof"],
  [tkey("narrow_typeof.rs", "narrow_typeof_nt07_bigint_on_union"), "FlowNarrowingFrameTypeof"],
  [tkey("narrow_typeof.rs", "narrow_typeof_nt08_symbol_on_union"), "FlowNarrowingFrameTypeof"],
  [tkey("narrow_typeof.rs", "narrow_typeof_nt09_string_on_unknown"), "FlowNarrowingFrameTypeof"],
  [
    tkey("narrow_typeof.rs", "narrow_typeof_nt10_string_on_unbound_generic"),
    "FlowNarrowingFrameTypeof",
  ],
  [
    tkey("narrow_typeof.rs", "narrow_typeof_nt11_negated_on_binary_union"),
    "FlowNarrowingFrameTypeof",
  ],
  [tkey("narrow_typeof.rs", "narrow_typeof_nt12_switch_exhaustive"), "FlowNarrowingFrameTypeof"],
  [
    tkey("narrow_typeof.rs", "narrow_typeof_nt13_negated_guard_early_return"),
    "FlowNarrowingFrameTypeof",
  ],
  [
    tkey("narrow_typeof.rs", "narrow_typeof_nt14_compare_literal_var_does_not_narrow"),
    "FlowNarrowingFrameTypeof",
  ],
  [
    tkey("narrow_typeof.rs", "narrow_typeof_nt15_compound_and_property"),
    "FlowNarrowingFrameTypeof",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq01_string_literal_on_literal_union"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq02_negated_string_literal_on_literal_union"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq03_number_literal_on_triple_union"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq04_boolean_true_on_boolean"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq05_null_on_nullable_string"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq06_undefined_on_optional_string"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq07_double_equals_null_on_nullish_string"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq08_string_literal_on_string_does_not_narrow"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq09_string_literal_on_primitive_union"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq10_two_unions_mutual_equality_does_not_narrow"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq11_impossible_compound_absorbs_never"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq12_property_equality_discriminant"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq13_as_const_literal_rhs"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq14_number_literal_on_number_does_not_narrow"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_equality.rs", "narrow_equality_eq15_nan_equality_does_not_narrow"),
    "FlowNarrowingFrameEquality",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr01_string_or_undefined"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr02_string_or_null"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr03_string_or_nullish"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr04_string_no_nullable_does_not_narrow"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr05_number_literal_union"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr06_string_literal_union"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr07_boolean_union"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr08_negated_string_or_undefined"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr09_property_truthiness"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr10_early_return_guard"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr11_unknown_collapses_to_unknown"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr12_object_or_null"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr13_compound_and_chain"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr14_number_or_undefined_does_not_split_zero"),
    "FlowNarrowingFrameTruthiness",
  ],
  [
    tkey("narrow_truthiness.rs", "narrow_truthiness_tr15_optional_chain_truthiness"),
    "FlowNarrowingFrameTruthiness",
  ],
  [tkey("narrow_in_operator.rs", "narrow_in_operator_io01_binary_union"), "FlowNarrowingFrameIn"],
  [tkey("narrow_in_operator.rs", "narrow_in_operator_io02_shared_key"), "FlowNarrowingFrameIn"],
  [tkey("narrow_in_operator.rs", "narrow_in_operator_io03_else_branch"), "FlowNarrowingFrameIn"],
  [tkey("narrow_in_operator.rs", "narrow_in_operator_io04_intersection"), "FlowNarrowingFrameIn"],
  [
    tkey("narrow_in_operator.rs", "narrow_in_operator_io05_optional_property"),
    "FlowNarrowingFrameIn",
  ],
  [tkey("narrow_in_operator.rs", "narrow_in_operator_io06_on_unknown"), "FlowNarrowingFrameIn"],
  [tkey("narrow_in_operator.rs", "narrow_in_operator_io07_on_object"), "FlowNarrowingFrameIn"],
  [
    tkey("narrow_in_operator.rs", "narrow_in_operator_io08_compound_conjunction"),
    "FlowNarrowingFrameIn",
  ],
  [tkey("narrow_in_operator.rs", "narrow_in_operator_io09_negated"), "FlowNarrowingFrameIn"],
  [
    tkey("narrow_in_operator.rs", "narrow_in_operator_io10_three_arm_union"),
    "FlowNarrowingFrameIn",
  ],
  [
    tkey("narrow_in_operator.rs", "narrow_in_operator_io11_generic_constrained"),
    "FlowNarrowingFrameIn",
  ],
  [
    tkey("narrow_in_operator.rs", "narrow_in_operator_io12_reassignment_renarrowing"),
    "FlowNarrowingFrameIn",
  ],
  [
    tkey("narrow_in_operator.rs", "narrow_in_operator_io13_class_vs_object"),
    "FlowNarrowingFrameIn",
  ],
  [
    tkey("narrow_in_operator.rs", "narrow_in_operator_io14_template_literal_key"),
    "FlowNarrowingFrameIn",
  ],
  [tkey("narrow_in_operator.rs", "narrow_in_operator_io15_symbol_key"), "FlowNarrowingFrameIn"],
  [
    tkey("narrow_instanceof.rs", "narrow_instanceof_in01_binary_union"),
    "FlowNarrowingFrameInstanceof",
  ],
  [
    tkey("narrow_instanceof.rs", "narrow_instanceof_in02_class_plus_primitive"),
    "FlowNarrowingFrameInstanceof",
  ],
  [
    tkey("narrow_instanceof.rs", "narrow_instanceof_in03_on_unknown"),
    "FlowNarrowingFrameInstanceof",
  ],
  [
    tkey("narrow_instanceof.rs", "narrow_instanceof_in04_subclass_union"),
    "FlowNarrowingFrameInstanceof",
  ],
  [
    tkey("narrow_instanceof.rs", "narrow_instanceof_in05_already_narrowed"),
    "FlowNarrowingFrameInstanceof",
  ],
  [
    tkey("narrow_instanceof.rs", "narrow_instanceof_in06_abstract_class"),
    "FlowNarrowingFrameInstanceof",
  ],
  [
    tkey("narrow_instanceof.rs", "narrow_instanceof_in07_else_reachability"),
    "FlowNarrowingFrameInstanceof",
  ],
  [
    tkey("narrow_instanceof.rs", "narrow_instanceof_in08_interface_union"),
    "FlowNarrowingFrameInstanceof",
  ],
  [
    tkey("narrow_instanceof.rs", "narrow_instanceof_in09_negated_early_return"),
    "FlowNarrowingFrameInstanceof",
  ],
  [
    tkey("narrow_instanceof.rs", "narrow_instanceof_in10_intersection"),
    "FlowNarrowingFrameInstanceof",
  ],
  [
    tkey("narrow_instanceof.rs", "narrow_instanceof_in11_generic_ctor"),
    "FlowNarrowingFrameInstanceof",
  ],
  [
    tkey("narrow_instanceof.rs", "narrow_instanceof_in13_array_special_case"),
    "FlowNarrowingFrameInstanceof",
  ],
  [
    tkey("narrow_instanceof.rs", "narrow_instanceof_in14_promise_special_case"),
    "FlowNarrowingFrameInstanceof",
  ],
  [tkey("narrow_instanceof.rs", "narrow_instanceof_in15_nullable"), "FlowNarrowingFrameInstanceof"],
  [
    tkey(
      "narrow_discriminated_union.rs",
      "narrow_discriminated_union_du01_if_equality_discriminant",
    ),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey("narrow_discriminated_union.rs", "narrow_discriminated_union_du02_switch_discriminant"),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey("narrow_discriminated_union.rs", "narrow_discriminated_union_du03_switch_default_never"),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey("narrow_discriminated_union.rs", "narrow_discriminated_union_du04_negated_discriminant"),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey(
      "narrow_discriminated_union.rs",
      "narrow_discriminated_union_du05_multi_property_discriminant",
    ),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey("narrow_discriminated_union.rs", "narrow_discriminated_union_du06_nested_discriminant"),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey(
      "narrow_discriminated_union.rs",
      "narrow_discriminated_union_du07_number_literal_discriminant",
    ),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey(
      "narrow_discriminated_union.rs",
      "narrow_discriminated_union_du08_boolean_literal_discriminant",
    ),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey(
      "narrow_discriminated_union.rs",
      "narrow_discriminated_union_du09_destructure_correlation",
    ),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey(
      "narrow_discriminated_union.rs",
      "narrow_discriminated_union_du10_in_guard_plus_discriminant",
    ),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey("narrow_discriminated_union.rs", "narrow_discriminated_union_du11_switch_per_arm_join"),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey("narrow_discriminated_union.rs", "narrow_discriminated_union_du12_switch_fall_through"),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey(
      "narrow_discriminated_union.rs",
      "narrow_discriminated_union_du14_reassignment_re_narrowing",
    ),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey(
      "narrow_discriminated_union.rs",
      "narrow_discriminated_union_du15_template_literal_discriminant",
    ),
    "FlowNarrowingFrameDiscriminated",
  ],
  [
    tkey("substitution_types.rs", "substitution_types_sb01_bare_narrowing_of_generic"),
    "FlowNarrowingFrameSubstitution",
  ],
  [
    tkey("substitution_types.rs", "substitution_types_sb02_narrowing_in_constrained_generic"),
    "FlowNarrowingFrameSubstitution",
  ],
  [
    tkey("substitution_types.rs", "substitution_types_sb03_substitution_survives_method_calls"),
    "FlowNarrowingFrameSubstitution",
  ],
  [
    tkey(
      "substitution_types.rs",
      "substitution_types_sb04_narrowed_substitution_to_return_position",
    ),
    "FlowNarrowingFrameSubstitution",
  ],
  [
    tkey("substitution_types.rs", "substitution_types_sb05_compound_typeof_and_instanceof"),
    "FlowNarrowingFrameSubstitution",
  ],
  [
    tkey("substitution_types.rs", "substitution_types_sb06_narrowing_widens_after_reassignment"),
    "FlowNarrowingFrameSubstitution",
  ],
  [
    tkey("substitution_types.rs", "substitution_types_sb07_constraint_flow_apparent_type"),
    "FlowNarrowingFrameSubstitution",
  ],
  [
    tkey(
      "substitution_types.rs",
      "substitution_types_sb08_generic_in_conditional_no_distribute_on_unknown",
    ),
    "FlowNarrowingFrameSubstitution",
  ],
  [
    tkey("substitution_types.rs", "substitution_types_sb11_generic_narrowed_via_in_operator"),
    "FlowNarrowingFrameSubstitution",
  ],
  [
    tkey("substitution_types.rs", "substitution_types_sb12_truthiness_on_t_or_undefined"),
    "FlowNarrowingFrameSubstitution",
  ],
  [
    tkey(
      "substitution_types.rs",
      "substitution_types_sb13_substitution_carried_across_destructure",
    ),
    "FlowNarrowingFrameSubstitution",
  ],
  [
    tkey(
      "flow_invalidations.rs",
      "flow_invalidations_fi01_reassignment_invalidates_string_narrowing",
    ),
    "FlowNarrowingFrameInvalidation",
  ],
  [
    tkey("flow_invalidations.rs", "flow_invalidations_fi02_narrowing_preserved_across_opaque_call"),
    "FlowNarrowingFrameInvalidation",
  ],
  [
    tkey(
      "flow_invalidations.rs",
      "flow_invalidations_fi04_destructured_discriminant_preserves_correlation",
    ),
    "FlowNarrowingFrameInvalidation",
  ],
  [
    tkey(
      "flow_invalidations.rs",
      "flow_invalidations_fi05_destructured_discriminant_loses_on_reassignment",
    ),
    "FlowNarrowingFrameInvalidation",
  ],
  [
    tkey(
      "flow_invalidations.rs",
      "flow_invalidations_fi09_exhaustive_never_tail_does_not_widen_return",
    ),
    "FlowNarrowingFrameInvalidation",
  ],
  [
    tkey("flow_invalidations.rs", "flow_invalidations_fi08_asserts_narrows_dotted_member_path"),
    "PredicateAssertionEffect",
  ],
  [
    tkey("substitution_types.rs", "substitution_types_sb09_asserts_x_is_string_on_generic"),
    "PredicateAssertionEffect",
  ],
  [
    tkey("substitution_types.rs", "substitution_types_sb10_x_is_t_predicate_on_generic"),
    "PredicateAssertionEffect",
  ],
  [
    tkey(
      "call_resolution.rs",
      "call_resolution_extracted_prototype_method_call_returns_declared_return",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey("call_resolution.rs", "call_resolution_generic_infers_from_callback_return_type"),
    "ResolveCallDispatch",
  ],
  [
    tkey(
      "call_resolution.rs",
      "call_resolution_generic_infers_from_positional_argument_through_callback_signature",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey(
      "call_resolution.rs",
      "call_resolution_generic_infers_object_literal_including_excess_properties",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey(
      "call_resolution.rs",
      "call_resolution_optional_overload_picks_first_arity_matching_signature",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey(
      "call_resolution.rs",
      "call_resolution_optional_overload_picks_two_arg_signature_when_required",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey("call_resolution.rs", "call_resolution_rest_overload_picks_rest_signature_when_required"),
    "ResolveCallDispatch",
  ],
  [
    tkey(
      "call_resolution.rs",
      "call_resolution_specific_literal_argument_picks_matching_overload_first",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey(
      "call_resolution.rs",
      "call_resolution_specific_literal_argument_skips_non_matching_first_overload",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey("call_resolution.rs", "call_resolution_this_receiver_method_call_returns_declared_return"),
    "ResolveCallDispatch",
  ],
  [
    tkey("call_resolution.rs", "call_resolution_union_argument_picks_union_compatible_overload"),
    "ResolveCallDispatch",
  ],
  [
    tkey(
      "const_type_param.rs",
      "const_type_param_route_call_preserves_readonly_tuple_with_literal_paths",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey(
      "const_type_param.rs",
      "const_type_param_string_call_preserves_readonly_literal_string_tuple",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_constrained_generic_infers_literal_under_as_const",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_higher_order_composition_returns_concrete_function",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_omit_this_parameter_returns_function_without_this",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey("function_advanced.rs", "function_advanced_overload_call_picks_matching_signature_return"),
    "ResolveCallDispatch",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_overload_generic_first_binds_to_literal_argument",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_overload_generic_first_widens_t_to_string_for_string_argument",
    ),
    "ResolveCallDispatch",
  ],
  [
    tkey("function_advanced.rs", "function_advanced_this_parameter_type_returns_this_annotation"),
    "ResolveCallDispatch",
  ],
  [
    tkey("function_advanced.rs", "function_advanced_void_callback_return_preserves_void"),
    "ResolveCallDispatch",
  ],
  [
    tkey("call_resolution.rs", "call_resolution_contextual_callback_return_picks_first_overload"),
    "ContextualCallbackInference",
  ],
  [
    tkey(
      "flow_return_catalog.rs",
      "flow_return_ho09_keeps_unknown_declared_callback_result_opaque",
    ),
    "ContextualCallbackInference",
  ],
  [
    tkey("modern_ts_features.rs", "satisfies_widens_inner_value_to_primitive_without_as_const"),
    "ValueInferenceWidening",
  ],
  [
    tkey("modern_ts_features.rs", "await_using_simulated_return_type_resolves_to_primitive"),
    "AsyncGeneratorCarrier",
  ],
  [
    tkey("flow_return_catalog.rs", "flow_return_xf02_expands_imported_value_function_return"),
    "CrossFileRouteFact",
  ],
  [
    tkey(
      "flow_return_catalog.rs",
      "flow_return_xf04_expands_barrel_imported_value_function_return",
    ),
    "CrossFileRouteFact",
  ],
  [
    tkey("flow_return_catalog.rs", "flow_return_xf04_records_barrel_route_before_selected_leaf"),
    "CrossFileRouteFact",
  ],
  [
    tkey("flow_return_catalog.rs", "flow_return_xf05_resolves_namespace_import_value_call"),
    "CrossFileRouteFact",
  ],
  [
    tkey("flow_return_catalog.rs", "flow_return_xf06_keeps_value_type_namespace_separate"),
    "CrossFileRouteFact",
  ],
  [
    tkey("flow_return_catalog.rs", "flow_return_xf09_terminates_cross_file_recursive_returns"),
    "CrossFileRouteFact",
  ],
  [
    tkey(
      "flow_invalidations.rs",
      "flow_invalidations_fi03_closure_capture_preserves_narrowing_at_return",
    ),
    "LoopClosureFixedPoint",
  ],
  [
    tkey(
      "flow_invalidations.rs",
      "flow_invalidations_fi06_finally_return_overrides_try_catch_returns",
    ),
    "LoopClosureFixedPoint",
  ],
  [
    tkey(
      "flow_invalidations.rs",
      "flow_invalidations_fi07_finally_without_return_preserves_try_catch",
    ),
    "LoopClosureFixedPoint",
  ],
  [
    tkey(
      "demand_boundary.rs",
      "demand_boundary_projection_into_selected_alias_loads_needed_but_not_unused",
    ),
    "ResultDbModeDemandExactness",
  ],
  [
    tkey(
      "demand_boundary.rs",
      "demand_boundary_terminal_projection_resolves_value_without_unused_branch",
    ),
    "ResultDbModeDemandExactness",
  ],
  [
    tkey(
      "demand_boundary.rs",
      "demand_boundary_barrel_resolution_does_not_load_unrequested_reexport",
    ),
    "PublicSessionFootprintInvalidation",
  ],
]);

// -- mechanism -> the live `SemanticQueryName`s that MECHANISM
//    dispatches/reads. The FULL set per mechanism (no per-block narrowing).
//    For the correct table every key's owner is reachable from the
//    mechanism's owning block -- asserted at generation time. --
const MECHANISM_TO_KEYS = new Map([
  // Zero-row substrate mechanisms.
  ["LedgerCoverageGate", []],
  [
    "QueryValueDomainFoundation",
    ["ResolveDecl", "TypeOf", "NormalizeUnion", "NormalizeIntersection"],
  ],
  ["WireSurfaceClosure", []],
  ["ExporterPublication", []],
  ["StructuralProjectionDecode", []],
  // U2 reducer mechanisms.
  ["RelateCoinductiveScc", ["Relate", "Conditional", "Instantiate", "ResolveDecl"]],
  ["UtilityGraphReduction", ["Instantiate", "IndexedAccess", "KeyOf", "ResolveDecl"]],
  [
    "IndexedAccessUnionDistribution",
    ["IndexedAccess", "KeyOf", "ProjectMember", "ProjectPath", "ResolveDecl"],
  ],
  ["MappedTemplateRemap", ["MappedType", "KeyOf", "Instantiate", "Conditional", "ResolveDecl"]],
  ["ClassSurfaceProjection", ["ResolveDecl", "Instantiate", "Relate"]],
  ["EnumValueTypeDuality", ["ResolveDecl", "KeyOf", "TypeOf"]],
  ["ResolveDeclarationAugmentation", ["ResolveDecl", "IndexedAccess"]],
  ["ResolveAmbientNamespaceJsx", ["ResolveDecl", "IndexedAccess", "KeyOf"]],
  // U6 flow / call mechanisms.
  ["ReturnPathPeekerTwoFrontier", ["TypeOf", "ResolveDecl"]],
  ["FlowNarrowingFrameTypeof", ["ResolveDecl", "Relate"]],
  ["FlowNarrowingFrameEquality", ["ResolveDecl", "Relate"]],
  ["FlowNarrowingFrameTruthiness", ["ResolveDecl", "Relate"]],
  ["FlowNarrowingFrameIn", ["ResolveDecl", "Relate"]],
  ["FlowNarrowingFrameInstanceof", ["ResolveDecl", "Relate"]],
  ["FlowNarrowingFrameDiscriminated", ["ResolveDecl", "Relate"]],
  ["FlowNarrowingFrameSubstitution", ["Instantiate", "Relate", "ResolveDecl"]],
  ["FlowNarrowingFrameInvalidation", ["ResolveDecl", "Relate"]],
  ["PredicateAssertionEffect", ["ResolveDecl", "Relate", "Instantiate"]],
  ["ResolveCallDispatch", ["ResolveDecl", "Instantiate", "Relate"]],
  ["ContextualCallbackInference", ["ResolveDecl", "Relate", "Instantiate"]],
  ["ValueInferenceWidening", ["TypeOf", "ResolveDecl"]],
  ["AsyncGeneratorCarrier", ["ResolveDecl", "Instantiate"]],
  ["CrossFileRouteFact", ["ResolveDecl"]],
  ["LoopClosureFixedPoint", ["ResolveDecl", "Relate"]],
  // Cache / result / session / adapter mechanisms.
  ["CacheFactModelAdmission", ["ResolveDecl"]],
  ["ResultDbModeDemandExactness", ["ResolveDecl", "ProjectPath"]],
  ["PublicSessionFootprintInvalidation", ["ResolveDecl"]],
  ["MacroSurfaceAdapter", ["ResolveDecl", "Instantiate", "ResolveMacroPayload"]],
  ["CompositeSurfaceFinalLift", ["ResolveDecl", "ProjectPath", "Instantiate"]],
]);

/**
 * Approximate Python `repr()` of a string (single-quote wrapped; identifiers
 * only). STDERR-ONLY: used solely in `SystemExit` error messages for malformed
 * capability inputs — it never reaches a generated file, so it is not
 * byte-significant to the manifest output.
 */
function pyRepr(s) {
  if (!s.includes("'")) {
    return `'${s}'`;
  }
  if (!s.includes('"')) {
    return `"${s}"`;
  }
  return `'${s.replaceAll("'", "\\'")}'`;
}

/** Raise a Python-style `SystemExit` with a message + exit code. */
class SystemExit extends Error {
  constructor(code, message) {
    super(message ?? "");
    this.code = code;
    this.sysexitMessage = message;
  }
}

function mechanismForRow(cap, file_, fn_name) {
  // A row's dominant `mechanism_id`, derived from `(capability [,
  // file::function override])` and INDEPENDENT of the `block_id` column.
  if (SPLIT_CAPABILITIES.has(cap)) {
    const key = tkey(file_, fn_name);
    if (!ROW_MECHANISM_OVERRIDE.has(key)) {
      throw new SystemExit(
        1,
        `split-capability row ${file_}::${fn_name} (capability ${pyRepr(cap)}) ` +
          `has no ROW_MECHANISM_OVERRIDE entry -- author its row-level ` +
          `mechanism from §10.4.1 (do NOT fall back to a block-derived ` +
          `placeholder)`,
      );
    }
    return ROW_MECHANISM_OVERRIDE.get(key);
  }
  if (!CAPABILITY_TO_MECHANISM.has(cap)) {
    throw new SystemExit(
      1,
      `capability ${pyRepr(cap)} is neither a split capability nor in ` +
        `CAPABILITY_TO_MECHANISM -- add its row-level mechanism`,
    );
  }
  return CAPABILITY_TO_MECHANISM.get(cap);
}

// -- capability -> its `ProofRequirement`. --
function proofForCapability(cap) {
  const oracle = new Map([
    ["UtilityComposition", "UtilityComposition"],
    ["MappedTypes", "MappedTemplate"],
    ["TemplateLiteralInference", "TemplateLiteral"],
    ["IndexSignatures", "IndexedAccess"],
    ["PathProjection", "IndexedAccess"],
    ["UnionDistribution", "IndexedAccess"],
    ["EnumResolution", "EnumProjection"],
    ["ClassFeatures", "ClassSurface"],
    ["ApparentTypes", "ApparentType"],
    ["UniqueSymbol", "ApparentType"],
    ["TupleFeatures", "TupleProjection"],
    ["ConditionalInfer", "ConditionalInfer"],
    ["RelationSemantics", "RelationSemantics"],
    ["FlowNarrowing", "FlowNarrowing"],
    ["CallResolution", "CallResolution"],
    ["ContextualTyping", "ContextualTyping"],
    ["ValueInference", "ValueInference"],
    ["JsxResolution", "JsxResolution"],
    ["ModuleFeatures", "ModuleAugmentation"],
    ["CompositeSurfaces", "CompositeSurface"],
    ["TypeParameterFeatures", "RelationSemantics"],
    ["TypeScriptRules", "RelationSemantics"],
    ["ModernTsFeatures", "RelationSemantics"],
    ["MacroResolution", "CompositeSurface"],
    ["CrossFileResolution", "RelationSemantics"],
  ]);
  const guard = new Map([
    ["ModeBoundary", "ModeBoundaryExactness"],
    ["ExpansionBoundaries", "ExpansionBoundaryPrecision"],
    ["DemandBoundary", "DemandBoundaryPrecision"],
    ["CacheInvalidation", "CacheInvalidationRoute"],
    ["AuditFootprint", "AuditFootprintAttachment"],
  ]);
  if (guard.has(cap)) {
    return `ProofRequirement::StructuralGuard(GuardId::${guard.get(cap)})`;
  }
  if (oracle.has(cap)) {
    return `ProofRequirement::Ts7Oracle(OracleId::${oracle.get(cap)})`;
  }
  throw new SystemExit(1, `no ProofRequirement mapping for capability ${pyRepr(cap)}`);
}

// -- LIFTED rows: the closed set of rows whose `#[ignore]` has been REMOVED
//    (an oracle snapshot + `ORACLE_QUERY_SPECS` registry entry now back their
//    `oracle::run_row` body), flipping `status` Ignored -> Lifted{block_id}.
//    The row's `block_id` is NOT overridden here -- it comes from §10.4.1.
//    This override map carries ONLY the lift metadata that is NOT expressible
//    in §10.4.1: the mechanism / proof / unblocker prose + the execution-true
//    `semantic_queries` / `consumed_mechanisms`. --
const LIFTED_ROW_OVERRIDES = new Map([
  [
    tkey("utility_top_bottom.rs", "utility_top_bottom_utb15_awaited_unknown_is_unknown"),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::UtilityComposition)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.UTILITIES: `Awaited<unknown>` reduces to `unknown` (no " +
        "thenable branch matches; the final conditional fallthrough returns T) " +
        "through the shared builtin-utility dispatch, proven against the " +
        "checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("utility_top_bottom.rs", "utility_top_bottom_utb17_awaited_null_is_null"),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::UtilityComposition)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.UTILITIES: `Awaited<null>` preserves `null` via the first " +
        "conditional clause (T extends null | undefined ? T : ...), proven " +
        "against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("utility_top_bottom.rs", "utility_top_bottom_utb18_awaited_undefined_is_undefined"),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::UtilityComposition)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.UTILITIES: `Awaited<undefined>` preserves `undefined` via " +
        "the first conditional clause (the nullish short-circuit), proven " +
        "against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey(
      "utility_top_bottom.rs",
      "utility_top_bottom_utb19_awaited_nested_promise_is_inner_primitive",
    ),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::UtilityComposition)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.UTILITIES: `Awaited<Promise<Promise<string>>>` recursively " +
        "unwraps the registry-classified Promise carriers to `string`, proven " +
        "against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("utility_top_bottom.rs", "utility_top_bottom_utb21_non_nullable_unknown_is_empty_object"),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::UtilityComposition)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.UTILITIES: `NonNullable<unknown>` collapses to the empty " +
        "object base (`unknown & {}` = `{}` — NOT unknown, NOT never), proven " +
        "against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_awaited_recursively_unwraps_promises"),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::UtilityComposition)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.UTILITIES: `Awaited<Promise<Promise<{ done: true }>>>` " +
        "recursively unwraps the registry-classified Promise carriers to the " +
        "fulfilled object payload, proven against the checked-in tsgo oracle " +
        "snapshot via oracle::run_row",
    },
  ],
  [
    tkey("utility_edge.rs", "utility_edge_non_nullable_strips_null_and_undefined"),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::UtilityComposition)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.UTILITIES: `NonNullable<string | null | undefined>` filters " +
        "the settled union nullish arms to the bare `string` primitive, " +
        "proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("variadic_tuples.rs", "variadic_tuple_concat_alias_produces_joined_literal_tuple"),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::TupleProjection)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.UTILITIES: `Concat<[1, 2], [3, 4]>` splices the `[...A, ...B]` " +
        "variadic spread into `[1, 2, 3, 4]` via the normalize-on-intern spread " +
        "rule, proven against the checked-in tsgo oracle snapshot via " +
        "oracle::run_row",
    },
  ],
  [
    tkey("mapped_modifiers.rs", "mapped_modifier_minus_optional_strips_optional_and_undefined"),
    {
      mech: "MappedTemplateRemap",
      proof: "ProofRequirement::Ts7Oracle(OracleId::MappedTemplate)",
      semantic_queries: [
        "ResolveDecl",
        "Instantiate",
        "KeyOf",
        "MappedType",
        "ProjectPath",
        "LowerLocator",
      ],
      consumed_mechanisms: ["QueryValueDomainFoundation", "IndexedAccessUnionDistribution"],
      unblocker:
        "lifted by U2.MAPPED_TEMPLATE: `AllRequired<{ a?: string; b?: number }>` " +
        "is the userland mapped type `{ [K in keyof T]-?: T[K] }`; the presence-only " +
        "`-?` optional-remover is the terminal MappedTemplateRemap producer, proven " +
        "against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("index_signatures.rs", "index_signatures_numeric_index_publishes_signature"),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::IndexSignature)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.QUERY_VALUE_DOMAIN: a declared object-type alias " +
        "resolves and publishes its terminal numeric-key index signature, " +
        "proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("index_signatures.rs", "index_signatures_symbol_index_publishes_signature"),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::IndexSignature)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.QUERY_VALUE_DOMAIN: a declared object-type alias " +
        "resolves and publishes its terminal symbol-key index signature, " +
        "proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("utility_edge.rs", "utility_edge_required_strips_optional_markers"),
    {
      mech: "MappedTemplateRemap",
      proof: "ProofRequirement::Ts7Oracle(OracleId::UtilityComposition)",
      semantic_queries: [
        "ResolveDecl",
        "Instantiate",
        "KeyOf",
        "MappedType",
        "ProjectPath",
        "LowerLocator",
      ],
      consumed_mechanisms: ["QueryValueDomainFoundation", "IndexedAccessUnionDistribution"],
      unblocker:
        "lifted by U2.MAPPED_TEMPLATE: `Required<T>` is the library mapped " +
        "type `{ [K in keyof T]-?: T[K] }`; the `-?` optional-stripping remap " +
        "is the terminal MappedTemplateRemap producer, proven against the " +
        "checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("utility_edge.rs", "utility_edge_readonly_required_composes_modifiers"),
    {
      mech: "MappedTemplateRemap",
      proof: "ProofRequirement::Ts7Oracle(OracleId::UtilityComposition)",
      semantic_queries: [
        "ResolveDecl",
        "Instantiate",
        "KeyOf",
        "MappedType",
        "ProjectPath",
        "LowerLocator",
      ],
      consumed_mechanisms: ["QueryValueDomainFoundation", "IndexedAccessUnionDistribution"],
      unblocker:
        "lifted by U2.MAPPED_TEMPLATE: `Readonly<Required<T>>` composes two " +
        "library mapped types; both modifier remaps (`-?` then `+readonly`) " +
        "are MappedTemplateRemap producers, proven against the checked-in " +
        "tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_indexed_access_reduces_terminal_property"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::IndexedAccess)",
      semantic_queries: ["IndexedAccess", "Instantiate", "ResolveDecl", "LowerLocator"],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        "lifted by U2.INDEXED_ACCESS: `IndexedRules = KeySource['nested']['value']` " +
        "reduces the multi-hop indexed-access chain to its terminal `string`, " +
        "proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("deep_path.rs", "deep_path_projection_resolves_terminal_without_losing_shape"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::IndexedAccess)",
      semantic_queries: ["IndexedAccess", "Instantiate", "ResolveDecl", "LowerLocator"],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        "lifted by U2.INDEXED_ACCESS: `DeepProjectedTarget` reduces a 16-hop " +
        "indexed-access chain to the terminal `TerminalPayload` object " +
        "`{ id: string; priority: 1 | 2 | 3 }` without losing shape, proven " +
        "against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("wide_deep.rs", "wide_deep_projected_token_resolves_literal_union"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::IndexedAccess)",
      semantic_queries: [
        "IndexedAccess",
        "Instantiate",
        "ProjectPath",
        "ResolveDecl",
        "LowerLocator",
      ],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        "lifted on its measured trace: `WideDeepProjectedToken` reduces the " +
        "multi-hop indexed-access chain to the literal union " +
        "`'alpha' | 'beta' | 'gamma'` PATH-PRECISELY — the requested `token` " +
        "member projects from the inline `{ token }` intersection arm via " +
        "indexed-access distribution while the non-contributing terminal " +
        "`Pick<TLeaf,'id'|'score'>` arm stays a deferred carrier (no " +
        "KeyOf/MappedType dispatch), proven against the checked-in tsgo " +
        "oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_keyof_materializes_literal_key_union"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::IndexedAccess)",
      semantic_queries: ["ResolveDecl", "Instantiate", "KeyOf", "ProjectPath", "LowerLocator"],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        "lifted by U2.INDEXED_ACCESS: `KeyOfRules = keyof KeySource` materializes " +
        "the expanded literal key union `'count' | 'id' | 'nested'`, proven against " +
        "the checked-in tsgo oracle snapshot (captured through the " +
        "distributive-identity probe scaffold) via oracle::run_row",
    },
  ],
  [
    tkey(
      "mode_boundary_invariants.rs",
      "mode_boundary_keyof_across_reexport_chain_resolves_all_keys",
    ),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::IndexedAccess)",
      semantic_queries: ["ResolveDecl", "Instantiate", "KeyOf", "ProjectPath", "LowerLocator"],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        "lifted by U2.INDEXED_ACCESS: `WantedKeys = keyof WantedType` (with " +
        "`WantedType = Foo & { a: 1 }`, `Foo` reached via the 7-hop re-export " +
        "chain) resolves the expanded literal key union `'a' | 'b'` over the real " +
        "9-file workspace, proven against the checked-in tsgo oracle snapshot " +
        "(distributive-identity probe scaffold) via oracle::run_row",
    },
  ],
  [
    tkey("union_key_access.rs", "union_key_access_keyof_self_projects_full_value_union"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::IndexedAccess)",
      semantic_queries: [
        "ResolveDecl",
        "Instantiate",
        "IndexedAccess",
        "KeyOf",
        "NormalizeUnion",
        "LowerLocator",
      ],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        "lifted by U2.INDEXED_ACCESS: `EveryMember = Surface[keyof Surface]` " +
        "projects the full member value union " +
        "`string | number | boolean | null` (the KeyofSelfIndex source-root " +
        "carve-out), proven against the checked-in tsgo oracle snapshot " +
        "(distributive-identity probe scaffold) via oracle::run_row",
    },
  ],
  [
    tkey("branded_types.rs", "branded_key_access_projects_literal_brand_tag"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::ApparentType)",
      semantic_queries: ["ResolveDecl", "Instantiate", "IndexedAccess", "LowerLocator"],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        'lifted by U2.CLASS_SURFACES-era E1 grammar: `UserId["__brand"]` reduces the string-literal index chain over the brand intersection to the literal tag `"UserId"`, proven against the checked-in tsgo oracle snapshot via oracle::run_row',
    },
  ],
  [
    tkey("branded_types.rs", "branded_key_access_projects_boolean_literal_brand_tag"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::ApparentType)",
      semantic_queries: ["ResolveDecl", "Instantiate", "IndexedAccess", "LowerLocator"],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        'lifted by U2.CLASS_SURFACES-era E1 grammar: `Cents["__cents"]` reduces the string-literal index chain over the numeric-brand intersection to the boolean literal `true`, proven against the checked-in tsgo oracle snapshot via oracle::run_row',
    },
  ],
  [
    tkey("class_features.rs", "class_features_static_inheritance_resolves_inherited_field_type"),
    {
      mech: "ClassSurfaceProjection",
      proof: "ProofRequirement::Ts7Oracle(OracleId::ClassSurface)",
      semantic_queries: [
        "ResolveDecl",
        "Instantiate",
        "TypeOf",
        "ProjectPath",
        "ResolveClassSurface",
        "LowerLocator",
      ],
      consumed_mechanisms: ["QueryValueDomainFoundation", "IndexedAccessUnionDistribution"],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `typeof StepCounter.initial` walks the static-heritage composer to the base `BaseCounter.initial: string` declared annotation, proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("class_features.rs", "class_features_static_inheritance_resolves_inherited_method_return"),
    {
      mech: "ClassSurfaceProjection",
      proof: "ProofRequirement::Ts7Oracle(OracleId::ClassSurface)",
      semantic_queries: [
        "ResolveDecl",
        "Instantiate",
        "TypeOf",
        "ProjectPath",
        "ResolveClassSurface",
        "LowerLocator",
      ],
      consumed_mechanisms: ["QueryValueDomainFoundation", "IndexedAccessUnionDistribution"],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `ReturnType<typeof StepCounter.describe>` resolves the inherited static method through the static-heritage composer and projects `string`, proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey(
      "class_features.rs",
      "class_features_static_generic_method_instantiation_projects_return_with_substitution",
    ),
    {
      mech: "ClassSurfaceProjection",
      proof: "ProofRequirement::Ts7Oracle(OracleId::ClassSurface)",
      semantic_queries: [
        "ResolveDecl",
        "Instantiate",
        "TypeOf",
        "ProjectPath",
        "ResolveClassSurface",
        "LowerLocator",
      ],
      consumed_mechanisms: ["QueryValueDomainFoundation", "IndexedAccessUnionDistribution"],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `ReturnType<typeof GenericStatic.make<string>>` lowers the instantiation-expression args on the typeof path and instantiates the static generic method to `{ wrapped: string }`, proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_return_type_of_overloaded_function_uses_last_overload",
    ),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::CallResolution)",
      semantic_queries: ["ResolveDecl", "Instantiate", "TypeOf", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `ReturnType<typeof lookup>` selects the LAST VISIBLE overload of the ordered declaration group (implementation hidden) and projects `boolean`, proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_constructor_parameters_publishes_constructor_arg_tuple",
    ),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::CallResolution)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `ConstructorParameters<Ctor>` reduces the construct signature to the labelled tuple `[id: string]` via the construct bucket, proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_instance_type_publishes_constructor_return_shape",
    ),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::CallResolution)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `InstanceType<Ctor>` materialises the construct signature's declared return object, proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_call_construct_hybrid_parameters_uses_call_signature",
    ),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::CallResolution)",
      semantic_queries: ["ResolveDecl", "Instantiate", "TypeOf", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `Parameters<typeof callable>` picks the CALL bucket of the hybrid call+construct interface (`[a: number]`), proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_call_construct_hybrid_return_type_uses_call_signature",
    ),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::CallResolution)",
      semantic_queries: ["ResolveDecl", "Instantiate", "TypeOf", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `ReturnType<typeof callable>` picks the CALL bucket of the hybrid interface and projects `string`, proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_call_construct_hybrid_constructor_parameters_uses_construct_signature",
    ),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::CallResolution)",
      semantic_queries: ["ResolveDecl", "Instantiate", "TypeOf", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `ConstructorParameters<typeof callable>` picks the CONSTRUCT bucket of the hybrid interface (`[b: string]`), proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_call_construct_hybrid_instance_type_uses_construct_signature",
    ),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::CallResolution)",
      semantic_queries: ["ResolveDecl", "Instantiate", "TypeOf", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `InstanceType<typeof callable>` picks the CONSTRUCT bucket of the hybrid interface and materialises `{ value: number }`, proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_class_method_prototype_extraction_projects_return",
    ),
    {
      mech: "ClassSurfaceProjection",
      proof: "ProofRequirement::Ts7Oracle(OracleId::CallResolution)",
      semantic_queries: [
        "ResolveDecl",
        "Instantiate",
        "TypeOf",
        "ProjectPath",
        "ResolveClassSurface",
        "LowerLocator",
      ],
      consumed_mechanisms: ["QueryValueDomainFoundation", "IndexedAccessUnionDistribution"],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `ReturnType<typeof MethodHolder.prototype.greet>` hops the synthesized `.prototype` instance projection to the method and projects `string`, proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey(
      "function_advanced.rs",
      "function_advanced_class_method_prototype_extraction_projects_parameters",
    ),
    {
      mech: "ClassSurfaceProjection",
      proof: "ProofRequirement::Ts7Oracle(OracleId::CallResolution)",
      semantic_queries: [
        "ResolveDecl",
        "Instantiate",
        "TypeOf",
        "ProjectPath",
        "ResolveClassSurface",
        "LowerLocator",
      ],
      consumed_mechanisms: ["QueryValueDomainFoundation", "IndexedAccessUnionDistribution"],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `Parameters<typeof MethodHolder.prototype.greet>` hops `.prototype` to the method and projects the labelled tuple `[name: string]`, proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("substitution_types.rs", "substitution_types_sb15_recursive_generic_substitution"),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::RelationSemantics)",
      semantic_queries: ["ResolveDecl", "Instantiate", "TypeOf", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.CLASS_SURFACES-era E1 grammar: `ReturnType<typeof sb15>` over the self-recursive generic instantiates the bare-generic declared `T` return at `unknown` (recursion is NOT a substitution event), proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_constructor_parameters_resolve_tuple"),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::RelationSemantics)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `ConstructorParameters<NumberBoxCtor>` reduces the construct signature to its parameter tuple via the construct bucket, proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("typescript_rules.rs", "typescript_rules_instance_type_resolves_constructed_object"),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::RelationSemantics)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.CLASS_SURFACES: `InstanceType<NumberBoxCtor>` materialises the construct signature's declared instance shape, proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("decorators.rs", "decorators_identity_method_decorator_preserves_return_inference"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::ClassSurface)",
      semantic_queries: ["ResolveDecl", "Instantiate", "IndexedAccess", "LowerLocator"],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        'lifted by U2.CLASS_SURFACES: `ReturnType<MethodHost["tag"]>` is decoration-invariant — the projection ignores the identity method decorator and preserves the literal `"tag"` return, proven against the checked-in tsgo oracle snapshot via oracle::run_row',
    },
  ],
  [
    tkey("decorators.rs", "decorators_metadata_reader_describe_return_is_literal_union"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::ClassSurface)",
      semantic_queries: ["ResolveDecl", "Instantiate", "IndexedAccess", "LowerLocator"],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        'lifted by U2.CLASS_SURFACES: `ReturnType<MetadataAware["describe"]>` is decoration-invariant — the metadata-reading class decorator does not rewrite the surface, so the literal union `"ready" | "pending"` survives, proven against the checked-in tsgo oracle snapshot via oracle::run_row',
    },
  ],
  [
    tkey("modern_ts_features.rs", "import_attribute_simulated_string_literal_indexed_member"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::ModuleAugmentation)",
      semantic_queries: ["ResolveDecl", "Instantiate", "IndexedAccess", "TypeOf", "LowerLocator"],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        'lifted by U2.MODULE_AUGMENTATION: `ImportedJsonConfig["name"]` reduces the string-literal index chain over the `as const` object alias to the literal `"verter-fixture"`, proven against the checked-in tsgo oracle snapshot via oracle::run_row',
    },
  ],
  [
    tkey("module_features.rs", "module_features_namespace_geometry_vector_aliases_point"),
    {
      mech: "QueryValueDomainFoundation",
      proof: "ProofRequirement::Ts7Oracle(OracleId::ModuleAugmentation)",
      semantic_queries: ["ResolveDecl", "Instantiate", "LowerLocator"],
      consumed_mechanisms: [],
      unblocker:
        "lifted by U2.MODULE_AUGMENTATION: `Geometry.Vector` (aliasing `Geometry.Point`) collapses the namespace-qualified alias chain to the underlying `{ x: number; y: number }` shape, proven against the checked-in tsgo oracle snapshot via oracle::run_row",
    },
  ],
  [
    tkey("module_features.rs", "module_features_typeof_import_named_value_resolves_to_literal"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::ModuleAugmentation)",
      semantic_queries: ["ResolveDecl", "Instantiate", "IndexedAccess", "TypeOf", "LowerLocator"],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        'lifted by U2.MODULE_AUGMENTATION: `(typeof import("./module_features_leaf"))["leafName"]` reduces the named-value typeof-import index chain to the const-narrowed literal `"leaf"`, proven against the checked-in tsgo oracle snapshot via oracle::run_row',
    },
  ],
  [
    tkey("module_features.rs", "module_features_typeof_import_default_resolves_value_shape"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::ModuleAugmentation)",
      semantic_queries: ["ResolveDecl", "Instantiate", "IndexedAccess", "TypeOf", "LowerLocator"],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        'lifted by U2.MODULE_AUGMENTATION: `(typeof import("./module_features_leaf"))["default"]` reduces the default-export typeof-import index chain to the value shape `{ tag: "leaf-default"; count: number }` (the `as const` initialiser narrows `tag`\'s value to a literal but does NOT mark the property readonly), proven against the checked-in tsgo oracle snapshot via oracle::run_row',
    },
  ],
  [
    tkey("jsx.rs", "jsx_intrinsic_via_generic_lookup_div_resolves_to_div_shape"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::JsxResolution)",
      semantic_queries: ["ResolveDecl", "Instantiate", "IndexedAccess", "LowerLocator"],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        'lifted (JSX family; re-homed to U2.INDEXED_ACCESS per measured trace): `IntrinsicPropsFor<"div">` (alias for `JSX.IntrinsicElements[Tag]`) instantiates `Tag = "div"` and reduces the indexed access over the global-augmented `JSX.IntrinsicElements` to the declared `div` shape `{ id?: string; className?: string }`, proven against the checked-in tsgo oracle snapshot via oracle::run_row',
    },
  ],
  [
    tkey("jsx.rs", "jsx_intrinsic_via_generic_lookup_span_resolves_to_span_shape"),
    {
      mech: "IndexedAccessUnionDistribution",
      proof: "ProofRequirement::Ts7Oracle(OracleId::JsxResolution)",
      semantic_queries: ["ResolveDecl", "Instantiate", "IndexedAccess", "LowerLocator"],
      consumed_mechanisms: ["QueryValueDomainFoundation"],
      unblocker:
        'lifted (JSX family; re-homed to U2.INDEXED_ACCESS per measured trace): `IntrinsicPropsFor<"span">` (alias for `JSX.IntrinsicElements[Tag]`) instantiates `Tag = "span"` and reduces the indexed access over the global-augmented `JSX.IntrinsicElements` to the declared `span` shape `{ title?: string }`, proven against the checked-in tsgo oracle snapshot via oracle::run_row',
    },
  ],
  [
    tkey("mapped_template.rs", "record_with_template_literal_key_union_projects_root_slot"),
    {
      mech: "MappedTemplateRemap",
      proof: "ProofRequirement::Ts7Oracle(OracleId::MappedTemplate)",
      semantic_queries: [
        "ResolveDecl",
        "Instantiate",
        "IndexedAccess",
        "MappedType",
        "NormalizeUnion",
        "ProjectPath",
        "TemplateLiteralReduce",
        "LowerLocator",
      ],
      consumed_mechanisms: ["QueryValueDomainFoundation", "IndexedAccessUnionDistribution"],
      unblocker:
        'lifted by U2.MAPPED_TEMPLATE: `RecordTemplateRootSlot = RecordTemplateSlots["slot:root"]` reduces the same-file string-literal index chain over the `Record<`slot:${"root"|"item"}`, …>` template keyspace to `(payload: { name: "item" | "root" }) => VNode[]`, proven against the checked-in tsgo oracle snapshot via oracle::run_row',
    },
  ],
  [
    tkey("template_literal_inference.rs", "template_literal_key_remap_capitalises_each_event_key"),
    {
      mech: "MappedTemplateRemap",
      proof: "ProofRequirement::Ts7Oracle(OracleId::TemplateLiteral)",
      semantic_queries: [
        "ResolveDecl",
        "Instantiate",
        "MappedType",
        "ProjectPath",
        "TemplateLiteralReduce",
        "LowerLocator",
      ],
      consumed_mechanisms: ["QueryValueDomainFoundation", "IndexedAccessUnionDistribution"],
      unblocker:
        'lifted by U2.MAPPED_TEMPLATE: `CounterHandlers = EventHandlers<"inc" | "dec">` expands the key-remapped mapped type `{ [K in T as `on${Capitalize<K>}`]: (payload: K) => void }` to `{ onDec: (payload: "dec") => void; onInc: (payload: "inc") => void }`, proven against the checked-in tsgo oracle snapshot via oracle::run_row',
    },
  ],
]);

function consumedMechsForBlock(blockVar) {
  // A row/block's consumed mechanisms = the dominant mechanisms of its
  // block's DIRECT prerequisites.
  return BLOCK_PREREQS.get(blockVar).map((p) => BLOCK_TO_MECHANISM.get(p));
}

// -- live `SemanticQueryName` -> its owning block_id (mirror of the Rust
//    `key_owning_block`). --
const KEY_OWNING_BLOCK = new Map([
  ["ResolveDecl", "U2QueryValueDomain"],
  ["TypeOf", "U2QueryValueDomain"],
  ["NormalizeUnion", "U2QueryValueDomain"],
  ["NormalizeIntersection", "U2QueryValueDomain"],
  // Generic substitution is a value-domain instantiation produced by
  // U2.QUERY_VALUE_DOMAIN's foundation, NOT a relation inference.
  ["Instantiate", "U2QueryValueDomain"],
  // The locator-shape body lowering the value-domain foundation demands
  // for every declaration body it instantiates.
  ["LowerLocator", "U2QueryValueDomain"],
  ["Relate", "U2RelationInfer"],
  ["Conditional", "U2RelationInfer"],
  ["IndexedAccess", "U2IndexedAccess"],
  ["KeyOf", "U2IndexedAccess"],
  ["ProjectMember", "U2IndexedAccess"],
  ["ProjectPath", "U2IndexedAccess"],
  ["MappedType", "U2MappedTemplate"],
  ["TemplateLiteralReduce", "U2MappedTemplate"],
  ["ResolveClassSurface", "U2ClassSurfaces"],
  ["ApparentType", "U2ClassSurfaces"],
  ["ResolveOverloadSet", "U2ClassSurfaces"],
  ["ResolveEnum", "U2Enums"],
  ["ResolveAmbientNamespace", "U2ModuleAugmentation"],
  ["FlowNarrowingAt", "U6FlowReturnSubstrate"],
  ["ContextualTypeAt", "U6ContextualCallback"],
  ["ResolveMacroPayload", "U14MacroAdapter"],
]);

function reaches(fromBlock, target) {
  // Is `target` == `fromBlock` or a transitive prerequisite of it?
  if (fromBlock === target) {
    return true;
  }
  const seen = new Set();
  const frontier = [fromBlock];
  while (frontier.length > 0) {
    const cur = frontier.pop();
    if (seen.has(cur)) {
      continue;
    }
    seen.add(cur);
    if (cur === target) {
      return true;
    }
    frontier.push(...(BLOCK_PREREQS.get(cur) ?? []));
  }
  return false;
}

function keysForRow(mech) {
  // The row's `semantic_queries`: the FULL set of keys its MECHANISM
  // dispatches/reads, emitted verbatim with NO per-block narrowing.
  return MECHANISM_TO_KEYS.get(mech);
}

function escapeRustStringLiteral(s) {
  return s.replaceAll("\\", "\\\\").replaceAll('"', '\\"');
}

function extractSites(source) {
  // Return `[reason, fnName]` for every literal-string `#[ignore = "..."]`
  // site in `source`.
  const sites = [];
  // Python: `source.splitlines()` (full Unicode line-boundary set).
  const lines = splitLines(source);
  for (let i = 0; i < lines.length; i++) {
    // Python `str.strip()` (NOT JS `.trim()` — see `pyStrip`): strips 0x1C–0x1F
    // and 0x85, keeps a BOM.
    const line = pyStrip(lines[i]);
    if (!line.startsWith("#[ignore")) {
      continue;
    }
    // Python `lstrip()` strips Unicode whitespace; mirror with the Python `\s`
    // class (`pyLstrip`, `u`-flagged) instead of JS `\s` (ASCII + a few points)
    // or a no-`u` `\p{...}` which would degrade to a literal char class.
    const rest = pyLstrip(line.slice("#[ignore".length));
    if (!rest.startsWith("=") || !rest.includes('"')) {
      continue;
    }
    // The capture class (`[^"\\]` / `\\.`) is class/literal-based, not
    // `\w`/`\s`/bare-`.`-dependent for Unicode semantics. `\\.`'s `.` excludes
    // `\n` in both Python (no DOTALL) and JS (no `s` flag); the only JS-extra
    // exclusions (`\r`/LS/PS) are inert here — `rest` is a single already-split,
    // already-trimmed line with no interior line terminator. Left as-is.
    const m = rest.match(/"((?:[^"\\]|\\.)*)"/);
    if (!m) {
      continue;
    }
    const reason = m[1];
    let fnName = null;
    for (let j = i + 1; j < Math.min(i + 6, lines.length); j++) {
      // Python `re` `fn\s+(\w+)` — both `\s` and `\w` are Unicode there. JS bare
      // `\s`/`\w` are not, so this builds the explicit Python `\s`/`\w` classes
      // (`PY_SPACE_SRC` / `PY_WORD_SRC`) and MUST pass `u` for their `\p{...}`
      // properties to resolve (without `u` they would be literal char classes).
      const fm = lines[j].match(new RegExp("fn" + PY_SPACE_SRC + "+(" + PY_WORD_SRC + "+)", "u"));
      if (fm) {
        fnName = fm[1];
        break;
      }
    }
    if (fnName) {
      sites.push([reason, fnName]);
    }
  }
  return sites;
}

function parsePartition(docText) {
  // Parse the §10.4.1 BEGIN/END coverage table region. Returns a Map
  // `tkey(file, function) -> [blockText, capability]`.
  const begin = "<!-- BEGIN U0 row→block coverage table";
  const end = "<!-- END U0 row→block coverage table";
  const bi = docText.indexOf(begin);
  const ei = docText.indexOf(end);
  if (bi < 0 || ei < 0) {
    throw new SystemExit(1, "could not locate §10.4.1 coverage table BEGIN/END markers");
  }
  const region = docText.slice(bi, ei);
  const out = new Map();
  let currentBlock = null;
  const blockHdr = /^\*\*`([A-Z0-9._]+)`\*\* \(\d+ rows?\):/;
  const rowRe = /^- `([a-z0-9_]+\.rs)::([A-Za-z0-9_]+)` — `([A-Za-z]+)`/;
  // Python: `region.splitlines()`. (The block/row regexes use explicit ASCII
  // literal classes that match Python's `r"..."` byte-for-byte, so only the
  // line split needs the Unicode-boundary fix.)
  for (let line of splitLines(region)) {
    // Python `str.strip()` (NOT JS `.trim()` — see `pyStrip`).
    line = pyStrip(line);
    const hm = line.match(blockHdr);
    if (hm) {
      currentBlock = hm[1];
      continue;
    }
    const rm = line.match(rowRe);
    if (rm && currentBlock !== null) {
      const file_ = rm[1];
      const fn_ = rm[2];
      const cap = rm[3];
      out.set(tkey(file_, fn_), [currentBlock, cap]);
    }
  }
  return out;
}

const GENERATED_HEADER =
  "// Auto-generated by `scripts/gen-typeinfo-ignore-manifest.mjs`\n" +
  "// (`pnpm gen:typeinfo-manifest`). DO NOT hand-edit. The §10.4.1\n" +
  "// row->block partition in `docs/arch/native-typeinfo-parity.md`\n" +
  "// is the authoritative source ONLY for each IgnoredTestRow's\n" +
  "// `block_id` (READ by the generator, joined with the live\n" +
  "// `#[ignore]` discovery + the Capability Map). This includes LIFTED\n" +
  "// rows: their `block_id` comes from §10.4.1 too — there is NO\n" +
  "// generator-side block override. The AdditionalProofRow\n" +
  "// table and the TYPEINFO_PARITY_BLOCKS DAG (each block's\n" +
  "// required_guards/verification_labels/prereqs/mechanisms) are\n" +
  "// authored in the generator's own data maps, NOT in §10.4.1.\n" +
  "// The Rust guards only diff/fail; they never write this file.\n";

function emitIgnoredRows(rows) {
  const out = [
    GENERATED_HEADER,
    "",
    "#[rustfmt::skip]",
    "const EXPECTED_IGNORE_MANIFEST: &[IgnoredTestRow] = &[",
  ];
  for (const r of rows) {
    const keys = r.keys.map((k) => `SemanticQueryName::${k}`).join(", ");
    const mechs = r.consumed.map((m) => `MechanismId::${m}`).join(", ");
    out.push(
      "    IgnoredTestRow { " +
        `file: "${r.file}", ` +
        `function: "${r.fn}", ` +
        `substrate: TargetSubstrate::${r.substrate}, ` +
        `capability: TypeInfoCapability::${r.cap}, ` +
        `organ: ArchitectureOrgan::${r.organ}, ` +
        `owning_u_block: UBlock::${r.ublock}, ` +
        `block_id: TypeInfoParityBlockId::${r.block}, ` +
        `semantic_queries: &[${keys}], ` +
        `proof: ${r.proof}, ` +
        `status: ${r.status}, ` +
        `oracle_query_ordinals: ${r.oracle_query_ordinals}, ` +
        `mechanism_id: MechanismId::${r.mech}, ` +
        `consumed_mechanisms: &[${mechs}], ` +
        `unblocker: "${r.unblocker}" },`,
    );
  }
  out.push("];");
  return out.join("\n") + "\n";
}

function emitAdditionalRows(rows) {
  const out = [
    GENERATED_HEADER,
    "",
    "#[rustfmt::skip]",
    "const ADDITIONAL_PROOF_ROWS: &[AdditionalProofRow] = &[",
  ];
  for (const r of rows) {
    const keys = r.keys.map((k) => `SemanticQueryName::${k}`).join(", ");
    const mechs = r.consumed.map((m) => `MechanismId::${m}`).join(", ");
    out.push(
      "    AdditionalProofRow { " +
        `file: "${r.file}", ` +
        `function: "${r.fn}", ` +
        `substrate: TargetSubstrate::${r.substrate}, ` +
        `capability: TypeInfoCapability::${r.cap}, ` +
        `organ: ArchitectureOrgan::${r.organ}, ` +
        `owning_u_block: UBlock::${r.ublock}, ` +
        `block_id: TypeInfoParityBlockId::${r.block}, ` +
        `semantic_queries: &[${keys}], ` +
        `proof: ${r.proof}, ` +
        `mechanism_id: MechanismId::${r.mech}, ` +
        `consumed_mechanisms: &[${mechs}] },`,
    );
  }
  out.push("];");
  return out.join("\n") + "\n";
}

function emitBlockRows() {
  const out = [
    GENERATED_HEADER,
    "",
    "#[rustfmt::skip]",
    "const TYPEINFO_PARITY_BLOCKS: &[BlockContractRow] = &[",
  ];
  const verification = BLOCK_VERIFICATION_LABELS.map((label) => `"${label}"`).join(", ");
  for (const block of BLOCK_TO_MECHANISM.keys()) {
    const prereqs = BLOCK_PREREQS.get(block)
      .map((p) => `TypeInfoParityBlockId::${p}`)
      .join(", ");
    const consumed = consumedMechsForBlock(block)
      .map((m) => `MechanismId::${m}`)
      .join(", ");
    const guards = BLOCK_TO_REQUIRED_GUARDS.get(block)
      .map((g) => `"${g}"`)
      .join(", ");
    out.push(
      "    BlockContractRow { " +
        `block_id: TypeInfoParityBlockId::${block}, ` +
        `owning_u_block: UBlock::${BLOCK_TO_UBLOCK.get(block)}, ` +
        `organ: ArchitectureOrgan::${BLOCK_TO_ORGAN.get(block)}, ` +
        `prereqs: &[${prereqs}], ` +
        `mechanism_id: MechanismId::${BLOCK_TO_MECHANISM.get(block)}, ` +
        `consumed_mechanisms: &[${consumed}], ` +
        `required_guards: &[${guards}], ` +
        `verification_labels: &[${verification}] },`,
    );
  }
  out.push("];");
  return out.join("\n") + "\n";
}

// -- The CLOSED set of 7 `AdditionalProofRow`s. All 7 rows are
//    FORWARD-DECLARATION coverage contracts emitting a RowTestGuard. --
const JSX_NO_NEW_KEY_ROWS = [
  "jsx_library_managed_attributes_via_ambient_namespace_and_indexed_access",
  "jsx_element_attributes_property_via_ambient_namespace_keyof",
  "jsx_element_children_attribute_via_ambient_namespace_keyof",
  "jsx_intrinsic_attributes_via_ambient_namespace_intersection",
  "jsx_element_class_check_via_resolve_class_surface_and_relate",
  "jsx_import_source_module_namespace_via_existing_resolution",
];
const MAPPED_COMPANION_FN =
  "mapped_modifier_minus_optional_preserves_explicit_undefined_on_required_property";

function buildAdditionalRows() {
  const rows = [];
  for (const fn of JSX_NO_NEW_KEY_ROWS) {
    const block = "U2JsxFoundations";
    const mech = mechanismForRow("JsxResolution", "jsx.rs", fn);
    rows.push({
      file: "jsx.rs",
      fn,
      substrate: "JsxResolution",
      cap: "JsxResolution",
      organ: BLOCK_TO_ORGAN.get(block),
      ublock: BLOCK_TO_UBLOCK.get(block),
      block,
      keys: keysForRow(mech),
      // FORWARD-DECLARATION coverage contract: a RowTestGuard pointing at
      // the FUTURE U2.JSX_FOUNDATIONS test fn.
      proof: `ProofRequirement::RowTestGuard { file: "jsx.rs", function: "${fn}" }`,
      mech,
      consumed: consumedMechsForBlock(block),
    });
  }
  {
    const block = "U2MappedTemplate";
    const mech = mechanismForRow("MappedTypes", "mapped_modifiers.rs", MAPPED_COMPANION_FN);
    rows.push({
      file: "mapped_modifiers.rs",
      fn: MAPPED_COMPANION_FN,
      substrate: "MappedTypes",
      cap: "MappedTypes",
      organ: BLOCK_TO_ORGAN.get(block),
      ublock: BLOCK_TO_UBLOCK.get(block),
      block,
      keys: keysForRow(mech),
      // FORWARD-DECLARATION coverage contract, consistent with the 6 JSX
      // rows: a RowTestGuard pointing at the FUTURE U2.MAPPED_TEMPLATE test fn.
      proof:
        'ProofRequirement::RowTestGuard { file: "mapped_modifiers.rs", ' +
        `function: "${MAPPED_COMPANION_FN}" }`,
      mech,
      consumed: consumedMechsForBlock(block),
    });
  }
  return rows;
}

/**
 * Element-wise comparison for `[file, fn]` pairs, replicating Python tuple
 * ordering (compare element 0, then element 1) with code-POINT string
 * comparison (not UTF-16 code-unit).
 */
function pairLess(a, b) {
  return codePointCompare(a[0], b[0]) || codePointCompare(a[1], b[1]);
}

function main(checkOnly = false) {
  const repoRoot = resolve(__dirname, "..");
  const srcDir = join(repoRoot, "crates/verter_session/src/typeinfo/typeinfo_tests");
  if (!existsSync(srcDir) || !statSync(srcDir).isDirectory()) {
    process.stderr.write(`typeinfo_tests dir missing: ${srcDir}\n`);
    return 2;
  }
  const outDir = join(repoRoot, "crates/verter_session/tests/cases/manifest_data");
  if (!checkOnly) {
    mkdirSync(outDir, { recursive: true });
  }

  const doc = readTextNormalized(join(repoRoot, "docs/arch/native-typeinfo-parity.md"));
  const partition = parsePartition(doc);

  // Discover live ignore sites + reasons.
  const discovered = new Map();
  const missingMappings = [];
  // Python: `sorted(os.listdir(src_dir))` — code-POINT order.
  for (const fn of readdirSync(srcDir).sort(codePointCompare)) {
    if (!fn.endsWith(".rs")) {
      continue;
    }
    const sites = extractSites(readTextNormalized(join(srcDir, fn)));
    if (sites.length === 0) {
      continue;
    }
    if (!FILE_TO_SUBSTRATE.has(fn)) {
      missingMappings.push(fn);
      continue;
    }
    for (const [reason, fnName] of sites) {
      discovered.set(tkey(fn, fnName), { file: fn, fn: fnName, reason });
    }
  }

  if (missingMappings.length > 0) {
    process.stderr.write("error: typeinfo-test files without a FILE_TO_SUBSTRATE mapping:\n");
    for (const fn of missingMappings) {
      process.stderr.write(`  - ${fn}\n`);
    }
    return 3;
  }

  // Cross-check discovery vs §10.4.1 partition.
  const discKeys = new Set(discovered.keys());
  const partKeys = new Set(partition.keys());
  const liftedKeys = new Set(LIFTED_ROW_OVERRIDES.keys());

  // Python `sorted(set ...)` — code-POINT order on the joined `"file fn"` keys.
  const liftedNotInPartition = [...liftedKeys]
    .filter((k) => !partKeys.has(k))
    .sort(codePointCompare);
  const liftedStillIgnored = [...liftedKeys].filter((k) => discKeys.has(k)).sort(codePointCompare);
  if (liftedNotInPartition.length > 0 || liftedStillIgnored.length > 0) {
    process.stderr.write("error: lifted-row override set is inconsistent:\n");
    for (const k of liftedNotInPartition) {
      const [f, fnn] = k.split(" ");
      process.stderr.write(`  lifted row absent from §10.4.1 partition: ${f} :: ${fnn}\n`);
    }
    for (const k of liftedStillIgnored) {
      const [f, fnn] = k.split(" ");
      process.stderr.write(`  lifted row still carries a live \`#[ignore]\`: ${f} :: ${fnn}\n`);
    }
    return 4;
  }
  const onlyDisc = [...discKeys].filter((k) => !partKeys.has(k)).sort(codePointCompare);
  const onlyPart = [...partKeys]
    .filter((k) => !discKeys.has(k) && !liftedKeys.has(k))
    .sort(codePointCompare);
  if (onlyDisc.length > 0 || onlyPart.length > 0) {
    process.stderr.write("error: §10.4.1 partition does not match the live ignore set:\n");
    for (const k of onlyDisc) {
      const [f, fnn] = k.split(" ");
      process.stderr.write(`  live-only (no partition row): ${f} :: ${fnn}\n`);
    }
    for (const k of onlyPart) {
      const [f, fnn] = k.split(" ");
      process.stderr.write(`  partition-only (no live ignore, not lifted): ${f} :: ${fnn}\n`);
    }
    return 4;
  }

  // Build the IgnoredTestRows in (file, function) sorted order.
  const allKeyPairs = [];
  const seenPair = new Set();
  for (const k of [...discKeys, ...liftedKeys]) {
    if (seenPair.has(k)) {
      continue;
    }
    seenPair.add(k);
    const sep = k.indexOf(" ");
    allKeyPairs.push([k.slice(0, sep), k.slice(sep + 1)]);
  }
  allKeyPairs.sort(pairLess);

  const rows = [];
  for (const [file_, fnName] of allKeyPairs) {
    const [blockText, cap] = partition.get(tkey(file_, fnName));
    const blockVar = BLOCK_TEXT_TO_VARIANT.get(blockText);
    const override = LIFTED_ROW_OVERRIDES.get(tkey(file_, fnName));
    let mech;
    let proof;
    let status;
    let unblocker;
    let rowKeys;
    let rowConsumed;
    if (override) {
      mech = override.mech;
      proof = override.proof;
      status = "IgnoreStatus::Lifted { block_id: TypeInfoParityBlockId::" + `${blockVar} }`;
      unblocker = escapeRustStringLiteral(override.unblocker);
      rowKeys = [...override.semantic_queries];
      rowConsumed = [...override.consumed_mechanisms];
    } else {
      mech = mechanismForRow(cap, file_, fnName);
      proof = proofForCapability(cap);
      status = "IgnoreStatus::Ignored";
      unblocker = escapeRustStringLiteral(discovered.get(tkey(file_, fnName)).reason);
      rowKeys = keysForRow(mech);
      rowConsumed = consumedMechsForBlock(blockVar);
    }
    const oracleQueryOrdinals = override ? 1 : 0;
    rows.push({
      file: file_,
      fn: fnName,
      substrate: FILE_TO_SUBSTRATE.get(file_),
      cap,
      organ: BLOCK_TO_ORGAN.get(blockVar),
      ublock: BLOCK_TO_UBLOCK.get(blockVar),
      block: blockVar,
      keys: rowKeys,
      proof,
      mech,
      consumed: rowConsumed,
      status,
      oracle_query_ordinals: oracleQueryOrdinals,
      unblocker,
    });
  }

  if (rows.length !== 362) {
    process.stderr.write(`error: expected 362 IgnoredTestRows, built ${rows.length}\n`);
    return 5;
  }

  // Unicode `\w` parity self-check (NON-circular): assert PY_WORD_SRC matches
  // CPython 3 `re.UNICODE` `\w` on the boundary set, NOT Node's looser
  // `[\p{L}\p{N}\p{Pc}]`/`\p{M}`-inclusive interpretations. A regression to the
  // connector-punctuation class (`\p{Pc}`) or a combining-mark class (`\p{M}`)
  // — the exact over-match this port replaced — flips a named case and throws
  // here, on every write AND `--check`. CPython `\w` ACCEPTS L*/N*/`_`; it
  // REJECTS the non-`_` Pc connectors (e.g. U+203F ‿) and combining marks
  // (e.g. U+0301, an Mn). The class is anchored so each probe is a whole match.
  const PY_WORD_ONE = new RegExp("^(?:" + PY_WORD_SRC + ")$", "u");
  const WORD_PARITY_CASES = [
    // [codepoint, label, mustMatch]
    [0x61, "ASCII letter 'a'", true],
    [0x35, "ASCII digit '5'", true],
    [0x5f, "underscore '_' (the sole Pc CPython treats as word)", true],
    [0xe9, "non-ASCII letter 'é' (U+00E9, category L)", true],
    [0x660, "non-ASCII digit '٠' (U+0660, category N)", true],
    [0x203f, "connector punctuation '‿' (U+203F, Pc but not '_')", false],
    [0x0301, "combining acute accent (U+0301, Mn — a \\p{M} mark)", false],
    [0x2d, "hyphen-minus '-' (U+002D, not a word char)", false],
    [0x20, "space (U+0020, not a word char)", false],
  ];
  for (const [cp, label, mustMatch] of WORD_PARITY_CASES) {
    const got = PY_WORD_ONE.test(String.fromCodePoint(cp));
    if (got !== mustMatch) {
      throw new SystemExit(
        1,
        `PY_WORD_SRC Unicode \\w parity broken: ${label} (U+` +
          cp.toString(16).toUpperCase().padStart(4, "0") +
          `) ${got ? "MATCHED" : "did NOT match"} but CPython 3 ` +
          `re.UNICODE \\w ${mustMatch ? "accepts" : "rejects"} it. ` +
          `Keep PY_WORD_SRC = [\\p{L}\\p{N}_] (literal '_'); do NOT widen to ` +
          `\\p{Pc} (over-matches connectors) or include \\p{M} (over-matches ` +
          `combining marks).`,
      );
    }
  }

  // Generation-time self-consistency assertions (NON-circular).
  for (const r of rows) {
    const owner = MECHANISM_OWNING_BLOCK.get(r.mech);
    if (owner !== r.block) {
      throw new SystemExit(
        1,
        `mechanism/block disagreement: ${r.file}::${r.fn} has ` +
          `row-level mechanism ${r.mech} owned by ${owner}, but the ` +
          `§10.4.1 partition places it in ${r.block}. Reconcile ` +
          `ROW_MECHANISM_OVERRIDE / CAPABILITY_TO_MECHANISM with the ` +
          `partition (do NOT derive mechanism from block).`,
      );
    }
    for (const k of r.keys) {
      const keyOwner = KEY_OWNING_BLOCK.get(k);
      if (keyOwner === undefined) {
        throw new SystemExit(
          1,
          `unknown semantic-query key: ${r.file}::${r.fn} ` +
            `(mechanism ${r.mech}) consumes ${k}, which has no entry ` +
            `in KEY_OWNING_BLOCK. Add ${k} to KEY_OWNING_BLOCK to match ` +
            `the live \`key_owning_block\` arms in ` +
            `typeinfo_ignored_test_manifest.rs.`,
        );
      }
      if (!reaches(r.block, keyOwner)) {
        throw new SystemExit(
          1,
          `unreachable key: ${r.file}::${r.fn} (mechanism ` +
            `${r.mech}) consumes ${k} owned by ${keyOwner}, ` +
            `not reachable from block ${r.block}. Fix MECHANISM_TO_KEYS ` +
            `or the block prereqs.`,
        );
      }
    }
  }

  const additional = buildAdditionalRows();

  // The generated artifacts, computed in memory.
  const generated = new Map([
    ["typeinfo_ignored_test_manifest_rows.rs", emitIgnoredRows(rows)],
    ["typeinfo_additional_proof_rows.rs", emitAdditionalRows(additional)],
    ["typeinfo_parity_blocks.rs", emitBlockRows()],
  ]);

  if (checkOnly) {
    const drifted = [];
    for (const [name, content] of generated) {
      const path = join(outDir, name);
      // Compare against the CRLF-normalized committed file: this makes
      // `--check` pass on a CRLF working-tree checkout while still catching
      // genuine content drift (the generator always emits `\n`).
      const committed = existsSync(path) ? readTextNormalized(path) : null;
      if (committed !== content) {
        drifted.push(name);
      }
    }
    if (drifted.length > 0) {
      process.stderr.write(
        "error: committed typeinfo manifest is STALE vs the generator " +
          "for the following file(s):\n",
      );
      for (const name of drifted) {
        process.stderr.write(`  - crates/verter_session/tests/cases/manifest_data/${name}\n`);
      }
      process.stderr.write("Regenerate with `pnpm gen:typeinfo-manifest` and commit the result.\n");
      return 6;
    }
    process.stderr.write(
      `check: ${generated.size} generated manifest file(s) match the ` +
        `regenerated output (${rows.length} IgnoredTestRows, ` +
        `${additional.length} AdditionalProofRows, ` +
        `${BLOCK_TO_MECHANISM.size} BlockContractRows)\n`,
    );
    return 0;
  }

  for (const [name, content] of generated) {
    // Explicit UTF-8 (mirrors Python `Path.write_text`, which encodes UTF-8).
    writeFileSync(join(outDir, name), content, "utf8");
  }

  process.stderr.write(
    `wrote ${rows.length} IgnoredTestRows, ${additional.length} AdditionalProofRows, ` +
      `${BLOCK_TO_MECHANISM.size} BlockContractRows\n`,
  );
  return 0;
}

function parseArgs(argv) {
  // Returns true for check-only mode. Accepts `--check` / `--verify`.
  const flags = new Set(argv.slice(2));
  const known = new Set(["--check", "--verify"]);
  // Python: `sorted(unknown)` — code-POINT order.
  const unknown = [...flags].filter((f) => !known.has(f)).sort(codePointCompare);
  if (unknown.length > 0) {
    process.stderr.write(
      `error: unknown argument(s): ${unknown.join(", ")}. ` +
        `Usage: gen-typeinfo-ignore-manifest.mjs [--check|--verify]\n`,
    );
    throw new SystemExit(2);
  }
  return flags.size > 0;
}

try {
  const checkOnly = parseArgs(process.argv);
  process.exit(main(checkOnly));
} catch (err) {
  if (err instanceof SystemExit) {
    if (err.sysexitMessage) {
      process.stderr.write(`${err.sysexitMessage}\n`);
    }
    process.exit(err.code);
  }
  throw err;
}
