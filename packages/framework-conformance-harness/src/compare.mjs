// Independent oracles the normalizer sits ALONGSIDE, not inside
// (conformance-normalizer.md: "Raw parse, import/export/link, execution,
// diagnostic, and mapping checks run outside the normalizer. A normalizer
// pass cannot override failure of any independent oracle.").

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { parseModule, canonicalize, canonicalDigest, deepEqualCanonical } from "./normalize.mjs";
import { VUE_DOMAIN, SVELTE_DOMAIN } from "./domain-pin.mjs";

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
 * The exact package/version identities generated conformance artifacts may
 * link against: the pinned official closures from domain-pin.mjs. Any bare
 * import outside this set is a link violation for a conformance artifact.
 */
const PINNED_PACKAGE_VERSIONS = new Map([
  ...Object.keys(VUE_DOMAIN.directPackages).map((name) => [name, VUE_DOMAIN.packageVersion]),
  ...Object.keys(SVELTE_DOMAIN.directPackages).map((name) => [name, SVELTE_DOMAIN.packageVersion]),
]);

/** Top-level package name of a bare specifier, or null for relative/URL. */
function barePackageName(specifier) {
  if (specifier.startsWith(".") || specifier.startsWith("/") || specifier.includes(":"))
    return null;
  const segments = specifier.split("/");
  return specifier.startsWith("@") ? segments.slice(0, 2).join("/") : segments[0];
}

const namespaceCache = new Map();

/**
 * Imports `specifier` exactly as an ES module located in `baseDir` would —
 * a scratch importer module is written under `baseDir` and dynamically
 * imported, so Node's real ESM resolution (export maps, conditions) applies
 * from that directory. Returns the live namespace (named exports + default
 * presence), never a mock or a static approximation.
 *
 * @returns {Promise<{ ns: object } | { error: string, kind: "unresolved"|"load-failed" }>}
 */
async function importNamespace(baseDir, specifier) {
  const key = `${baseDir}\0${specifier}`;
  if (namespaceCache.has(key)) return namespaceCache.get(key);
  const scratchDir = path.join(baseDir, ".link-scratch");
  mkdirSync(scratchDir, { recursive: true });
  const digest = createHash("sha256").update(specifier).digest("hex").slice(0, 16);
  const importerPath = path.join(scratchDir, `ns-${digest}.mjs`);
  writeFileSync(importerPath, `export * as ns from ${JSON.stringify(specifier)};\n`, "utf8");
  let result;
  try {
    const mod = await import(pathToFileURL(importerPath).href);
    result = { ns: mod.ns };
  } catch (error) {
    const message = String(error?.message ?? error);
    const notFound =
      (error?.code === "ERR_MODULE_NOT_FOUND" ||
        error?.code === "ERR_PACKAGE_PATH_NOT_EXPORTED" ||
        error?.code === "MODULE_NOT_FOUND") &&
      message.includes(specifier);
    result = { error: message, kind: notFound ? "unresolved" : "load-failed" };
  }
  namespaceCache.set(key, result);
  return result;
}

export function cleanupLinkScratch(baseDir) {
  namespaceCache.clear();
  rmSync(path.join(baseDir, ".link-scratch"), { recursive: true, force: true });
}

/** Realized (name, version) of the package a bare specifier resolves to. */
function resolvedPackageIdentity(baseDir, packageName) {
  const require = createRequire(baseDir.endsWith("/") ? baseDir : `${baseDir}/`);
  let manifestPath = null;
  try {
    manifestPath = require.resolve(`${packageName}/package.json`);
  } catch {
    try {
      let dir = path.dirname(require.resolve(packageName));
      while (dir !== path.dirname(dir)) {
        const candidate = path.join(dir, "package.json");
        try {
          const parsed = JSON.parse(readFileSync(candidate, "utf8"));
          if (parsed.name === packageName) {
            manifestPath = candidate;
            break;
          }
        } catch {
          /* keep walking up */
        }
        dir = path.dirname(dir);
      }
    } catch {
      return null;
    }
  }
  if (manifestPath === null) return null;
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  return { name: manifest.name, version: manifest.version };
}

