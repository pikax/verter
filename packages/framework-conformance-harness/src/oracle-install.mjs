// Isolated per-domain oracle installations — the sole source official
// compilers and runtimes are loaded from.
//
// Domain is defined by committed evidence locks
// (`oracles/{vue,svelte}/package-lock.json`), never workspace resolution:
// a workspace store can resolve the oracle's transitive deps (Svelte's
// `acorn` / `@sveltejs/acorn-typescript`, Vue's `postcss`, …) to versions
// that drift from the committed closure. Each domain is an `npm ci` of the
// committed lock into `.oracle-installs/<framework>`. Every load proves
// the exact realized closure before any oracle module is evaluated:
//
//   1. Static evidence (package-pin layers 2–5): lock byte digest,
//      lock-content vs domain-pin, closure.tsv digest, independent
//      re-derivation. Mutated evidence refuses before any install work.
//   2. Realization: offline-only `npm ci --offline --ignore-scripts` from
//      the committed lock, sourced only from `.oracle-npm-cache`. Missing
//      cache throws `OracleCacheUnprovisionedError` — never a networked
//      fallback. Re-run only when the copied lock no longer byte-matches
//      or validation fails. Realize-then-swap is serialized by an exclusive
//      mkdir lock, stages, validates the stage, then atomically renames —
//      no concurrent load sees a half-written tree.
//   3. Realized-closure validation (closure-verify): physical tree
//      enumerated (real manifests, re-resolved edges) vs lock-derived
//      closure; plus layer 1 (direct packages resolve to the pinned
//      version from the isolated install).
//   4. Content-drift refusal: each realization records per-package content
//      digests; later loads throw before any oracle module is resolved
//      when live digests deviate. Drift is never silently re-realized.
//   5. Entry-point loadability: every direct oracle package's declared
//      entry must resolve (no evaluation) from the realized tree.
//
// Only then do `oracleRequire` / `importOracleModule` hand out a module
// dynamically from the isolated install. No production path holds a static
// top-level import of an oracle package.

import { execFileSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { VUE_DOMAIN, SVELTE_DOMAIN, EVIDENCE_LOCK_DIGESTS } from "./domain-pin.mjs";
import {
  assertEvidenceStaticPinned,
  assertPackagesPinned,
  PackageDriftError,
} from "./package-pin.mjs";
import {
  compareRealizedToLock,
  enumerateInstalledClosure,
  enumerateLockClosure,
} from "./closure-verify.mjs";
import {
  HARNESS_ROOT,
  VUE_EVIDENCE_LOCK,
  VUE_EVIDENCE_CLOSURE,
  SVELTE_EVIDENCE_LOCK,
  SVELTE_EVIDENCE_CLOSURE,
} from "./paths.mjs";

export const ORACLE_INSTALLS_ROOT = process.env.BF2_ORACLE_INSTALLS
  ? path.resolve(process.env.BF2_ORACLE_INSTALLS)
  : path.join(HARNESS_ROOT, ".oracle-installs");

const ORACLE_NPM_CACHE_ROOT = process.env.BF2_ORACLE_NPM_CACHE
  ? path.resolve(process.env.BF2_ORACLE_NPM_CACHE)
  : path.join(HARNESS_ROOT, ".oracle-npm-cache");

const NPM = process.platform === "win32" ? "npm.cmd" : "npm";

const FRAMEWORKS = Object.freeze({
  vue: {
    domain: VUE_DOMAIN,
    lockPath: () => VUE_EVIDENCE_LOCK,
    closurePath: () => VUE_EVIDENCE_CLOSURE,
    lockSha256: EVIDENCE_LOCK_DIGESTS.vuePackageLockSha256,
    closureSha256: EVIDENCE_LOCK_DIGESTS.vueClosureSha256,
  },
  svelte: {
    domain: SVELTE_DOMAIN,
    lockPath: () => SVELTE_EVIDENCE_LOCK,
    closurePath: () => SVELTE_EVIDENCE_CLOSURE,
    lockSha256: EVIDENCE_LOCK_DIGESTS.sveltePackageLockSha256,
    closureSha256: EVIDENCE_LOCK_DIGESTS.svelteClosureSha256,
  },
});

function frameworkEntry(framework) {
  const entry = FRAMEWORKS[framework];
  if (entry === undefined) throw new Error(`unknown oracle framework: ${framework}`);
  return entry;
}

function sha256Text(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

/**
 * Digest of the realized installed tree — the physical enumeration, not
 * the lock's claim. Recorded in golden provenance as the closure the
 * oracle actually executed from. Each row contributes `contentSha256`
 * (per-package file digest) with path/name/version, so a tampered payload
 * file with untouched package.json still fails provenance.
 */
export function realizedClosureDigest(realizedRows) {
  const canonical = realizedRows
    .map((row) => `${row.path}\t${row.name}\t${row.version}\t${row.contentSha256 ?? "-"}`)
    .sort()
    .join("\n");
  return sha256Text(canonical);
}

export class OracleCacheUnprovisionedError extends Error {
  constructor(message, details) {
    super(message);
    this.name = "OracleCacheUnprovisionedError";
    this.details = details;
  }
}

/**
 * Realization is offline-only, fail-closed. The provisioned local cache is
 * the sole package source (`packages/framework-conformance-harness/scripts/provision-oracle-npm-cache.mjs` is the
 * one sanctioned network step). Missing cache refuses with an actionable
 * error — no networked `npm ci`, no opt-in network mode.
 */
function npmInstallArgs() {
  if (!existsSync(ORACLE_NPM_CACHE_ROOT)) {
    throw new OracleCacheUnprovisionedError(
      `oracle npm cache not provisioned at ${ORACLE_NPM_CACHE_ROOT} — run ` +
        "`node packages/framework-conformance-harness/scripts/provision-oracle-npm-cache.mjs` first; oracle realization is " +
        "offline-only and never falls back to a networked install",
      { cacheRoot: ORACLE_NPM_CACHE_ROOT },
    );
  }
  return [
    "ci",
    "--ignore-scripts",
    "--no-audit",
    "--no-fund",
    "--offline",
    "--cache",
    ORACLE_NPM_CACHE_ROOT,
  ];
}

/**
 * Validate the physical tree at `installDir` against the committed lock:
 * realized-closure enumeration + layer 1 resolved versions.
 * @returns realized rows on success, null on mismatch.
 */
function validateRealizedTree(entry, installDir) {
  if (!existsSync(path.join(installDir, "node_modules"))) return null;
  const copiedLock = path.join(installDir, "package-lock.json");
  if (!existsSync(copiedLock)) return null;
  if (
    sha256Text(readFileSync(copiedLock, "utf8")) !==
    sha256Text(readFileSync(entry.lockPath(), "utf8"))
  )
    return null;
  const lockRows = enumerateLockClosure(entry.lockPath());
  const realized = enumerateInstalledClosure(installDir);
  const comparison = compareRealizedToLock(realized, lockRows);
  if (!comparison.ok || realized.length !== lockRows.length) return null;
  try {
    assertPackagesPinned(
      entry.domain,
      installDir,
      entry.lockPath(),
      entry.lockSha256,
      entry.closurePath(),
      entry.closureSha256,
    );
  } catch {
    return null;
  }
  return realized;
}

// Recorded-content refusal gate. `validateRealizedTree` compares the
// physical tree against the lock (paths, names, versions, edges) — the lock
// records no file contents, so a payload mutation with untouched
// package.json, or a torn tree whose manifests survive, still satisfies it.
// Each successful realization records per-package content digests (sibling
// manifest keyed to the committed lock digest). Later loads throw
// PackageDriftError before any oracle module is resolved when live digests
// deviate. Drift is never self-healed: delete the install dir and its
// content manifest, then re-realize.

function contentManifestPath(framework) {
  return path.join(ORACLE_INSTALLS_ROOT, `${framework}.content-manifest.json`);
}

/**
 * Record per-package content digests for a just-validated tree
 * (temp-write-then-rename). Keyed to the committed lock digest so a new
 * lock supersedes the record instead of refusing the new realization.
 */
function recordContentManifest(framework, entry, realizedRows) {
  const manifest = {
    lockSha256: entry.lockSha256,
    content: Object.fromEntries(realizedRows.map((row) => [row.path, row.contentSha256])),
  };
  const finalPath = contentManifestPath(framework);
  const tempPath = `${finalPath}.tmp-${process.pid}-${randomUUID()}`;
  writeFileSync(tempPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
  renameSync(tempPath, finalPath);
}

/**
 * Compare live per-package content digests against the recorded
 * expectation and throw on any deviation. Runs before
 * validation/realization inside `ensureOracleDomain` — strictly before any
 * oracle module load. A missing record (first realization or superseded
 * lock) is not drift; an unparseable record is a refusal.
 */
function assertRecordedContentIntact(entry, framework, installDir) {
  const manifestFile = contentManifestPath(framework);
  if (!existsSync(manifestFile)) return;
  let recorded;
  try {
    recorded = JSON.parse(readFileSync(manifestFile, "utf8"));
  } catch (error) {
    throw new PackageDriftError(
      `${framework}: recorded content manifest ${manifestFile} is unreadable (${error}); ` +
        `refusing to load the oracle — remove ${installDir} and the manifest, then re-realize`,
      { framework, installDir, manifestFile, layer: "realized-content-manifest-unreadable" },
    );
  }
  if (recorded.lockSha256 !== entry.lockSha256) return; // superseded by a new committed lock
  if (!existsSync(path.join(installDir, "node_modules"))) return; // no tree — realization path
  const realized = enumerateInstalledClosure(installDir);
  const realizedByPath = new Map(realized.map((row) => [row.path, row]));
  const problems = [];
  for (const [rowPath, expected] of Object.entries(recorded.content)) {
    const installed = realizedByPath.get(rowPath);
    if (installed === undefined) {
      problems.push(`${rowPath}: recorded package missing from the installed tree`);
    } else if (installed.contentSha256 !== expected) {
      problems.push(
        `${rowPath}: installed content sha256 ${installed.contentSha256} does not match the recorded ${expected}`,
      );
    }
  }
  for (const row of realized) {
    if (recorded.content[row.path] === undefined) {
      problems.push(`${row.path}: installed package absent from the recorded closure`);
    }
  }
  if (problems.length > 0) {
    throw new PackageDriftError(
      `${framework}: installed oracle tree at ${installDir} has drifted from the content ` +
        `digests recorded at realization time — refusing to load the oracle compiler. ` +
        `First problems: ${problems.slice(0, 3).join("; ")}. If this drift is expected, ` +
        `remove ${installDir} and ${manifestFile}, then re-realize from the committed lock.`,
      { framework, installDir, manifestFile, layer: "realized-content-drift", problems },
    );
  }
}

/**
 * Resolve a package-exports target under an enabled condition set, in
 * declaration order (Node's PACKAGE_TARGET_RESOLVE). Relative target
 * string, or null when no branch matches.
 */
function resolveExportsTarget(target, conditions) {
  if (typeof target === "string") return target;
  if (Array.isArray(target)) {
    for (const item of target) {
      const resolved = resolveExportsTarget(item, conditions);
      if (resolved !== null) return resolved;
    }
    return null;
  }
  if (target !== null && typeof target === "object") {
    for (const [key, value] of Object.entries(target)) {
      if (key === "default" || conditions.has(key)) {
        const resolved = resolveExportsTarget(value, conditions);
        if (resolved !== null) return resolved;
      }
    }
  }
  return null;
}

/**
 * On-disk file an ESM `import` of `specifier` from inside the install tree
 * resolves to — resolution only, no evaluation. Walks the package's exports
 * map under the real loader's import conditions (`node`, `import`, plus
 * per-row extras such as hydration's `browser`). Require-condition
 * resolution can land on a different file (svelte/compiler: `require` →
 * compiler/index.js, `default` → src/compiler/index.js). Fail-closed: a
 * missing manifest, missing subpath exports entry, or unresolvable
 * condition branch returns null — never guess.
 */
function esmImportTargetFile(installDir, specifier, extraConditions) {
  const parts = specifier.split("/");
  const packageNameSegments = specifier.startsWith("@") ? 2 : 1;
  if (parts.length < packageNameSegments) return null;
  const pkgName = parts.slice(0, packageNameSegments).join("/");
  const subpath =
    parts.length === packageNameSegments ? "." : `./${parts.slice(packageNameSegments).join("/")}`;
  const pkgDir = path.join(installDir, "node_modules", ...pkgName.split("/"));
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(path.join(pkgDir, "package.json"), "utf8"));
  } catch {
    return null;
  }
  const exportsField = manifest.exports;
  if (exportsField === undefined || exportsField === null) return null;
  // Top-level exports without "./" keys is shorthand for the "." subpath.
  const isSubpathKeyed =
    typeof exportsField === "object" &&
    !Array.isArray(exportsField) &&
    Object.keys(exportsField).some((key) => key === "." || key.startsWith("./"));
  const subpathTarget = isSubpathKeyed
    ? exportsField[subpath]
    : subpath === "."
      ? exportsField
      : undefined;
  if (subpathTarget === undefined) return null;
  const conditions = new Set(["node", "import", ...(extraConditions ?? [])]);
  const relativeTarget = resolveExportsTarget(subpathTarget, conditions);
  if (typeof relativeTarget !== "string") return null;
  return path.join(pkgDir, relativeTarget);
}

/**
 * Torn-tree structural check, independent of the content-digest record —
 * resolution only, no evaluation. Both halves required:
 *
 *  1. every direct oracle package's declared root entry must resolve from
 *     the realized installation;
 *  2. every production load specifier (`domain.oracleLoadSpecifiers`) must
 *     resolve under the same loader semantics as its caller — CJS for
 *     `require` rows, import-condition exports + on-disk existence for
 *     `import` rows. Root resolvability alone misses a torn subpath whose
 *     exports targets diverge (deleting svelte/src/compiler leaves `svelte`
 *     and its CJS `svelte/compiler` bundle resolvable while the ESM import
 *     cannot load).
 *
 * A half-written install whose package.json manifests survived (missing
 * dist/) passes the lock comparison but cannot load; refuse it instead of
 * handing it to the loader.
 */
function assertOracleEntrypointsResolvable(entry, framework, installDir) {
  const refuse = (subject, error) => {
    throw new PackageDriftError(
      `${framework}: oracle entry ${subject} does not resolve inside the realized installation ` +
        `at ${installDir} — the install tree is torn or incomplete; remove it (and ` +
        `${contentManifestPath(framework)}) and re-realize`,
      {
        framework,
        installDir,
        pkgName: subject,
        layer: "oracle-entry-unresolvable",
        cause: String(error),
      },
    );
  };
  const req = createRequire(path.join(installDir, "package.json"));
  for (const pkgName of Object.keys(entry.domain.directPackages)) {
    try {
      req.resolve(pkgName);
    } catch (error) {
      refuse(pkgName, error);
    }
  }
  for (const row of entry.domain.oracleLoadSpecifiers) {
    if (row.loader === "require") {
      try {
        req.resolve(row.specifier);
      } catch (error) {
        refuse(row.specifier, error);
      }
      continue;
    }
    const targetFile = esmImportTargetFile(installDir, row.specifier, row.extraConditions);
    if (targetFile === null) {
      refuse(row.specifier, "no exports target resolvable under the loader's import conditions");
      continue;
    }
    let stats = null;
    try {
      stats = statSync(targetFile);
    } catch (error) {
      refuse(row.specifier, error);
    }
    if (stats !== null && !stats.isFile()) {
      refuse(row.specifier, `resolved exports target ${targetFile} is not a file`);
    }
  }
}

// Bounded wait for the cross-process realization lock. A cold `npm ci`
// takes tens of seconds; a lock left by a crashed process must surface as
// a loud error (naming the stale path), not hang.
const REALIZE_LOCK_TIMEOUT_MS = 5 * 60 * 1000;
const REALIZE_LOCK_POLL_MS = 200;

/** Synchronous sleep (the realization path is fully synchronous). */
function sleepSync(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

/**
 * Cross-process mutual exclusion for one framework's realize-then-swap.
 * Non-recursive `mkdirSync` is an atomic test-and-set on POSIX and Windows:
 * one contender succeeds, the rest see EEXIST. Wait-and-retry with a
 * bounded timeout (not fail-closed: two live contenders must converge on
 * one validated tree; failing closed would make concurrent runs flaky). A
 * crashed holder surfaces as the timeout error naming the stale lock.
 *
 * @returns {string} the held lock path (remove in a `finally`)
 */
function acquireRealizeLock(framework) {
  mkdirSync(ORACLE_INSTALLS_ROOT, { recursive: true });
  const lockPath = path.join(ORACLE_INSTALLS_ROOT, `${framework}.lock`);
  const deadline = Date.now() + REALIZE_LOCK_TIMEOUT_MS;
  for (;;) {
    try {
      mkdirSync(lockPath); // not recursive: EEXIST is the exclusion signal
      return lockPath;
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      if (Date.now() >= deadline) {
        throw new Error(
          `timed out after ${REALIZE_LOCK_TIMEOUT_MS}ms waiting for the oracle realization ` +
            `lock ${lockPath}; if no other realization is running, remove the stale lock ` +
            `directory and retry`,
        );
      }
      sleepSync(REALIZE_LOCK_POLL_MS);
    }
  }
}

function realizeInstall(entry, framework, installDir) {
  const npmArgs = npmInstallArgs(); // fail-closed cache check before any lock or stage work
  const oracleDir = path.dirname(entry.lockPath());
  const lockPath = acquireRealizeLock(framework);
  try {
    // A concurrent realizer may have completed while we waited: adopt its
    // validated tree instead of clobbering it.
    if (validateRealizedTree(entry, installDir) !== null) return;
    const stage = path.join(
      ORACLE_INSTALLS_ROOT,
      `.stage-${framework}-${process.pid}-${randomUUID()}`,
    );
    mkdirSync(stage, { recursive: true });
    try {
      copyFileSync(path.join(oracleDir, "package.json"), path.join(stage, "package.json"));
      copyFileSync(entry.lockPath(), path.join(stage, "package-lock.json"));
      execFileSync(NPM, npmArgs, { cwd: stage, stdio: "pipe" });
      // Validate the stage before it is reader-visible: swap only after
      // `npm ci` and closure validation succeed.
      const staged = validateRealizedTree(entry, stage);
      if (staged === null) {
        throw new PackageDriftError(
          `${framework}: staged oracle installation does not realize the committed lock closure`,
          { framework, stage, layer: "realized-install-stage" },
        );
      }
      rmSync(installDir, { recursive: true, force: true });
      renameSync(stage, installDir);
      // Record content digests every later load is checked against.
      recordContentManifest(framework, entry, staged);
    } finally {
      rmSync(stage, { recursive: true, force: true });
    }
  } finally {
    rmSync(lockPath, { recursive: true, force: true });
  }
}

const ensured = new Map();

/**
 * Validation gate every oracle load passes through. Static evidence,
 * realize when needed, prove the exact realized closure.
 *
 * Memoization covers only the expensive one-time proof (realization,
 * lock-closure validation, digest). The two live refusal gates —
 * recorded-content intactness and oracle-entry resolvability — run on
 * every call: a payload mutated after a successful load in this process
 * must refuse the next load. No path returns without both gates passing
 * against the live tree.
 *
 * @param {"vue"|"svelte"} framework
 * @returns {{ installDir: string, realizedClosureSha256: string }}
 */
export function ensureOracleDomain(framework) {
  const entry = frameworkEntry(framework);
  const memo = ensured.get(framework);
  if (memo !== undefined) {
    assertRecordedContentIntact(entry, framework, memo.installDir);
    assertOracleEntrypointsResolvable(entry, framework, memo.installDir);
    return memo;
  }

  // 1. Static evidence first — mutated committed evidence refuses before
  // any install work or oracle module evaluation.
  assertEvidenceStaticPinned(
    entry.domain,
    entry.lockPath(),
    entry.lockSha256,
    entry.closurePath(),
    entry.closureSha256,
  );

  // 2. Content-drift refusal before validation/realization: a tree that
  // deviates from recorded digests refuses here — never silently
  // re-realized, never reaches the loader.
  const installDir = path.join(ORACLE_INSTALLS_ROOT, framework);
  assertRecordedContentIntact(entry, framework, installDir);

  // 3–4. Realize when needed, then prove the physical tree.
  let realized = validateRealizedTree(entry, installDir);
  if (realized === null) {
    realizeInstall(entry, framework, installDir);
    realized = validateRealizedTree(entry, installDir);
  }
  if (realized === null) {
    throw new PackageDriftError(
      `${framework}: isolated oracle installation at ${installDir} does not realize the committed lock closure`,
      { framework, installDir, layer: "realized-install" },
    );
  }

  // 5. Torn-tree loadability: every direct oracle entry must resolve from
  // the validated tree before the loader touches it (resolution only).
  assertOracleEntrypointsResolvable(entry, framework, installDir);

  // Arm the content-drift gate for installs realized before the record
  // existed (or under a superseded lock): current digests become the record.
  const manifestFile = contentManifestPath(framework);
  let needsRecord = true;
  if (existsSync(manifestFile)) {
    try {
      needsRecord = JSON.parse(readFileSync(manifestFile, "utf8")).lockSha256 !== entry.lockSha256;
    } catch {
      needsRecord = true; // unreadable records are refused above when armed
    }
  }
  if (needsRecord) recordContentManifest(framework, entry, realized);

  const result = Object.freeze({
    installDir,
    realizedClosureSha256: realizedClosureDigest(realized),
  });
  ensured.set(framework, result);
  return result;
}

const requires = new Map();

/**
 * Synchronous CommonJS load of an oracle module from the validated isolated
 * install. Vue compiler/runtime packages route Node import and require to
 * the same CJS artifacts via the `node` export condition.
 */
export function oracleRequire(framework, specifier) {
  const { installDir } = ensureOracleDomain(framework);
  let req = requires.get(installDir);
  if (req === undefined) {
    req = createRequire(path.join(installDir, "package.json"));
    requires.set(installDir, req);
  }
  return req(specifier);
}

const importedNamespaces = new Map();

/**
 * ESM load of an oracle module from the validated isolated install — a
 * scratch importer written inside the install tree so Node's real ESM
 * resolution (export maps, conditions) runs against the realized closure
 * and locked transitive deps. Shares one instance graph with compiled
 * scratch modules from `oracleScratchDir` (same resolution root), which
 * SSR execution requires.
 *
 * @returns {Promise<object>} the module namespace
 */
export async function importOracleModule(framework, specifier) {
  const { installDir } = ensureOracleDomain(framework);
  const key = `${installDir}\0${specifier}`;
  const memo = importedNamespaces.get(key);
  if (memo !== undefined) return memo;
  const loaderDir = path.join(installDir, ".bf2-loader");
  mkdirSync(loaderDir, { recursive: true });
  const digest = sha256Text(specifier).slice(0, 16);
  // The install is shared by every concurrent harness process. A stable
  // importer name lets one process truncate the file while another process
  // is importing it, yielding an empty namespace (for example, an undefined
  // Vue `createSSRApp` or `renderToString`). Give every load its own file;
  // the resolved package module still comes from Node's canonical cache and
  // therefore shares the instance graph with compiled oracle scratch files.
  const importerPath = path.join(loaderDir, `ns-${digest}-${process.pid}-${randomUUID()}.mjs`);
  writeFileSync(importerPath, `export * as ns from ${JSON.stringify(specifier)};\n`, "utf8");
  try {
    const namespace = (await import(pathToFileURL(importerPath).href)).ns;
    importedNamespaces.set(key, namespace);
    return namespace;
  } finally {
    rmSync(importerPath, { force: true });
  }
}

/**
 * Scratch directory inside the validated install tree, for executing
 * compiled oracle output whose bare imports (`from "vue"`,
 * `from "svelte/internal/client"`) must resolve against the realized
 * closure and the same module instances `importOracleModule` hands out.
 */
export function oracleScratchDir(framework, label) {
  const { installDir } = ensureOracleDomain(framework);
  const dir = path.join(installDir, ".bf2-scratch", label);
  mkdirSync(dir, { recursive: true });
  return dir;
}

/** Resolution base for link-validity checks against a domain's closure. */
export function oracleLinkBaseDir(framework) {
  return ensureOracleDomain(framework).installDir;
}
