// Verifies the packages this harness actually resolved at require-time are
// exactly the pinned official closures — three independent layers, so a
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
//
// This is the "package drift refusal" half of official-core-oracles.md's
// "The harness rejects any source SHA/tree, package version, integrity, or
// transitive closure mismatch before generating expectations or running
// candidate output."

import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";

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

/**
 * @param {object} domain a VUE_DOMAIN or SVELTE_DOMAIN entry from domain-pin.mjs
 * @param {string} baseDir harness package root
 * @param {string} evidenceLockPath absolute path to the committed
 *   oracles/<framework>/package-lock.json
 * @param {string} expectedLockSha256 from EVIDENCE_LOCK_DIGESTS
 */
export function assertPackagesPinned(domain, baseDir, evidenceLockPath, expectedLockSha256) {
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

  return resolved;
}
