// Verifies the packages this harness actually resolved at require-time are
// exactly the pinned official closures — five independent layers, so a
// single mutated file cannot silently pass:
//
//   1. resolved version equality: every direct package this harness
//      requires resolves to exactly domain.packageVersion (catches a
//      package.json range/dist-tag or a stale pnpm store).
//   2. evidence-lock byte integrity: the COMMITTED
//      oracles/{vue,svelte}/package-lock.json evidence file's own SHA-256
//      still matches the digest BF1 recorded (catches evidence tampering).
//   3. evidence-lock content cross-check: every direct package's
//      version+integrity INSIDE that lock file matches domain-pin.mjs's
//      transcription (catches domain-pin.mjs itself drifting from the
//      ratified evidence).
//   4. closure-evidence byte integrity: the COMMITTED
//      oracles/{vue,svelte}/closure.tsv full-transitive-closure evidence
//      file's own SHA-256 still matches the digest BF1 recorded.
//   5. transitive-closure derivation cross-check: the FULL closure —
//      every nested package path, name, version, integrity, resolution
//      URL, and dependency edge — independently re-derived from the
//      committed lockfile must be byte-identical to the committed
//      closure.tsv. A hand-edited nested lock entry (a transitive
//      dependency's resolved version or integrity) breaks this layer even
//      though no direct package changed; an attacker who regenerates
//      closure.tsv to match a mutated lock instead breaks layer 4.
//
// Layers 4-5 run inside `assertPackagesPinned`, which every oracle invoker
// calls BEFORE its first compiler invocation — so transitive drift refuses
// the run before any expectation or candidate work happens. This is the
// "package drift refusal" half of official-core-oracles.md's "The harness
// rejects any source SHA/tree, package version, integrity, or transitive
// closure mismatch before generating expectations or running candidate
// output." The REALIZED half — proving the committed lockfile actually
// installs to exactly this closure — is exercised by the disposable
// scripts-disabled install self-test (test/closure-drift.spec.mjs) via
// src/closure-verify.mjs.

import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";

import { enumerateLockClosure, closureRowsToTsv } from "./closure-verify.mjs";