/**
 * Full linking-surface validation against the REAL installed pinned
 * packages — never a mock. Covers, each independently reported:
 *
 *  - static import resolution (bare specifiers; relative specifiers are
 *    declared unsupported here and reported unresolved — generated
 *    conformance fragments only ever import bare package specifiers);
 *  - module-load failure (module resolves but throws while evaluating);
 *  - named imports (`import { a }`) — the name must exist on the module's
 *    real, live ESM export surface;
 *  - default imports (`import X`) — the module must expose a default
 *    binding on that surface;
 *  - namespace imports (`import * as ns`) — the module must load;
 *  - side-effect imports (`import "x"`) — the module must load;
 *  - re-export sources (`export { a } from "x"`, `export * from "x"`) —
 *    the source must resolve and load, and each named re-export must exist
 *    on its surface;
 *  - local named exports (`export { x }`) — an undeclared `x` is a module
 *    early error the PARSE oracle itself raises (acorn checks export
 *    references), so that category is caught upstream of this function;
 *  - exact-package identity — every bare import must resolve to the exact
 *    pinned package version from domain-pin.mjs; a resolvable-but-wrong
 *    version and a package outside the pinned closures are both failures.
 *
 * @returns {Promise<{
 *   ok: boolean, resolved: string[], unresolved: string[],
 *   loadFailures: string[], missingExports: string[],
 *   missingDefaults: string[],
 *   packageIdentityViolations: string[], unpinnedPackages: string[],
 * }>}
 */
export async function checkLinkValidity(ast, baseDir) {
  const resolved = [];
  const unresolved = [];
  const loadFailures = [];
  const missingExports = [];
  const missingDefaults = [];
  const packageIdentityViolations = [];
  const unpinnedPackages = [];
  const checkedPackages = new Set();

  function checkPackageIdentity(specifier) {
    const packageName = barePackageName(specifier);
    if (packageName === null || checkedPackages.has(packageName)) return;
    checkedPackages.add(packageName);
    const pinnedVersion = PINNED_PACKAGE_VERSIONS.get(packageName);
    const identity = resolvedPackageIdentity(baseDir, packageName);
    if (pinnedVersion === undefined) {
      unpinnedPackages.push(packageName);
      return;
    }
    if (identity === null) {
      packageIdentityViolations.push(`${packageName}: resolved package identity unreadable`);
      return;
    }
    if (identity.name !== packageName || identity.version !== pinnedVersion) {
      packageIdentityViolations.push(
        `${packageName}: resolved ${identity.name}@${identity.version}, pinned ${packageName}@${pinnedVersion}`,
      );
    }
  }

  async function loadSurface(specifier) {
    const result = await importNamespace(baseDir, specifier);
    if (result.ns) {
      resolved.push(specifier);
      checkPackageIdentity(specifier);
      return result.ns;
    }
    if (result.kind === "unresolved") unresolved.push(specifier);
    else loadFailures.push(`${specifier}: ${result.error}`);
    return null;
  }

  for (const stmt of ast.body) {
    if (stmt.type === "ImportDeclaration") {
      const specifier = stmt.source.value;
      const ns = await loadSurface(specifier);
      if (ns === null) continue;
      for (const spec of stmt.specifiers) {
        if (spec.type === "ImportSpecifier") {
          const name = spec.imported.name ?? spec.imported.value;
          if (!Object.prototype.hasOwnProperty.call(ns, name))
            missingExports.push(`${specifier}#${name}`);
        } else if (spec.type === "ImportDefaultSpecifier") {
          if (!Object.prototype.hasOwnProperty.call(ns, "default")) missingDefaults.push(specifier);
        }
        // ImportNamespaceSpecifier: load success above is the whole check.
      }
    } else if (stmt.type === "ExportNamedDeclaration" && stmt.source) {
      const specifier = stmt.source.value;
      const ns = await loadSurface(specifier);
      if (ns === null) continue;
      for (const spec of stmt.specifiers ?? []) {
        const name = spec.local.name ?? spec.local.value;
        if (!Object.prototype.hasOwnProperty.call(ns, name))
          missingExports.push(`${specifier}#${name}`);
      }
    } else if (stmt.type === "ExportAllDeclaration") {
      await loadSurface(stmt.source.value);
    }
  }

  return {
    ok:
      unresolved.length === 0 &&
      loadFailures.length === 0 &&
      missingExports.length === 0 &&
      missingDefaults.length === 0 &&
      packageIdentityViolations.length === 0 &&
      unpinnedPackages.length === 0,
    resolved,
    unresolved,
    loadFailures,
    missingExports,
    missingDefaults,
    packageIdentityViolations,
    unpinnedPackages,
  };
}

