#!/usr/bin/env node
// check-integration-test-layout.mjs
//
// ANTI-BINARY-GROWTH GUARD (fast-fail CI Node check).
//
// Mechanically prevents re-adding standalone integration-test binaries after the
// consolidation onto a single `tests/main.rs` per crate. Every workspace package
// must expose AT MOST one `tests/main.rs` integration-test target, PLUS any
// targets named in the EXACT central allowlist
// (`scripts/integration-test-layout-allowlist.json`). A second top-level
// `tests/*.rs` auto-becomes its own test binary at compile time and balloons the
// gate; this guard fails the build before that happens.
//
// This is the Node half of a DUAL guard. The in-gate Rust durability mirror is
// `crates/verter_session/tests/cases/integration_test_layout_guard.rs`; both read
// the SAME committed allowlist JSON, so the exception set cannot drift between
// them.
//
// Cross-platform: drives `cargo metadata --format-version 1 --no-deps` and
// Node's `node:fs` / `node:path` only — NO Unix `find` / `grep`, and all path
// comparisons normalize separators (`path.sep` -> `/`) so `src_path` matching
// works identically on Windows, macOS, and Linux.
//
// Exit 0 = GREEN (layout is conformant). Non-zero = RED (a clear per-package
// failure list is printed to stderr). `--json` emits a machine-readable report.

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "..");
const ALLOWLIST_PATH = resolve(__dirname, "integration-test-layout-allowlist.json");

const HELP = `check-integration-test-layout.mjs — anti-binary-growth integration-test layout guard

USAGE
  node scripts/check-integration-test-layout.mjs [--json] [--help]

WHAT IT CHECKS
  Drives \`cargo metadata --format-version 1 --no-deps\` and, for EVERY workspace
  package, FAILS unless the package has:
    * 0 integration-test targets, OR
    * exactly 1 integration-test target whose src_path normalizes to
      <pkg>/tests/main.rs, PLUS any targets named in the EXACT central allowlist.

  PLUS these structural checks (so the bare "exactly 1 main.rs" rule cannot be
  evaded):
    1. If <pkg>/tests/main.rs exists on disk, cargo metadata MUST report that
       exact tests/main.rs target (catches a missing / misconfigured target).
    2. If <pkg>/tests/ has any immediate top-level *.rs AND cargo metadata reports
       ZERO integration-test targets for that package -> FAIL (catches
       autotests = false hiding tests).
    3. An explicit [[test]] whose src_path is NOT tests/main.rs -> FAIL unless
       EXACTLY allowlisted.
    4. A stray IMMEDIATE <pkg>/tests/*.rs file other than main.rs (and other than
       an allowlisted src file) -> FAIL. Files UNDER tests/cases/ (or any
       subdirectory) are fine — only the immediate tests/*.rs level is constrained,
       since those auto-become separate binaries.
    5. MORE THAN ONE metadata test target whose src is <pkg>/tests/main.rs -> FAIL
       (two [[test]] blocks both pointing at tests/main.rs still compile two
       binaries).
    6. Every cargo-auto-discoverable position (immediate tests/*.rs AND
       tests/<dir>/main.rs one subdir deep) that has NO matching metadata target
       -> FAIL (catches a hidden nested tests/rogue/main.rs under autotests=false,
       even when another valid target exists).

THE ALLOWLIST (central, exact, stale-failing)
  scripts/integration-test-layout-allowlist.json is the SINGLE source of truth,
  shared with the Rust guard. Each entry is exact:
    { package, target, src_path (repo-relative, forward-slash), reason }
  No globs, no prefixes, no package-wide switches. STALE-FAILING: if an
  allowlisted (package, target) no longer exists in cargo metadata, or its
  src_path moved, the guard FAILS — a removed binary cannot leave a dead
  exception.

OPTIONS
  --json   Print a JSON report ({ ok, failures, allowlist }) instead of text.
  --help   Show this help and exit 0.

EXIT
  0 = GREEN (conformant). Non-zero = RED (per-package failure list on stderr).
`;

