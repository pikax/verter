// The independent JavaScript reference implementation of the assembled Vue
// main-module source-map composition algebra.
//
// WHAT THIS IS. AMD-008 §2 item 2's mandated artifact: an independent,
// input-only, cross-language reference that computes the expected map artifact
// FROM INPUTS ALONE. It is an ORACLE, not a second production assembler — it is
// never supplied to BF2 as a candidate map. Its only consumer is an acceptance
// harness that also runs production's real assembler and compares the two
// decoded artifacts for exact equality, field for field and position for
// position, including the exact ORDERED sequence of segments.
//
// WHAT IT IS BUILT FROM. Exactly one authority: the FROZEN layer-1 semantic
// specification at `spec/assembled-map-composition-layer1.md`. Every section
// reference below is to that document. It carries NO dependency — import, FFI,
// generated binding, or fixture produced by — on Rust composition, rewrite,
// placement, or map-emission code, and it is not a translation of the
// production implementation, which is developed independently and concurrently
// from the same frozen specification. That independence is what makes the later
// equality comparison evidence rather than two copies of one bug.
//
// MODULE MAP (each file names the sections it implements):
//   assembled-map-coordinates.mjs    §2.1–§2.3, §2.6, §5.2
//   assembled-map-json.mjs           §4.3 step 1.1–1.2, §4.5
//   assembled-map-wire.mjs           §4.3 step 1.21, §7.6
//   assembled-map-validate.mjs       §4.1–§4.5
//   assembled-map-rewrite.mjs        §2.4–§2.5, §5.1, §5.3–§5.5, §5.7
//   assembled-map-write-grammar.mjs  §6.1–§6.3
//   this file                        §3, §4 orchestration, §5.8, §6.4, §7, §8

import { lineTable } from "./assembled-map-coordinates.mjs";
import { runScriptRewritePasses } from "./assembled-map-rewrite.mjs";
import { checkSourceRootAgreement, validateContributingMap } from "./assembled-map-validate.mjs";
import { encodeMappings } from "./assembled-map-wire.mjs";
import { assembleModule, placeSegment } from "./assembled-map-write-grammar.mjs";

/** §8 — the three provenance origins. Never serialized: no member of §7.1 carries one. */
export const ORIGIN_SCRIPT = "Script";
export const ORIGIN_TEMPLATE = "Template";
export const ORIGIN_ASSEMBLY_BOUNDARY = "AssemblyBoundary";

/**
 * §11.4 — a DTO instance that violates §3.3's schema or §3.5's precondition P1
 * is OUT OF LAYER-1 SCOPE and gets no `UncomposableInputMap` family. It is a
 * defective vector, caught at suite load, not a composition outcome — so it is
 * raised as a distinct error rather than modelled as either fail-closed
 * outcome kind of §4.2.
 */
export class MalformedAssembleInputError extends Error {}

// ---------------------------------------------------------------------------
// §3 — the pre-assembly input DTO
// ---------------------------------------------------------------------------

const ASSEMBLE_INPUT_FIELDS = [
  "canonicalId",
  "styleCount",
  "customBlockCount",
  "styleLangs",
  "customTypes",
  "script",
  "template",
  "scopeId",
  "runtimeModuleName",
  "isProduction",
  "ssr",
  "ssrModuleId",
  "emitSsrModuleRegistration",
  "hmrStrategy",
  "sourceMapRequested",
  "authored",
];
const SCRIPT_FRAGMENT_FIELDS = ["code", "sourceMap"];
const TEMPLATE_FRAGMENT_FIELDS = ["code", "imports", "ssrImports", "sourceMap"];
const AUTHORED_FIELDS = ["script", "template"];
const HMR_STRATEGIES = ["vite", "webpack", "none"];

const UINT32_MAX = 2 ** 32 - 1;

function malformed(message) {
  throw new MalformedAssembleInputError(message);
}

