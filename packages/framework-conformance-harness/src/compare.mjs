// Independent oracles the normalizer sits alongside, not inside
// (conformance-normalizer.md: raw parse, import/export/link, execution,
// diagnostic, and mapping checks run outside the normalizer. A normalizer
// pass cannot override failure of any independent oracle).

import { createHash, randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { parseModule, canonicalize, canonicalDigest, deepEqualCanonical } from "./normalize.mjs";
import { VUE_DOMAIN, SVELTE_DOMAIN } from "./domain-pin.mjs";
import { validateAuthoredMapping } from "./mapping-oracle.mjs";

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
 * Exact package/version identities generated conformance artifacts may
 * link against (pinned official closures from domain-pin.mjs). Any bare
 * import outside this set is a link violation.
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

// `baseDir` (the shared per-framework oracle install, e.g.
// `.oracle-installs/vue`) is the SAME directory across every concurrent
// process that links against that oracle — one `node
// bin/check-candidate.mjs` invocation per test, all sharing the install.
// The scratch importer must therefore be unique PER PROCESS, not merely
// per imported specifier: two processes importing the same specifier
// concurrently must never write the same file, and one process's cleanup
// must never delete a sibling process's still-in-use files. Minted once
// per process at module load, never reused across a process boundary.
const PROCESS_SCRATCH_ID = `${process.pid}-${randomUUID()}`;

/**
 * Import `specifier` as an ES module located in `baseDir` would: a scratch
 * importer under `baseDir` so Node's real ESM resolution applies. Returns
 * the live namespace, never a mock.
 *
 * @returns {Promise<{ ns: object } | { error: string, kind: "unresolved"|"load-failed" }>}
 */
async function importNamespace(baseDir, specifier) {
  const key = `${baseDir}\0${specifier}`;
  if (namespaceCache.has(key)) return namespaceCache.get(key);
  const scratchDir = path.join(baseDir, ".link-scratch", PROCESS_SCRATCH_ID);
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
  // Remove only THIS process's own scratch subdirectory — never the shared
  // `.link-scratch` parent, which sibling processes linking against the
  // same oracle install may still be writing under concurrently.
  rmSync(path.join(baseDir, ".link-scratch", PROCESS_SCRATCH_ID), {
    recursive: true,
    force: true,
  });
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
 * Linking-surface validation against the real installed pinned packages —
 * never a mock. Independently reported:
 *
 *  - static import resolution (bare specifiers; relatives are unsupported
 *    here and reported unresolved)
 *  - module-load failure (resolves but throws while evaluating)
 *  - named imports — name must exist on the live ESM export surface
 *  - default imports — module must expose a default binding
 *  - namespace / side-effect imports — module must load
 *  - re-export sources — source must resolve/load; named re-exports must
 *    exist on its surface
 *  - local named exports (`export { x }`) — undeclared `x` is a parse-oracle
 *    early error (acorn checks export references), caught upstream
 *  - exact-package identity — every bare import must resolve to the pinned
 *    version from domain-pin.mjs
 *
 * `specifierOverrides` redirects the export-surface load of a bare
 * specifier to a different entry of the same pinned install (vapor-backend
 * `vue` → with-vapor runtime build; Vue publishes vapor exports only in
 * ESM browser/bundler builds). Exact-package-identity still runs against
 * the original bare specifier.
 *
 * @returns {Promise<{
 *   ok: boolean, resolved: string[], unresolved: string[],
 *   loadFailures: string[], missingExports: string[],
 *   missingDefaults: string[],
 *   packageIdentityViolations: string[], unpinnedPackages: string[],
 * }>}
 */
export async function checkLinkValidity(ast, baseDir, { specifierOverrides } = {}) {
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
    const target = specifierOverrides?.get(specifier) ?? specifier;
    const result = await importNamespace(baseDir, target);
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
 * Full comparison report combining every independent oracle. `structural`
 * is only computed when both arms parse validly — a normalizer pass never
 * runs over, and can never mask, a parse failure.
 *
 * `axes` records, per independent oracle, whether it genuinely RAN or was
 * SKIPPED (with the reason). Default behavior is unchanged: a skipped axis
 * is informational. Under `authoritative: true` — the fail-closed mode a
 * consumer opts into to prove every axis genuinely executed — any skipped
 * axis becomes a hard failure reason instead of a silent narrowing.
 */
export async function compareArtifacts(
  golden,
  candidate,
  { linkBaseDir, authoritative, linkSpecifierOverrides, mappingContext } = {},
) {
  const reasons = [];
  const axes = {
    parse: { status: "ran", reason: null },
    link: { status: "ran", reason: null },
    structural: { status: "ran", reason: null },
    diagnostics: { status: "ran", reason: null },
    mapping: { status: "ran", reason: null },
  };
  const goldenParse = checkParseValidity(golden.code, "golden");
  const candidateParse = checkParseValidity(candidate.code, "candidate");
  if (!goldenParse.ok) reasons.push(`golden failed to parse: ${goldenParse.error}`);
  if (!candidateParse.ok) reasons.push(`candidate failed to parse: ${candidateParse.error}`);

  let link = null;
  if (!linkBaseDir) {
    axes.link = { status: "skipped", reason: "no linkBaseDir supplied" };
  } else if (!candidateParse.ok) {
    axes.link = { status: "skipped", reason: "candidate failed to parse" };
  }
  if (candidateParse.ok && linkBaseDir) {
    link = await checkLinkValidity(candidateParse.ast, linkBaseDir, {
      specifierOverrides: linkSpecifierOverrides,
    });
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
  } else {
    axes.structural = { status: "skipped", reason: "an arm failed to parse" };
  }

  const diagnostics = compareDiagnostics(golden.diagnostics ?? [], candidate.diagnostics ?? []);
  if (!diagnostics.equal) {
    reasons.push(
      `diagnostics diverge (index ${diagnostics.firstMismatch.index}: ${diagnostics.firstMismatch.fields.join(", ")})`,
    );
  }

  // The mapping axis is SELF-REFERENTIAL: the candidate's map is validated
  // against the candidate's OWN generated code and the authored fixture, and
  // the golden's map is not an input. See mapping-oracle.mjs for why a
  // candidate-vs-official map comparison cannot be the oracle here.
  let mapping = null;
  if (!mappingContext) {
    axes.mapping = {
      status: "skipped",
      reason: "no authored-source mapping context supplied",
    };
  } else {
    mapping = validateAuthoredMapping({
      ...mappingContext,
      code: candidate.code ?? null,
      map: candidate.map ?? null,
    });
    if (!mapping.ok) {
      reasons.push(
        `candidate source map is not truthful about its own output: ${mapping.violations
          .map((violation) => `${violation.rule} — ${violation.detail}`)
          .join("; ")}`,
      );
    }
  }

  if (authoritative) {
    for (const [axis, state] of Object.entries(axes)) {
      if (state.status === "skipped") {
        reasons.push(`authoritative mode: ${axis} axis skipped (${state.reason})`);
      }
    }
  }

  return {
    verdict: reasons.length === 0 ? "pass" : "fail",
    reasons,
    axes,
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
