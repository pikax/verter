/**
 * semver.mjs — minimal strict semver parsing and comparison.
 *
 * Shared by scripts/set-version.mjs and scripts/bump.mjs. Intentionally
 * dependency-free: these scripts must run on any developer machine with
 * nothing but Node.
 */

const SEMVER_RE =
  /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

/**
 * Parse a strict semver string.
 * Returns { major, minor, patch, prerelease: string[] | null } or null.
 */
export function parseSemver(version) {
  const m = SEMVER_RE.exec(version);
  if (!m) return null;
  return {
    major: Number(m[1]),
    minor: Number(m[2]),
    patch: Number(m[3]),
    prerelease: m[4] ? m[4].split(".") : null,
  };
}

export function isValidSemver(version) {
  return parseSemver(version) !== null;
}

function compareIdentifiers(a, b) {
  const aNum = /^\d+$/.test(a);
  const bNum = /^\d+$/.test(b);
  if (aNum && bNum) return Number(a) - Number(b);
  if (aNum) return -1; // numeric identifiers sort before alphanumeric
  if (bNum) return 1;
  return a < b ? -1 : a > b ? 1 : 0;
}

/**
 * Strict semver precedence. Returns a negative number, 0, or a positive
 * number when a < b, a == b, a > b. Build metadata is ignored.
 */
export function compareSemver(a, b) {
  const pa = parseSemver(a);
  const pb = parseSemver(b);
  if (!pa || !pb) throw new Error(`semver: cannot compare "${a}" and "${b}"`);
  for (const key of ["major", "minor", "patch"]) {
    if (pa[key] !== pb[key]) return pa[key] - pb[key];
  }
  // A stable release sorts after any of its pre-releases.
  if (!pa.prerelease && !pb.prerelease) return 0;
  if (!pa.prerelease) return 1;
  if (!pb.prerelease) return -1;
  const len = Math.min(pa.prerelease.length, pb.prerelease.length);
  for (let i = 0; i < len; i++) {
    const c = compareIdentifiers(pa.prerelease[i], pb.prerelease[i]);
    if (c !== 0) return c;
  }
  return pa.prerelease.length - pb.prerelease.length;
}

export function semverGt(a, b) {
  return compareSemver(a, b) > 0;
}