function assertExactFields(value, fields, where) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    malformed(`${where} must be an object`);
  }
  const present = Object.keys(value);
  for (const field of fields) {
    if (!Object.hasOwn(value, field)) malformed(`${where} is missing the member \`${field}\``);
  }
  for (const field of present) {
    if (!fields.includes(field)) malformed(`${where} carries the extra member \`${field}\``);
  }
}

function assertString(value, where) {
  if (typeof value !== "string") malformed(`${where} must be a string`);
}

function assertBoolean(value, where) {
  if (typeof value !== "boolean") malformed(`${where} must be a boolean`);
}

function assertUint32(value, where) {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 0 || value > UINT32_MAX) {
    malformed(`${where} must be a uint32`);
  }
}

function assertStringArray(value, where, allowNull) {
  if (!Array.isArray(value)) malformed(`${where} must be an array`);
  for (const element of value) {
    if (typeof element === "string") continue;
    if (allowNull && element === null) continue;
    malformed(`${where} carries a non-string element`);
  }
}

/** §3.5 — precondition P1: printable ASCII, `U+0020`–`U+007E`. */
function assertPrintableAscii(value, where) {
  for (let i = 0; i < value.length; i += 1) {
    const code = value.charCodeAt(i);
    if (code < 0x20 || code > 0x7e) {
      malformed(`${where} violates precondition P1 (§3.5): only U+0020–U+007E are specified`);
    }
  }
}

/**
 * §3.3 — the DTO schema, and §3.5 — precondition P1. "Nothing else is
 * admissible": exact field lists, no extra and no missing member.
 */
export function assertValidAssembleInput(input) {
  assertExactFields(input, ASSEMBLE_INPUT_FIELDS, "AssembleInput");

  assertString(input.canonicalId, "canonicalId");
  assertUint32(input.styleCount, "styleCount");
  assertUint32(input.customBlockCount, "customBlockCount");
  assertStringArray(input.styleLangs, "styleLangs", true);
  assertStringArray(input.customTypes, "customTypes", false);

  if (input.script !== null) {
    assertExactFields(input.script, SCRIPT_FRAGMENT_FIELDS, "script");
    assertString(input.script.code, "script.code");
    assertString(input.script.sourceMap, "script.sourceMap");
  }
  if (input.template !== null) {
    assertExactFields(input.template, TEMPLATE_FRAGMENT_FIELDS, "template");
    assertString(input.template.code, "template.code");
    assertStringArray(input.template.imports, "template.imports", false);
    assertStringArray(input.template.ssrImports, "template.ssrImports", false);
    assertString(input.template.sourceMap, "template.sourceMap");
  }

  assertString(input.scopeId, "scopeId");
  if (input.runtimeModuleName !== null) assertString(input.runtimeModuleName, "runtimeModuleName");
  assertBoolean(input.isProduction, "isProduction");
  assertBoolean(input.ssr, "ssr");
  if (input.ssrModuleId !== null) assertString(input.ssrModuleId, "ssrModuleId");
  assertBoolean(input.emitSsrModuleRegistration, "emitSsrModuleRegistration");
  if (!HMR_STRATEGIES.includes(input.hmrStrategy)) {
    malformed('hmrStrategy must be one of "vite", "webpack", "none"');
  }
  assertBoolean(input.sourceMapRequested, "sourceMapRequested");
  assertExactFields(input.authored, AUTHORED_FIELDS, "authored");
  assertBoolean(input.authored.script, "authored.script");
  assertBoolean(input.authored.template, "authored.template");

  // §3.5 — the six embedded strings.
  assertPrintableAscii(input.canonicalId, "canonicalId");
  assertPrintableAscii(input.scopeId, "scopeId");
  if (input.runtimeModuleName !== null) {
    assertPrintableAscii(input.runtimeModuleName, "runtimeModuleName");
  }
  if (input.ssrModuleId !== null) assertPrintableAscii(input.ssrModuleId, "ssrModuleId");
  for (const lang of input.styleLangs) {
    if (lang !== null) assertPrintableAscii(lang, "styleLangs[i]");
  }
  for (const type of input.customTypes) assertPrintableAscii(type, "customTypes[i]");
}