/**
 * @returns {{
 *   equal: boolean, goldenDigest: string, candidateDigest: string,
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

/** Canonical position: {line, column} with absent members normalized to null. */
function canonicalPosition(position) {
  if (position === null || position === undefined) return null;
  return { line: position.line ?? null, column: position.column ?? null };
}

/**
 * Full message chain, flattened in order. Accepts a plain string, an array
 * chain, or a nested `{ message, next }` chain (the TypeScript
 * DiagnosticMessageChain shape) — every link enters the comparison.
 */
function canonicalMessageChain(message) {
  if (message === null || message === undefined) return [];
  if (typeof message === "string") return [message];
  if (Array.isArray(message)) return message.flatMap(canonicalMessageChain);
  if (typeof message === "object") {
    const head = typeof message.messageText === "string" ? message.messageText : message.message;
    return [
      ...(head === undefined ? [] : [String(head)]),
      ...canonicalMessageChain(message.next ?? []),
    ];
  }
  return [String(message)];
}

function canonicalRelated(related) {
  return {
    message: canonicalMessageChain(related.message ?? related.messageText),
    source: related.source ?? related.file ?? null,
    start: canonicalPosition(related.start),
    end: canonicalPosition(related.end),
  };
}

/**
 * The canonical, fully-discriminating diagnostic record. EVERY
 * contract-observable field participates: category/kind, code, the FULL
 * message chain, source/file identity, start AND end spans, related
 * information, and (by positional array comparison) order and count.
 */
export function canonicalDiagnostic(diagnostic) {
  return {
    kind: diagnostic.kind ?? null,
    code: diagnostic.code ?? null,
    message: canonicalMessageChain(diagnostic.message),
    source: diagnostic.source ?? diagnostic.file ?? null,
    start: canonicalPosition(diagnostic.start),
    end: canonicalPosition(diagnostic.end),
    related: (diagnostic.related ?? diagnostic.relatedInformation ?? []).map(canonicalRelated),
  };
}

const DIAGNOSTIC_FIELDS = ["kind", "code", "message", "source", "start", "end", "related"];

/**
 * Ordered, full-field diagnostic comparison. Two sequences are equal only
 * when they have the same length and every diagnostic matches on every
 * canonical field at the same position. `firstMismatch` names the index and
 * the exact fields that differ (or `count` when the lengths differ), so a
 * diagnostic matching on every field but one is always caught and
 * attributable.
 */
export function compareDiagnostics(goldenDiagnostics, candidateDiagnostics) {
  const golden = goldenDiagnostics.map(canonicalDiagnostic);
  const candidate = candidateDiagnostics.map(canonicalDiagnostic);
  let firstMismatch = null;
  if (golden.length !== candidate.length) {
    firstMismatch = { index: Math.min(golden.length, candidate.length), fields: ["count"] };
  } else {
    for (let i = 0; i < golden.length; i += 1) {
      const fields = DIAGNOSTIC_FIELDS.filter(
        (field) => !deepEqualCanonical(golden[i][field], candidate[i][field]),
      );
      if (fields.length > 0) {
        firstMismatch = { index: i, fields };
        break;
      }
    }
  }
  return {
    equal: firstMismatch === null,
    firstMismatch,
    goldenCount: golden.length,
    candidateCount: candidate.length,
    golden,
    candidate,
  };
}

