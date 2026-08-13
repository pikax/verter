// Isolated per-domain oracle installations — the SOLE source the official
// compilers and runtimes are ever loaded from.
//
// The oracle domain is defined by the COMMITTED evidence locks
// (docs/.../oracles/{vue,svelte}/package-lock.json), never by workspace
// dependency resolution: a workspace store can (and did) resolve the
// oracle's transitive dependencies — the Svelte compiler's own `acorn` /
// `@sveltejs/acorn-typescript`, Vue compiler-sfc's `postcss`, … — to
// versions that DRIFT from the committed closure, silently changing the
// actual parser/plugin combination the oracle runs with. So each domain is
// realized as an actual `npm ci` installation of the committed lock into a
// dedicated non-workspace directory (`.oracle-installs/<framework>`), and
// EVERY load goes through a validation gate that proves the EXACT realized
// closure before a single oracle module is evaluated:
//
//   1. static evidence layers (package-pin.mjs layers 2-5): committed lock
//      byte digest, lock-content cross-check vs domain-pin.mjs, closure.tsv
//      byte digest, independent closure re-derivation — mutated evidence
//      refuses the run BEFORE any install work;
//   2. realization: OFFLINE-ONLY `npm ci --offline --ignore-scripts` from
//      the committed lock into the isolated directory, sourced exclusively
//      from the provisioned .oracle-npm-cache; a missing cache REFUSES
//      realization with an actionable error (OracleCacheUnprovisionedError)
//      — never a silent networked fallback. Re-run only when the copied
//      lock no longer byte-matches the committed one or validation fails.
//      The realize-then-swap sequence is serialized cross-process by an
//      exclusive mkdir lock (.oracle-installs/<framework>.lock), installs
//      into a temporary stage directory, validates the STAGE, and only
//      then atomically renames it onto the final path — no concurrent
//      process or in-flight compiler load ever observes a half-written
//      tree;
//   3. realized-closure validation (closure-verify.mjs): the PHYSICAL
//      installed tree is independently enumerated — real manifests at real
//      nested paths, dependency edges re-resolved through the tree — and
//      compared entry-for-entry against the lock-derived closure; plus
//      layer 1 (every direct package resolves to exactly the pinned
//      version FROM THE ISOLATED INSTALL);
//   4. content-drift refusal: each realization RECORDS the installed
//      tree's per-package content digests (a sibling manifest keyed to the
//      committed lock digest), and every later load REFUSES — throws
//      before any oracle module is resolved or evaluated — when the live
//      tree's freshly-computed digests deviate from that record (a
//      poisoned payload file, a torn subtree). Drift is never silently
//      repaired by re-realizing over it;
//   5. entry-point loadability: every direct oracle package's declared
//      entry must RESOLVE (no evaluation) from the realized tree, so a
//      half-written install whose package.json manifests all survived is
//      refused structurally rather than handed to the loader.
//
// Only after all of these does `oracleRequire` / `importOracleModule` hand out
// a module — dynamically, from the isolated realized installation. No
// production path holds a static top-level import of an oracle package.

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
 * Deterministic digest of the REALIZED installed tree — the physical
 * enumeration, not the lock's claim about it. Recorded in golden
 * provenance as the exact closure the oracle actually executed from.
 * Each row contributes its `contentSha256` (the per-package file-content
 * digest from enumerateInstalledClosure) alongside path/name/version, so
 * a tampered payload FILE inside an installed package — package.json
 * name/version untouched — changes this digest and therefore fails the
 * strict provenance comparison against the recorded value.
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
 * Realization is OFFLINE-ONLY, fail-closed. The provisioned local cache is
 * the sole package source (provisioning it is the package's ONE sanctioned
 * network step — scripts/provision-oracle-npm-cache.mjs). A missing cache
 * REFUSES realization with an actionable error instead of silently falling
 * back to a networked `npm ci`: there is no opt-in network mode.
 */