/**
 * §3.4 — map requiredness.
 *
 * > A fragment's map is REQUIRED iff the fragment is both AUTHORED and PRESENT.
 * > A required map is present iff its `sourceMap` string is non-empty.
 *
 * Present-but-not-authored (a compiler-synthesised script block) is not
 * required to carry a map; authored-but-not-present (the inline topology)
 * requires nothing, because the fragment contributes no bytes.
 */
function mapIsRequired(fragment, authored) {
  return authored && fragment !== null;
}

// ---------------------------------------------------------------------------
// §4 / §5 / §6 / §7 — the composition
// ---------------------------------------------------------------------------

/**
 * The mandated oracle entry point: computes the assembled module's code and,
 * when a map is requested, the complete decoded map artifact — from ONE
 * `AssembleInput` and nothing else.
 *
 * @param {object} input an `AssembleInput` matching §3.3 exactly
 * @returns {
 *     { outcome: "composed", code: string, map: object|null,
 *       segments: object[]|null, provenance: string[]|null }
 *   | { outcome: "MissingRequiredInputMap", fragment: "script"|"template" }
 *   | { outcome: "UncomposableInputMap", family: string, code: string,
 *       fragment: "script"|"template" }
 *   }
 * @throws {MalformedAssembleInputError} §11.4 — a schema- or P1-invalid DTO
 */