/**
 * Every CONTRACTUAL source-map field, classified and compared explicitly.
 * `file` is the single incidental field: it names the build environment's
 * output filename, which no framework contract observes — it is recorded
 * in the classification below precisely so its exclusion is a decision,
 * not an omission.
 */
export const CONTRACTUAL_MAP_FIELDS = [
  "version",
  "mappings",
  "sources",
  "sourceRoot",
  "sourcesContent",
  "names",
];
export const INCIDENTAL_MAP_FIELDS = ["file"];

/**
 * Mapping comparison over every contractual field independently. Presence
 * itself is significant: a golden WITH a map and a candidate withOUT one
 * (or vice versa) is a divergence, not a pass. Returns per-field equality
 * so a map matching on every field but one is caught and attributable.
 */
export function compareMappings(goldenMap, candidateMap) {
  if (goldenMap === null && candidateMap === null)
    return { equal: true, reason: "both absent", fields: null };
  if (goldenMap === null || candidateMap === null)
    return { equal: false, reason: "map presence differs", fields: null };
  const fields = {};
  for (const field of CONTRACTUAL_MAP_FIELDS) {
    const goldenValue = goldenMap[field] === undefined ? null : goldenMap[field];
    const candidateValue = candidateMap[field] === undefined ? null : candidateMap[field];
    fields[field] = deepEqualCanonical(goldenValue, candidateValue);
  }
  const differing = CONTRACTUAL_MAP_FIELDS.filter((field) => !fields[field]);
  return {
    equal: differing.length === 0,
    reason: differing.length === 0 ? null : `map fields differ: ${differing.join(", ")}`,
    fields,
  };
}

/**
 * Full comparison report combining every independent oracle. `structural`
 * is only computed when both arms parse validly — a normalizer pass never
 * runs over, and can never mask, a parse failure.
 */
export async function compareArtifacts(golden, candidate, { linkBaseDir } = {}) {
  const reasons = [];
  const goldenParse = checkParseValidity(golden.code, "golden");
  const candidateParse = checkParseValidity(candidate.code, "candidate");
  if (!goldenParse.ok) reasons.push(`golden failed to parse: ${goldenParse.error}`);
  if (!candidateParse.ok) reasons.push(`candidate failed to parse: ${candidateParse.error}`);

  let link = null;
  if (candidateParse.ok && linkBaseDir) {
    link = await checkLinkValidity(candidateParse.ast, linkBaseDir);
    if (link.unresolved.length > 0)
      reasons.push(`candidate has unresolved imports: ${link.unresolved.join(", ")}`);
    if (link.loadFailures.length > 0)
      reasons.push(`candidate imports fail to load: ${link.loadFailures.join("; ")}`);
    if (link.missingExports.length > 0)
      reasons.push(`candidate imports missing named exports: ${link.missingExports.join(", ")}`);
    if (link.missingDefaults.length > 0)
      reasons.push(
        `candidate default-imports modules without a default export: ${link.missingDefaults.join(", ")}`,
      );
    if (link.packageIdentityViolations.length > 0)
      reasons.push(
        `candidate resolves wrong package identities: ${link.packageIdentityViolations.join("; ")}`,
      );
    if (link.unpinnedPackages.length > 0)
      reasons.push(
        `candidate imports packages outside the pinned closures: ${link.unpinnedPackages.join(", ")}`,
      );
  }

  let structural = null;
  if (goldenParse.ok && candidateParse.ok) {
    structural = compareStructural(goldenParse.ast, candidateParse.ast);
    if (!structural.equal) reasons.push(`structural divergence at ${structural.firstDivergence}`);
  }

  const diagnostics = compareDiagnostics(golden.diagnostics ?? [], candidate.diagnostics ?? []);
  if (!diagnostics.equal) {
    reasons.push(
      `diagnostics diverge (index ${diagnostics.firstMismatch.index}: ${diagnostics.firstMismatch.fields.join(", ")})`,
    );
  }

  const mapping = compareMappings(golden.map ?? null, candidate.map ?? null);
  if (!mapping.equal) reasons.push(`source map diverges: ${mapping.reason}`);

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