export class PackageDriftError extends Error {
  constructor(message, details) {
    super(message);
    this.name = "PackageDriftError";
    this.details = details;
  }
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

/**
 * @param {string} pkgName
 * @param {string} baseDir directory to resolve `require` from (the harness
 *   package root, whose devDependencies carry the exact pins)
 */
export function resolvedPackageVersion(pkgName, baseDir) {
  const require = createRequire(baseDir.endsWith("/") ? baseDir : `${baseDir}/`);
  const manifestPath = require.resolve(`${pkgName}/package.json`);
  return JSON.parse(readFileSync(manifestPath, "utf8")).version;
}

/** Line-ending-normalized text equality (Cross-Platform Portability). */
function textEqual(a, b) {
  return a.replace(/\r\n/g, "\n") === b.replace(/\r\n/g, "\n");
}

/**
 * Layers 4-5: full transitive-closure verification against the committed
 * evidence. See the module header for what each layer catches.
 *
 * @param {string} evidenceLockPath committed oracles/<fw>/package-lock.json
 * @param {string} closureTsvPath committed oracles/<fw>/closure.tsv
 * @param {string} expectedClosureSha256 from EVIDENCE_LOCK_DIGESTS
 */
export function assertClosurePinned(evidenceLockPath, closureTsvPath, expectedClosureSha256) {
  const closureText = readFileSync(closureTsvPath, "utf8");
  const closureFileDigest = createHash("sha256").update(closureText, "utf8").digest("hex");
  if (closureFileDigest !== expectedClosureSha256) {
    throw new PackageDriftError(
      `${closureTsvPath}: closure evidence digest drift — expected ${expectedClosureSha256}, got ${closureFileDigest}`,
      {
        closureTsvPath,
        expected: expectedClosureSha256,
        actual: closureFileDigest,
        layer: "closure-digest",
      },
    );
  }
  const derived = closureRowsToTsv(enumerateLockClosure(evidenceLockPath));
  if (!textEqual(derived, closureText)) {
    throw new PackageDriftError(
      `${evidenceLockPath}: transitive closure derived from the lockfile does not match the committed closure evidence ${closureTsvPath}`,
      { evidenceLockPath, closureTsvPath, layer: "closure-derivation" },
    );
  }
}

/**
 * Layers 2-5 only — every check that reads COMMITTED EVIDENCE, none that
 * reads an installed tree. The oracle-install realization gate runs this
 * FIRST (mutated evidence refuses the run before any install work), then
 * realizes the isolated install and runs layer 1 + the realized-closure
 * enumeration against it.
 *
 * @param {object} domain a VUE_DOMAIN or SVELTE_DOMAIN entry from domain-pin.mjs
 * @param {string} evidenceLockPath committed oracles/<framework>/package-lock.json
 * @param {string} expectedLockSha256 from EVIDENCE_LOCK_DIGESTS
 * @param {string} [closureTsvPath] committed oracles/<framework>/closure.tsv
 * @param {string} [expectedClosureSha256] from EVIDENCE_LOCK_DIGESTS
 */
export function assertEvidenceStaticPinned(
  domain,
  evidenceLockPath,
  expectedLockSha256,
  closureTsvPath,
  expectedClosureSha256,
) {
  // Layer 2: the committed evidence lock file's own bytes are unmutated.
  const lockDigest = sha256(evidenceLockPath);
  if (lockDigest !== expectedLockSha256) {
    throw new PackageDriftError(
      `${evidenceLockPath}: evidence lock digest drift — expected ${expectedLockSha256}, got ${lockDigest}`,
      { evidenceLockPath, expected: expectedLockSha256, actual: lockDigest, layer: "lock-digest" },
    );
  }

  // Layer 3: every direct package's version+integrity inside that lock file
  // matches this module's own transcription in domain-pin.mjs.
  const lock = JSON.parse(readFileSync(evidenceLockPath, "utf8"));
  for (const [pkgName, expectedIntegrity] of Object.entries(domain.directPackages)) {
    const entry = lock.packages?.[`node_modules/${pkgName}`];
    if (!entry) {
      throw new PackageDriftError(`${evidenceLockPath}: missing lock entry for ${pkgName}`, {
        pkgName,
        layer: "lock-entry-missing",
      });
    }
    if (entry.version !== domain.packageVersion) {
      throw new PackageDriftError(
        `${evidenceLockPath}: ${pkgName} lock version ${entry.version} != domain pin ${domain.packageVersion}`,
        { pkgName, expected: domain.packageVersion, actual: entry.version, layer: "lock-version" },
      );
    }
    if (entry.integrity !== expectedIntegrity) {
      throw new PackageDriftError(
        `${evidenceLockPath}: ${pkgName} integrity drift vs domain-pin.mjs transcription`,
        { pkgName, expected: expectedIntegrity, actual: entry.integrity, layer: "lock-integrity" },
      );
    }
  }

  // Layers 4-5: full transitive-closure verification.
  if (closureTsvPath !== undefined) {
    assertClosurePinned(evidenceLockPath, closureTsvPath, expectedClosureSha256);
  }
}

/**
 * @param {object} domain a VUE_DOMAIN or SVELTE_DOMAIN entry from domain-pin.mjs
 * @param {string} baseDir directory the direct packages must RESOLVE from —
 *   the isolated per-domain oracle installation realized from the committed
 *   lock (oracle-install.mjs), never the workspace store
 * @param {string} evidenceLockPath absolute path to the committed
 *   oracles/<framework>/package-lock.json
 * @param {string} expectedLockSha256 from EVIDENCE_LOCK_DIGESTS
 * @param {string} [closureTsvPath] committed oracles/<framework>/closure.tsv —
 *   when supplied (every production caller supplies it), layers 4-5 run too
 * @param {string} [expectedClosureSha256] from EVIDENCE_LOCK_DIGESTS
 */
export function assertPackagesPinned(
  domain,
  baseDir,
  evidenceLockPath,
  expectedLockSha256,
  closureTsvPath,
  expectedClosureSha256,
) {
  // Layer 1: resolved version equality for every direct package this
  // harness actually requires.
  const resolved = {};
  for (const pkgName of Object.keys(domain.directPackages)) {
    const version = resolvedPackageVersion(pkgName, baseDir);
    if (version !== domain.packageVersion) {
      throw new PackageDriftError(
        `${pkgName}: resolved version ${version}, expected exactly ${domain.packageVersion}`,
        { pkgName, expected: domain.packageVersion, actual: version, layer: "resolved-version" },
      );
    }
    resolved[pkgName] = version;
  }

  // Layers 2-5: committed-evidence checks.
  assertEvidenceStaticPinned(
    domain,
    evidenceLockPath,
    expectedLockSha256,
    closureTsvPath,
    expectedClosureSha256,
  );

  return resolved;
}