function npmInstallArgs() {
  if (!existsSync(ORACLE_NPM_CACHE_ROOT)) {
    throw new OracleCacheUnprovisionedError(
      `oracle npm cache not provisioned at ${ORACLE_NPM_CACHE_ROOT} — run ` +
        "`node scripts/provision-oracle-npm-cache.mjs` first; oracle realization is " +
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
 * Validates the physical installed tree at `installDir` against the
 * committed lock: full realized-closure enumeration + layer 1 resolved
 * versions. @returns the realized enumeration rows on success, null on any
 * mismatch (callers decide between re-realizing and refusing).
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

// ---------------------------------------------------------------------------
// Recorded-content refusal gate. `validateRealizedTree` compares the physical
// tree against the LOCK (paths, names, versions, edges) — but the lock records
// no file contents, so a payload mutation that leaves every package.json
// untouched, or a torn tree whose manifests all survive, still satisfies it
// and would previously be silently re-certified (or, on a name/version
// mismatch, silently re-realized over). The gate below closes that: each
// successful realization RECORDS the per-package content digests the freshly
// installed tree actually had (a sibling manifest file next to the install
// dir, keyed to the committed lock digest), and every later load REFUSES —
// throws PackageDriftError BEFORE any oracle module is resolved, required, or
// imported — when the live tree's freshly-computed digests deviate from that
// record in any way. Drift is never self-healed: remediation is an explicit
// operator action (delete the install dir AND its content manifest, then
// re-realize).
// ---------------------------------------------------------------------------

function contentManifestPath(framework) {
  return path.join(ORACLE_INSTALLS_ROOT, `${framework}.content-manifest.json`);
}

/**
 * Records the expected per-package content digests for a just-validated
 * realized tree (temp-write-then-rename, same atomicity discipline as the
 * install swap itself). Keyed to the committed lock digest so a legitimate
 * domain amendment (new committed lock) supersedes the record instead of
 * refusing the new realization.
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
 * The refusal half: compares the live installed tree's freshly-computed
 * per-package content digests against the recorded expectation and THROWS on
 * any deviation — a mutated payload file, a missing recorded package, an
 * extra unrecorded one. Runs before validation/realization inside
 * `ensureOracleDomain`, i.e. strictly before any oracle module load. A
 * missing record (first realization, or a record for a superseded lock) is
 * not drift — realization writes the record; an unparseable record IS a
 * refusal (it only exists torn if something interfered with it).
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
 * Resolves a package-exports target object/string/array under an enabled
 * condition set, in declaration order — the same order Node's own
 * PACKAGE_TARGET_RESOLVE applies for these shapes. Returns the relative
 * target string, or null when no branch matches.
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
 * The on-disk file an ESM `import` of `specifier` from inside the install
 * tree resolves to — resolution only, no evaluation. Walks the package's
 * own exports map under the import conditions the real loader runs with
 * (`node`, `import`, plus any per-row extras such as the hydration runner's
 * `browser`), because require-condition resolution can land on a DIFFERENT
 * file than the import the production loader performs (svelte/compiler:
 * `require` → compiler/index.js, `default` → src/compiler/index.js). The
 * walk is deliberately fail-closed: a missing package manifest, a missing
 * exports entry for the exact subpath, or an unresolvable condition branch
 * all return null and refuse the tree, never guess.
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
 * resolution only, no module code is evaluated. Two halves, both required:
 *
 *  1. every direct oracle package's declared root entry must resolve from
 *     the realized installation (broad structural coverage of the direct
 *     closure members);
 *  2. every ACTUAL oracle load specifier the production callers use
 *     (`domain.oracleLoadSpecifiers`, derived from the real
 *     oracleRequire/importOracleModule callsites) must resolve UNDER THE
 *     SAME LOADER SEMANTICS as its caller — CJS resolution for `require`
 *     rows, import-condition exports resolution with an on-disk existence
 *     check for `import` rows. Root resolvability alone proved nothing
 *     about a torn subpath whose exports condition targets diverge
 *     (deleting svelte/src/compiler leaves `svelte` and its CJS
 *     `svelte/compiler` bundle resolvable while the production ESM import
 *     cannot load).
 *
 * A half-written install whose package.json manifests all survived — e.g. a
 * package's dist/ payload directory missing — passes the lock comparison
 * (names/versions/edges intact) but cannot possibly load; this refuses it
 * with a clear error instead of handing it to the loader.
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

// Bounded wait for the cross-process realization lock: a cold `npm ci` of a
// domain takes tens of seconds, so contenders poll patiently, but a lock
// left behind by a crashed process must eventually surface as a LOUD error
// (naming the stale path) rather than an infinite hang.
const REALIZE_LOCK_TIMEOUT_MS = 5 * 60 * 1000;
const REALIZE_LOCK_POLL_MS = 200;

/** Synchronous sleep (the realization path is fully synchronous). */
function sleepSync(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

/**
 * Cross-process mutual exclusion for one framework's realize-then-swap
 * sequence. A non-recursive `mkdirSync` is an atomic test-and-set on POSIX
 * and Windows: exactly one contender observes success; the rest see EEXIST.
 * Contention strategy: WAIT-AND-RETRY with a bounded timeout (chosen over
 * fail-closed because two live contenders must CONVERGE on one validated
 * tree — the loser waits, then adopts the winner's install; failing closed
 * would make routine concurrent runs flaky). A crashed holder surfaces as
 * the timeout error naming the stale lock directory to remove.
 *
 * @returns {string} the held lock path (remove in a `finally`)
 */
function acquireRealizeLock(framework) {
  mkdirSync(ORACLE_INSTALLS_ROOT, { recursive: true });
  const lockPath = path.join(ORACLE_INSTALLS_ROOT, `${framework}.lock`);
  const deadline = Date.now() + REALIZE_LOCK_TIMEOUT_MS;
  for (;;) {
    try {
      mkdirSync(lockPath); // deliberately NOT recursive: EEXIST is the exclusion signal
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
  const npmArgs = npmInstallArgs(); // fail-closed cache check BEFORE any lock or stage work
  const oracleDir = path.dirname(entry.lockPath());
  const lockPath = acquireRealizeLock(framework);
  try {
    // A concurrent realizer may have completed while this process waited on
    // the lock: adopt its validated tree instead of clobbering it.
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
      // Validate the STAGE before it can become reader-visible: the atomic
      // swap happens only after BOTH `npm ci` and the closure validation
      // succeed (the same temp-write-then-rename discipline as golden
      // publication in golden-store.mjs).
      const staged = validateRealizedTree(entry, stage);
      if (staged === null) {
        throw new PackageDriftError(
          `${framework}: staged oracle installation does not realize the committed lock closure`,
          { framework, stage, layer: "realized-install-stage" },
        );
      }
      rmSync(installDir, { recursive: true, force: true });
      renameSync(stage, installDir);
      // Record the freshly-installed tree's content digests as the expected
      // state every later load is checked against (assertRecordedContentIntact).
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
 * The validation gate every oracle load passes through. Runs the static
 * evidence layers, realizes the isolated install when needed, and proves
 * the EXACT realized closure before returning.
 *
 * Memoization covers ONLY the expensive one-time proof (realization, the
 * full lock-closure validation, the realized-closure digest). The two LIVE
 * refusal gates — recorded-content intactness and oracle-entry
 * resolvability — run on EVERY call, memo hit or cold: a payload mutated or
 * torn AFTER a successful load in this same process must refuse the NEXT
 * load, not only the next process's first load. No path returns a result
 * (cached or fresh) without both gates having just passed against the live
 * tree.
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

  // 1. Static evidence layers FIRST — mutated committed evidence refuses
  // the run before any install work or oracle module evaluation.
  assertEvidenceStaticPinned(
    entry.domain,
    entry.lockPath(),
    entry.lockSha256,
    entry.closurePath(),
    entry.closureSha256,
  );

  // 2. Content-drift refusal gate BEFORE validation/realization: a tree that
  // deviates from the digests recorded at its own realization time (mutated
  // payload bytes, a torn subtree) REFUSES the run here — it is never
  // silently repaired by a re-realization and never reaches the loader.
  const installDir = path.join(ORACLE_INSTALLS_ROOT, framework);
  assertRecordedContentIntact(entry, framework, installDir);

  // 3-4. Realize when needed, then prove the physical tree.
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

  // 5. Torn-tree loadability: every direct oracle entry point must RESOLVE
  // from the validated tree before the loader may touch it (resolution only —
  // no oracle code is evaluated here).
  assertOracleEntrypointsResolvable(entry, framework, installDir);

  // Arm the content-drift gate for installations realized before the record
  // existed (or recorded under a superseded committed lock): the tree just
  // passed full lock validation, so its current digests become the record.
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
 * installation. (Vue's compiler/runtime packages route Node loads — import
 * AND require — to the same CJS artifacts via their `node` export
 * condition, so this is the identical module shape either way.)
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
 * ESM load of an oracle module from the validated isolated installation —
 * a scratch importer module written INSIDE the install tree, so Node's real
 * ESM resolution (export maps, conditions) runs against the realized
 * closure and every transitive dependency (the Svelte compiler's `acorn`,
 * `@sveltejs/acorn-typescript`, …) is the locked one. Modules loaded this
 * way share one instance graph with compiled scratch modules executed from
 * `oracleScratchDir` (same resolution root), which SSR execution requires.
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
  const importerPath = path.join(loaderDir, `ns-${digest}.mjs`);
  writeFileSync(importerPath, `export * as ns from ${JSON.stringify(specifier)};\n`, "utf8");
  const namespace = (await import(pathToFileURL(importerPath).href)).ns;
  importedNamespaces.set(key, namespace);
  return namespace;
}

/**
 * A scratch directory INSIDE the validated install tree, for executing
 * compiled oracle output whose bare imports (`from "vue"`,
 * `from "svelte/internal/client"`) must resolve against the realized
 * closure — and against the SAME module instances `importOracleModule`
 * hands out.
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
