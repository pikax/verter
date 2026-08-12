// Independent oracles the normalizer sits ALONGSIDE, not inside
// (conformance-normalizer.md: "Raw parse, import/export/link, execution,
// diagnostic, and mapping checks run outside the normalizer. A normalizer
// pass cannot override failure of any independent oracle.").

import { createRequire } from "node:module";

import { parseModule, canonicalize, canonicalDigest, deepEqualCanonical } from "./normalize.mjs";

/** @returns {{ ok: true, ast: object } | { ok: false, error: string }} */
export function checkParseValidity(code, label) {
  try {
    const ast = parseModule(code, label);
    return { ok: true, ast };
  } catch (error) {
    return { ok: false, error: String(error.message ?? error) };
  }
}

/**
 * Resolves every static import specifier against the REAL installed oracle
 * packages (or any other real `baseDir`) — never a mock. Bare specifiers
 * resolve through `createRequire(baseDir)`; relative specifiers are
 * declared unsupported here (generated fragments only ever import bare
 * package specifiers or other declared fragments, which is out of this
 * bounded harness's scope — see README "what's built vs deferred").
 *
 * @returns {{ ok: boolean, resolved: string[], unresolved: string[] }}
 */
export function checkLinkValidity(ast, baseDir) {
  const require = createRequire(baseDir.endsWith("/") ? baseDir : `${baseDir}/`);
  const resolved = [];
  const unresolved = [];
  for (const stmt of ast.body) {
    if (stmt.type !== "ImportDeclaration") continue;
    const specifier = stmt.source.value;
    try {
      require.resolve(specifier);
      resolved.push(specifier);
    } catch {
      unresolved.push(specifier);
    }
  }
  return { ok: unresolved.length === 0, resolved, unresolved };
}

/**
 * @returns {{
 *   equal: boolean, goldenDigest: string, candidateDigest: string,
 *   goldenRenameCount: number, candidateRenameCount: number,
 *   firstDivergence: string|null,
 * }}
 */
export function compareStructural(goldenAst, candidateAst) {
  const golden = canonicalize(goldenAst);
  const candidate = canonicalize(candidateAst);
  const equal = deepEqualCanonical(golden.tree, candidate.tree);
  return {
    equal,
    goldenDigest: canonicalDigest(golden.tree),
    candidateDigest: canonicalDigest(candidate.tree),
    goldenRenameCount: golden.renameCount,
    candidateRenameCount: candidate.renameCount,
    firstDivergence: equal ? null : firstDivergencePath(golden.tree, candidate.tree),
  };
}

/** Best-effort structural-diff pointer for failure reports — not itself an oracle. */
function firstDivergencePath(a, b, path = "$") {
  if (a === b) return null;
  if (typeof a !== typeof b || a === null || b === null) return `${path}: type/nullness differs`;
  if (Array.isArray(a) !== Array.isArray(b)) return `${path}: array-shape differs`;
  if (Array.isArray(a)) {
    if (a.length !== b.length) return `${path}: length ${a.length} vs ${b.length}`;
    for (let i = 0; i < a.length; i += 1) {
      const sub = firstDivergencePath(a[i], b[i], `${path}[${i}]`);
      if (sub) return sub;
    }
    return null;
  }
  if (typeof a === "object") {
    const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
    for (const key of keys) {
      const sub = firstDivergencePath(a[key], b[key], `${path}.${key}`);
      if (sub) return sub;
    }
    return null;
  }
  return `${path}: ${JSON.stringify(a)} vs ${JSON.stringify(b)}`;
}

/**
 * Diagnostics are compared as ordered (kind, code, line, column) tuples —
 * distinct positions/codes are never conflated, satisfying the
 * "diagnostic/mapping discrimination" self-test requirement.
 */
export function compareDiagnostics(goldenDiagnostics, candidateDiagnostics) {
  const key = (d) => `${d.kind}\0${d.code ?? ""}\0${d.start?.line ?? ""}\0${d.start?.column ?? ""}`;
  const goldenKeys = goldenDiagnostics.map(key);
  const candidateKeys = candidateDiagnostics.map(key);
  const equal =
    goldenKeys.length === candidateKeys.length &&
    goldenKeys.every((k, i) => k === candidateKeys[i]);
  return { equal, goldenKeys, candidateKeys };
}

/**
 * Full comparison report combining every independent oracle. `structural`
 * is only computed when both arms parse validly — a normalizer pass never
 * runs over, and can never mask, a parse failure.
 */
export function compareArtifacts(golden, candidate, { linkBaseDir } = {}) {
  const reasons = [];
  const goldenParse = checkParseValidity(golden.code, "golden");
  const candidateParse = checkParseValidity(candidate.code, "candidate");
  if (!goldenParse.ok) reasons.push(`golden failed to parse: ${goldenParse.error}`);
  if (!candidateParse.ok) reasons.push(`candidate failed to parse: ${candidateParse.error}`);

  let link = null;
  if (candidateParse.ok && linkBaseDir) {
    link = checkLinkValidity(candidateParse.ast, linkBaseDir);
    if (!link.ok) reasons.push(`candidate has unresolved imports: ${link.unresolved.join(", ")}`);
  }

  let structural = null;
  if (goldenParse.ok && candidateParse.ok) {
    structural = compareStructural(goldenParse.ast, candidateParse.ast);
    if (!structural.equal) reasons.push(`structural divergence at ${structural.firstDivergence}`);
  }

  const diagnostics = compareDiagnostics(golden.diagnostics ?? [], candidate.diagnostics ?? []);
  if (!diagnostics.equal) reasons.push("diagnostics diverge");

  const mapping = compareMappings(golden.map ?? null, candidate.map ?? null);
  if (!mapping.equal) reasons.push("source map diverges");

  return {
    verdict: reasons.length === 0 ? "pass" : "fail",
    reasons,
    goldenParse: { ok: goldenParse.ok, error: goldenParse.ok ? null : goldenParse.error },
    candidateParse: {
      ok: candidateParse.ok,
      error: candidateParse.ok ? null : candidateParse.error,
    },
    link,
    structural,
    diagnostics,
    mapping,
  };
}

/**
 * Mapping drift is compared on the semantically load-bearing fields only
 * (`mappings` VLQ string, `sources`, `names`) — never on incidental fields
 * like `file`. Presence itself is significant: a golden WITH a map and a
 * candidate withOUT one (or vice versa) is a divergence, not a pass.
 */
export function compareMappings(goldenMap, candidateMap) {
  if (goldenMap === null && candidateMap === null) return { equal: true, reason: "both absent" };
  if (goldenMap === null || candidateMap === null) {
    return { equal: false, reason: "map presence differs" };
  }
  const equal =
    goldenMap.mappings === candidateMap.mappings &&
    JSON.stringify(goldenMap.sources ?? []) === JSON.stringify(candidateMap.sources ?? []) &&
    JSON.stringify(goldenMap.names ?? []) === JSON.stringify(candidateMap.names ?? []);
  return { equal, reason: equal ? null : "mappings/sources/names differ" };
}