export function composeAssembledVueMainModule(input) {
  assertValidAssembleInput(input);

  // ---- Stage 0.1 — request ------------------------------------------------
  // "If `sourceMapRequested` is `false`: composition is not performed; the
  // result carries code and no map (§7.7). No further check runs." A non-empty
  // `sourceMap` string is ignored (§3.4). The passes still run: they determine
  // the module's BYTES, and the code baseline is pinned regardless of any map
  // (§5.8).
  if (!input.sourceMapRequested) {
    const rewritten =
      input.script === null
        ? null
        : runScriptRewritePasses(input.script.code, null, ORIGIN_SCRIPT).code;
    return {
      outcome: "composed",
      code: assembleModule(input, rewritten).code,
      // §7.7: "the result carries NO map — not an empty map, not a map with an
      // empty `mappings`". Asserted positively, never by omitting the check.
      map: null,
      segments: null,
      provenance: null,
    };
  }

  // ---- Stage 0.2 / 0.3 — inventory ---------------------------------------
  if (mapIsRequired(input.script, input.authored.script) && input.script.sourceMap === "") {
    return { outcome: "MissingRequiredInputMap", fragment: "script" };
  }
  if (mapIsRequired(input.template, input.authored.template) && input.template.sourceMap === "") {
    return { outcome: "MissingRequiredInputMap", fragment: "template" };
  }

  // ---- Stage 1 — per contributing map, script THEN template ---------------
  // §5.8: a "contributing map" is a fragment that is present and carries a
  // non-empty `sourceMap` which passed §4's validation. A present fragment with
  // an empty `sourceMap` string is not one: its code is still written, and for
  // the script still rewritten by both passes, but it contributes NOTHING to
  // the assembled map — no carried segments, no replacement segments, no resume
  // segments, no table rows, no ignore-list entries, and no BR-3 boundary
  // segment.
  const contributing = [];
  for (const fragment of ["script", "template"]) {
    const value = input[fragment];
    if (value === null || value.sourceMap === "") continue;
    const validated = validateContributingMap(value.sourceMap, value.code);
    if (!validated.ok) {
      return {
        outcome: "UncomposableInputMap",
        family: validated.family,
        code: validated.code,
        fragment,
      };
    }
    contributing.push({ fragment, map: validated.map });
  }

  // ---- Stage 2 — across the contributing maps -----------------------------
  const agreement = checkSourceRootAgreement(contributing);
  if (!agreement.ok) {
    // §4.3 stage 2 runs over the contributing SET; the conflict is reported
    // against the LATER contributing map in fixed script-then-template order
    // — layer 1's `DECISION` D-8 (§4.3 step 2.1). Under the current
    // two-fragment DTO this is always the template; D-8 does not generalise
    // to a hypothetical third fragment.
    return {
      outcome: "UncomposableInputMap",
      family: agreement.family,
      code: agreement.code,
      fragment: contributing[contributing.length - 1].fragment,
    };
  }

  // Validation has run to completion. Only now does any composition work begin
  // (§4.1): no segment was translated, no table row appended, no artifact field
  // computed before this point.
  const contributingByFragment = new Map(contributing.map((entry) => [entry.fragment, entry.map]));

  // ---- §5 — chain each fragment through its rewrite passes ----------------
  let rewrittenScriptCode = null;
  let chainedScriptSegments = null;
  if (input.script !== null) {
    const scriptMap = contributingByFragment.get("script") ?? null;
    const tagged =
      scriptMap === null
        ? null
        : scriptMap.segments.map((segment) => ({ ...segment, origin: ORIGIN_SCRIPT }));
    const passed = runScriptRewritePasses(input.script.code, tagged, ORIGIN_SCRIPT);
    rewrittenScriptCode = passed.code;
    chainedScriptSegments = passed.segments;
  }

  // §5.7 — neither pass applies to the template fragment: its code is written
  // verbatim and its map is PLACED DIRECTLY, with no chain step.
  const templateMap = contributingByFragment.get("template") ?? null;
  const templateSegments =
    templateMap === null
      ? null
      : templateMap.segments.map((segment) => ({ ...segment, origin: ORIGIN_TEMPLATE }));

  // ---- §6.2 / §6.3 — write the module and derive placement ----------------
  const assembled = assembleModule(input, rewrittenScriptCode);

  // ---- §7.4 — tables: stable append in contribution order, NO dedup -------
  const sources = [];
  const names = [];
  const sourcesContent = [];
  const ignoreList = [];
  const baseOffsets = new Map();
  for (const { fragment, map } of contributing) {
    baseOffsets.set(fragment, { sources: sources.length, names: names.length });
    for (let i = 0; i < map.sources.length; i += 1) {
      sources.push(map.sources[i]);
      const declared = map.sourcesContent;
      const entry = declared !== null && typeof declared[i] === "string" ? declared[i] : null;
      sourcesContent.push(entry);
    }
    for (const name of map.names) names.push(name);
  }
  // §7.3 — carried and remapped (`DECISION` D-4), each shifted by that
  // fragment's source-table base offset, in contribution order and, within a
  // fragment, in the fragment's declared order.
  for (const { fragment, map } of contributing) {
    if (map.ignoreList === null) continue;
    const base = baseOffsets.get(fragment).sources;
    for (const entry of map.ignoreList) ignoreList.push(entry + base);
  }

  // ---- §6.3 placement, §7.4 index remap, §6.4 BR-3 ------------------------
  const assembledSegments = [];

  const emitFragment = (fragment, segments, placement, finalCode) => {
    if (segments === null) return; // not a contributing map (§5.8)
    const base = baseOffsets.get(fragment);
    for (const segment of segments) {
      const placed = placeSegment(segment, placement);
      assembledSegments.push({
        genLine: placed.genLine,
        genCol: placed.genCol,
        srcIdx: placed.srcIdx === null ? null : placed.srcIdx + base.sources,
        srcLine: placed.srcLine,
        srcCol: placed.srcCol,
        nameIdx: placed.nameIdx === null ? null : placed.nameIdx + base.names,
        origin: placed.origin,
      });
    }
    // BR-3 — the fragment-end boundary segment. "Emit one SOURCELESS segment
    // iff the fragment's final code ENDS WITH LF — equivalently, iff that
    // fragment's newline patch (W-07 / W-12) does NOT fire." It is NOT "the end
    // cursor column is zero": those predicates disagree on an empty present
    // fragment (§6.4 case 4′), where firing would shadow the fragment's own
    // carried segment at the same coordinate and make a faithfully composed
    // authored position unobservable.
    if (!finalCode.endsWith("\n")) return;
    // `bl` is the module line of the fragment's trailing empty line.
    const boundaryLine = placement.lineOffset + lineTable(finalCode).length - 1;
    // §5.5 rule 6 — emitted AFTER every placed segment of the fragment it
    // bounds, so it wins the `resolveAt` tie at its own coordinate.
    assembledSegments.push({
      genLine: boundaryLine,
      genCol: 0,
      srcIdx: null,
      srcLine: null,
      srcCol: null,
      nameIdx: null,
      origin: ORIGIN_ASSEMBLY_BOUNDARY,
    });
  };

  // §5.5 rule 5 — the assembled sequence is the concatenation in ASSEMBLY WRITE
  // ORDER: every placed script segment precedes every placed template segment.
  if (input.script !== null && chainedScriptSegments !== null) {
    emitFragment("script", chainedScriptSegments, assembled.scriptPlacement, rewrittenScriptCode);
  }
  if (input.template !== null && templateSegments !== null) {
    emitFragment("template", templateSegments, assembled.templatePlacement, input.template.code);
  }

  // ---- §7.1 / §7.2 — the artifact -----------------------------------------
  // `file`, `debugId` and unknown/extension members are DROPPED, never
  // inherited: metadata describing the GENERATED document is dropped because
  // the document it described no longer exists.
  const map = { version: 3 };
  if (agreement.sourceRoot.present) map.sourceRoot = agreement.sourceRoot.value;
  map.names = names;
  map.sources = sources;
  // §7.4 — present iff at least one entry is non-null.
  if (sourcesContent.some((entry) => entry !== null)) map.sourcesContent = sourcesContent;
  // §7.3 — present iff the resulting list is non-empty. The COMPARED artifact's
  // member is the logical name `ignoreList`; `x_google_ignoreList` is a wire
  // key, not a second field (§7.8).
  if (ignoreList.length > 0) map.ignoreList = ignoreList;
  map.mappings = encodeMappings(assembledSegments);

  return {
    outcome: "composed",
    code: assembled.code,
    map,
    // §7.1 — the compared artifact is the decoded object "together with the
    // decoded segment sequence of `mappings`". Surfaced here so a comparator
    // never has to re-decode, and so the ordered sequence is directly
    // assertable. §8 — the provenance tag is NEVER serialized: it rides
    // alongside, on no member of §7.1.
    segments: assembledSegments.map(({ origin, ...segment }) => segment),
    provenance: assembledSegments.map((segment) => segment.origin),
  };
}

/**
 * §5 — the chain algebra for the script fragment's two passes, exposed at the
 * level §9's CHAIN-SCOPED vectors (V1–V3, V5–V7) assert: one fragment, no
 * assembly writes, no placement, no BR-3.
 *
 * The MANDATED oracle interface is `composeAssembledVueMainModule`, which
 * consumes the §3.3 DTO. This export is the same algebra observed one layer in,
 * and exists so those vectors can be reproduced at the level they were authored
 * at rather than through an assembly they do not model.
 */
export function chainScriptFragment(code, segments) {
  const tagged = segments.map((segment) => ({ ...segment, origin: ORIGIN_SCRIPT }));
  const passed = runScriptRewritePasses(code, tagged, ORIGIN_SCRIPT);
  return {
    code: passed.code,
    segments: passed.segments.map(({ origin, ...segment }) => segment),
    provenance: passed.segments.map((segment) => segment.origin),
  };
}