/** Normalize an absolute or relative path to a forward-slash, repo-relative string. */
function toRepoRelPosix(absPath) {
  const rel = relative(REPO_ROOT, absPath);
  return rel.split(sep).join("/");
}

/** Normalize any path's separators to forward slashes (no rebasing). */
function toPosix(p) {
  return p.split(sep).join("/");
}

function loadAllowlist() {
  let raw;
  try {
    raw = readFileSync(ALLOWLIST_PATH, "utf8");
  } catch (err) {
    throw new Error(`failed to read allowlist ${toRepoRelPosix(ALLOWLIST_PATH)}: ${err.message}`);
  }
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (err) {
    throw new Error(
      `allowlist ${toRepoRelPosix(ALLOWLIST_PATH)} is not valid JSON: ${err.message}`,
    );
  }
  // The `allow` array is MANDATORY: a missing / non-array `allow` is a broken
  // allowlist, not "no exceptions" — fail loud (mirrors the Rust guard's panic).
  // NOTE: the Rust guard additionally pins the allowlist to EXACTLY its
  // known process-isolated entries; that durable exact-count pin lives there on
  // purpose. This Node check is the fast structural mirror and does NOT replicate
  // that exact-count pin.
  if (!Array.isArray(parsed.allow)) {
    throw new Error(
      `allowlist ${toRepoRelPosix(ALLOWLIST_PATH)} is missing a top-level \`allow\` ` +
        `array (a missing or non-array \`allow\` is a broken allowlist, not "no exceptions").`,
    );
  }
  const entries = parsed.allow;
  // Reject duplicate keys: a duplicate (package, target) would let a STALE
  // duplicate be masked by a correct one in the matched-set below; we also reject
  // an exact (package, target, src_path) triplet duplicate for full hygiene.
  const seenPkgTarget = new Set();
  const seenTriplet = new Set();
  for (const e of entries) {
    if (
      typeof e.package !== "string" ||
      typeof e.target !== "string" ||
      typeof e.src_path !== "string" ||
      typeof e.reason !== "string"
    ) {
      throw new Error(
        `allowlist entry is malformed (each entry needs string package/target/src_path/reason): ${JSON.stringify(e)}`,
      );
    }
    // src_path must be repo-relative + forward-slash already.
    if (e.src_path.includes("\\") || e.src_path.startsWith("/")) {
      throw new Error(
        `allowlist src_path must be repo-relative and forward-slash normalized: ${JSON.stringify(e.src_path)}`,
      );
    }
    const triplet = `${e.package}::${e.target}::${e.src_path}`;
    if (seenTriplet.has(triplet)) {
      throw new Error(
        `duplicate allowlist entry (package \`${e.package}\`, target \`${e.target}\`, ` +
          `src_path \`${e.src_path}\`): each exception must appear exactly once.`,
      );
    }
    seenTriplet.add(triplet);
    const pkgTarget = `${e.package}::${e.target}`;
    if (seenPkgTarget.has(pkgTarget)) {
      throw new Error(
        `duplicate allowlist (package, target) key (package \`${e.package}\`, target ` +
          `\`${e.target}\`): a (package, target) may appear at most once, otherwise a ` +
          `stale duplicate could be masked by a correct one.`,
      );
    }
    seenPkgTarget.add(pkgTarget);
  }
  return entries;
}

function runCargoMetadata() {
  let stdout;
  try {
    stdout = execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
      cwd: REPO_ROOT,
      encoding: "utf8",
      maxBuffer: 256 * 1024 * 1024,
    });
  } catch (err) {
    throw new Error(`\`cargo metadata\` failed: ${err.message}`);
  }
  try {
    return JSON.parse(stdout);
  } catch (err) {
    throw new Error(`failed to parse \`cargo metadata\` output: ${err.message}`);
  }
}

/**
 * Enumerate the IMMEDIATE *.rs files at <tests_dir> (NOT recursive — only the
 * top-level tests/*.rs level auto-becomes separate binaries). Returns absolute
 * paths. Missing dir -> [].
 */
function immediateTestRsFiles(testsDir) {
  if (!existsSync(testsDir)) return [];
  let dirents;
  try {
    dirents = readdirSync(testsDir, { withFileTypes: true });
  } catch {
    return [];
  }
  const out = [];
  for (const d of dirents) {
    if (!d.isFile()) continue;
    if (!d.name.endsWith(".rs")) continue;
    out.push(join(testsDir, d.name));
  }
  return out;
}

/**
 * Enumerate every cargo-AUTO-DISCOVERABLE integration-test source position under
 * <tests_dir>: every immediate tests/*.rs PLUS every tests/<dir>/main.rs exactly
 * ONE subdirectory deep. Cargo compiles each of these into its OWN test binary,
 * so each must correspond to a reported metadata target. Files deeper than one
 * level, and non-main.rs files inside a subdirectory, are NOT auto-discovered
 * (they only compile when wired as modules under tests/main.rs) and are excluded.
 * Returns absolute paths. Missing dir -> [].
 */
function autoDiscoverableTestCandidates(testsDir) {
  const out = immediateTestRsFiles(testsDir);
  if (!existsSync(testsDir)) return out;
  let dirents;
  try {
    dirents = readdirSync(testsDir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const d of dirents) {
    if (!d.isDirectory()) continue;
    const nestedMain = join(testsDir, d.name, "main.rs");
    if (existsSync(nestedMain)) {
      out.push(nestedMain);
    }
  }
  return out;
}

function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    process.stdout.write(HELP);
    process.exit(0);
  }
  const jsonOut = args.includes("--json");
  for (const a of args) {
    if (a !== "--json" && a !== "--help" && a !== "-h") {
      process.stderr.write(`unknown argument: ${a}\n\n${HELP}`);
      process.exit(2);
    }
  }

  const allowlist = loadAllowlist();
  const metadata = runCargoMetadata();

  // Index: which workspace packages exist, and their (package -> manifest dir).
  const wsMembers = new Set(metadata.workspace_members ?? []);
  const packages = (metadata.packages ?? []).filter((p) => wsMembers.has(p.id));

  // failures: array of { package, message }.
  const failures = [];

  // Track which allowlist entries got matched against a real metadata target
  // (for the stale-failing check). Keyed `${package}::${target}`.
  const allowKey = (pkgName, target) => `${pkgName}::${target}`;
  const matchedAllow = new Set();

  // Build a quick lookup of allowlist entries by (package,target).
  const allowByKey = new Map();
  for (const e of allowlist) {
    allowByKey.set(allowKey(e.package, e.target), e);
  }

  for (const pkg of packages) {
    const pkgName = pkg.name;
    const manifestDir = dirname(pkg.manifest_path);
    const manifestDirPosix = toPosix(manifestDir);
    const expectedMainSrcPosix = `${manifestDirPosix}/tests/main.rs`;
    const testsDir = join(manifestDir, "tests");

    // Integration-test targets reported by cargo metadata (kind includes "test").
    const testTargets = (pkg.targets ?? []).filter(
      (t) => Array.isArray(t.kind) && t.kind.includes("test"),
    );

    // ---- Exactly ONE tests/main.rs binary. Two [[test]] blocks both
    //      `path = "tests/main.rs"` make cargo metadata report TWO targets with
    //      identical src; each individually `continue`s on the sanctioned-main
    //      path below, so without this count the second compiled binary slips by.
    const mainTargets = testTargets.filter((t) => toPosix(t.src_path) === expectedMainSrcPosix);
    if (mainTargets.length > 1) {
      const names = mainTargets.map((t) => t.name).join(", ");
      failures.push({
        package: pkgName,
        message:
          `package \`${pkgName}\` has ${mainTargets.length} tests/main.rs ` +
          `integration-test targets (${names}) — exactly one tests/main.rs binary is ` +
          `allowed; a second [[test]] pointing at tests/main.rs still compiles a ` +
          `separate binary.`,
      });
    }

    // ---- Rule on metadata test targets: each must be tests/main.rs OR allowlisted.
    for (const t of testTargets) {
      const srcPosix = toPosix(t.src_path);
      const repoRelSrc = toRepoRelPosix(t.src_path);
      if (srcPosix === expectedMainSrcPosix) {
        continue; // the one sanctioned consolidated target
      }
      // Not tests/main.rs -> must be EXACTLY allowlisted (package + target + src_path).
      const entry = allowByKey.get(allowKey(pkgName, t.name));
      if (!entry) {
        failures.push({
          package: pkgName,
          message:
            `integration-test target \`${t.name}\` (src ${repoRelSrc}) is not ` +
            `tests/main.rs and is not allowlisted. Consolidate it into ` +
            `${toRepoRelPosix(join(testsDir, "main.rs"))} (e.g. under tests/cases/), ` +
            `or add an exact allowlist entry to ` +
            `${toRepoRelPosix(ALLOWLIST_PATH)} if it genuinely needs a separate ` +
            `test process.`,
        });
        continue;
      }
      // Allowlisted by (package,target): verify src_path matches exactly (stale-failing on move).
      if (entry.src_path !== repoRelSrc) {
        failures.push({
          package: pkgName,
          message:
            `allowlisted target \`${t.name}\` src_path moved: allowlist expects ` +
            `\`${entry.src_path}\` but cargo metadata reports \`${repoRelSrc}\`. ` +
            `Update ${toRepoRelPosix(ALLOWLIST_PATH)} to the new path (the exact ` +
            `exception must track the real binary).`,
        });
        // Still mark matched so we don't ALSO report it stale below.
      }
      matchedAllow.add(allowKey(pkgName, t.name));
    }

    // ---- GOV-D4 (1): if tests/main.rs exists on disk, metadata MUST contain it.
    const mainRsAbs = join(testsDir, "main.rs");
    if (existsSync(mainRsAbs)) {
      const hasMainTarget = testTargets.some((t) => toPosix(t.src_path) === expectedMainSrcPosix);
      if (!hasMainTarget) {
        failures.push({
          package: pkgName,
          message:
            `${toRepoRelPosix(mainRsAbs)} exists on disk but cargo metadata does ` +
            `NOT report a tests/main.rs integration-test target — a missing or ` +
            `misconfigured [[test]] / autotests setting is hiding it.`,
        });
      }
    }

    // ---- GOV-D4 (2): tests/*.rs present immediately but ZERO metadata test targets
    //                  => autotests = false (or misconfig) is hiding tests.
    const immediate = immediateTestRsFiles(testsDir);
    if (immediate.length > 0 && testTargets.length === 0) {
      const names = immediate.map((p) => toRepoRelPosix(p)).sort();
      failures.push({
        package: pkgName,
        message:
          `${names.length} immediate tests/*.rs file(s) exist (${names.join(", ")}) ` +
          `but cargo metadata reports ZERO integration-test targets — \`autotests = ` +
          `false\` (or an equivalent misconfig) is hiding compiled test binaries.`,
      });
    }

    // ---- GOV-D4 (4): stray IMMEDIATE tests/*.rs other than main.rs / allowlisted src.
    // (Files under tests/cases/ or any subdir are fine — only the immediate level.)
    const allowedImmediateSrcRel = new Set([toRepoRelPosix(mainRsAbs)]);
    for (const e of allowlist) {
      if (e.package === pkgName) {
        // Only treat an allowlist src as "allowed immediate" if it lives directly
        // under this package's tests/ (the immediate level the rule constrains).
        const eAbs = resolve(REPO_ROOT, e.src_path);
        if (toPosix(dirname(eAbs)) === toPosix(testsDir)) {
          allowedImmediateSrcRel.add(e.src_path);
        }
      }
    }
    for (const fileAbs of immediate) {
      const rel = toRepoRelPosix(fileAbs);
      if (allowedImmediateSrcRel.has(rel)) continue;
      failures.push({
        package: pkgName,
        message:
          `stray immediate test file ${rel} — only tests/main.rs (plus exactly ` +
          `allowlisted files) may live at the top tests/*.rs level, because each ` +
          `such file auto-becomes its own test binary. Move it under tests/cases/ ` +
          `(or another subdirectory) and wire it through tests/main.rs.`,
      });
    }

    // ---- HIDDEN AUTO-DISCOVERABLE BINARY: every cargo-auto-discoverable position
    //      (tests/*.rs and tests/<dir>/main.rs one subdir deep) must correspond to
    //      a reported metadata target. A candidate WITHOUT a matching target is a
    //      binary cargo compiles but metadata does not report (the autotests=false
    //      hiding case). This fires PER CANDIDATE even when the package has OTHER
    //      metadata targets, so it catches a hidden tests/rogue/main.rs next to a
    //      valid tests/main.rs (which the zero-targets GOV-D4(2) rule cannot).
    const reportedSrcs = new Set(testTargets.map((t) => toPosix(t.src_path)));
    for (const candAbs of autoDiscoverableTestCandidates(testsDir)) {
      const candPosix = toPosix(candAbs);
      if (reportedSrcs.has(candPosix)) continue;
      failures.push({
        package: pkgName,
        message:
          `${toRepoRelPosix(candAbs)} is a cargo-auto-discoverable integration-test ` +
          `position (tests/*.rs or tests/<dir>/main.rs) but cargo metadata reports no ` +
          `integration-test target for it — \`autotests = false\` (or an explicit ` +
          `[[test]] that omits it) is hiding a separately-compiled test binary. Wire ` +
          `it through tests/main.rs (e.g. as a module under tests/cases/) or remove it.`,
      });
    }
  }

  // ---- STALE-FAILING: every allowlist entry must have matched a real target.
  for (const e of allowlist) {
    const key = allowKey(e.package, e.target);
    if (!matchedAllow.has(key)) {
      // Distinguish "package missing" from "target missing" for a clearer message.
      const pkgExists = packages.some((p) => p.name === e.package);
      const reasonText = pkgExists
        ? `cargo metadata reports no integration-test target named \`${e.target}\` ` +
          `for package \`${e.package}\` (it was removed or renamed)`
        : `package \`${e.package}\` is not a workspace member in cargo metadata`;
      failures.push({
        package: e.package,
        message:
          `STALE allowlist entry: ${reasonText}. Remove the dead exception from ` +
          `${toRepoRelPosix(ALLOWLIST_PATH)} (an allowlisted binary that no longer ` +
          `exists must not leave a lingering exception).`,
      });
    }
  }

  const ok = failures.length === 0;

  if (jsonOut) {
    process.stdout.write(`${JSON.stringify({ ok, failures, allowlist }, null, 2)}\n`);
    process.exit(ok ? 0 : 1);
  }

  if (ok) {
    process.stdout.write(
      `integration-test layout OK — every workspace package has at most one ` +
        `tests/main.rs integration-test target (plus ${allowlist.length} exact ` +
        `allowlisted exception(s)).\n`,
    );
    process.exit(0);
  }

  // RED: clear per-package failure list.
  const byPackage = new Map();
  for (const f of failures) {
    if (!byPackage.has(f.package)) byPackage.set(f.package, []);
    byPackage.get(f.package).push(f.message);
  }
  let report = `integration-test layout VIOLATION — ${failures.length} problem(s) across ${byPackage.size} package(s):\n\n`;
  for (const [pkgName, msgs] of [...byPackage.entries()].sort()) {
    report += `  ${pkgName}:\n`;
    for (const m of msgs) {
      report += `    - ${m}\n`;
    }
  }
  report +=
    `\nThe consolidation rule: each crate exposes AT MOST one tests/main.rs ` +
    `integration-test binary (extra cases live under tests/cases/ and are wired ` +
    `through main.rs). A new top-level tests/*.rs auto-becomes a separate binary ` +
    `and is forbidden unless exactly allowlisted in ` +
    `${toRepoRelPosix(ALLOWLIST_PATH)}.\n`;
  process.stderr.write(report);
  process.exit(1);
}

try {
  main();
} catch (err) {
  process.stderr.write(`check-integration-test-layout: ${err.message}\n`);
  process.exit(2);
}
