/**
 * Fixture dependency installation for the E2E launchers.
 *
 * A fixture's `node_modules` is gitignored and lives inside the repository, so it
 * is not staged, not cleaned between runs, and invisible to `git status`. The
 * launchers used to skip installing whenever that directory merely EXISTED, which
 * made "installed once, ever" indistinguishable from "installed from this
 * manifest". A package installed months ago therefore survived every subsequent
 * run and could decide test outcomes — a four-month-old `@verter/types` inside
 * one fixture shadowed the workspace package and decided eight of them.
 *
 * Existence is replaced by PROVENANCE, recorded in a stamp beside the tree and
 * covering two independent things:
 *
 *   - the MANIFEST the install was produced from, so editing `package.json`
 *     reinstalls; and
 *   - the INSTALLED TREE that install produced, so a package added, removed,
 *     replaced, or re-pointed inside `node_modules` WITHOUT touching the manifest
 *     is still detected. A manifest hash alone would miss exactly the shape the
 *     original incident took, one level down.
 *
 * A fixture's `node_modules` is harness-owned generated infrastructure — but the
 * harness stops using a tree it cannot prove it produced WITHOUT irreversibly
 * deleting it. Four dispositions, and no others:
 *
 *   - STAMPED AND MATCHING. Reused with no mutation at all.
 *   - STAMPED, MANIFEST-ONLY CHANGE. The tree still matches the digest recorded
 *     for it, so it is provably this module's own output. It is renamed aside,
 *     the fixture is installed clean, validated and stamped, and only then is the
 *     predecessor deleted. A failed install puts it back.
 *   - ANYTHING ELSE ON DISK. An unstamped tree, a legacy stamp, or a tree that
 *     changed under a valid stamp is MOVED to a persistent quarantine outside
 *     every fixture workspace, its absolute recovery path is reported, and the
 *     fixture is installed clean. Never adopted, never merged, never deleted.
 *     Quarantines go only when someone removes them, and only through the
 *     command that can prove which directories this harness created.
 *   - UNDECIDABLE. An unreadable current-format stamp, a fingerprint that cannot
 *     be taken, a `node_modules` that is not a tree, or a move that fails: the
 *     run REFUSES, having mutated nothing.
 *
 * Replaced, not merged, in every case: `npm install` over a wrong tree reconciles
 * the packages the manifest still lists and leaves the ones it dropped, which is
 * the failure mode this exists to end. And moved, not deleted, because a fixture
 * `node_modules` is gitignored and invisible to `git status`, so anything a
 * developer put there by hand is unrecoverable once removed — while the harness
 * cannot tell that tree from one an old run left behind.
 *
 * Displacing is DESTRUCTIVE, so the decide → displace → install → stamp sequence
 * runs under an exclusive cross-process lock (`fixtureLock.ts`) and RE-DECIDES
 * once it owns one. Without that, two processes that both decided to install
 * would race, and one would displace the tree the other was still creating.
 *
 * The Rust real-provider harness solves the staleness half by never reading the
 * accumulating directory at all (`crates/verter_lsp/src/test_harness_fixture_dependencies.rs`
 * stages the fixture into a per-process copy). That is not available here: VS
 * Code opens the fixture path itself, and the in-tree fixtures reach the
 * repository through relative paths (`"verter": "file:../../../../.."`) a temp
 * copy would break. Its OWNERSHIP rules do apply, and `fixtureLock.ts` follows
 * them.
 */

import { createHash, randomUUID } from "node:crypto";
import { execSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";

import { withFixtureLock, type FixtureLockOptions } from "./fixtureLock";
import { resolveE2eFixtureSelection } from "./routeInventory";

/** Records what the installed tree was produced from. */
const STAMP_BASENAME = ".verter-e2e-install.json";

/** A stamp digest, so a malformed one is distinguishable from a legacy one. */
const DIGEST = /^[0-9a-f]{64}$/;

/**
 * The harness cannot decide what to do without destroying something it may not
 * own, so it does nothing and says why.
 *
 * A distinct type because "refused" and "the install failed" call for different
 * responses: the first is a state on disk for a person to resolve, the second is
 * a retry.
 */
export class FixtureDepsRefusal extends Error {
  constructor(message: string) {
    super(`refusing to touch fixture dependencies: ${message}`);
    this.name = "FixtureDepsRefusal";
  }
}

/** Dependency specs that must resolve to a live link rather than a copy. */
const LOCAL_SPEC = /^(file|link):/;

/** Why {@link decideFixtureInstall} reached its verdict. Reported, so a run says which. */
export type FixtureInstallReason =
  /** No `package.json` — nothing to install, and no tree to distrust. */
  | "no-manifest"
  /** Never installed here. */
  | "no-node-modules"
  /** A tree with no provenance: predates the stamp, or was written by something else. */
  | "unstamped-tree"
  /** Stamped, but from a different manifest than the one on disk now. */
  | "manifest-changed"
  /** Stamped, manifest unchanged, but the installed tree is no longer the one stamped. */
  | "tree-changed"
  /** Stamped from exactly this manifest, and the tree is still the one installed. */
  | "current"
  /** The deliberate developer override: an existing tree used as-is, unstamped. */
  | "adopted";

export interface FixtureInstallDecision {
  readonly install: boolean;
  readonly reason: FixtureInstallReason;
  /** Absolute path of the displaced tree, when one was quarantined. */
  readonly quarantined?: string;
}

/**
 * What happens to the tree already on disk.
 *
 * `rollback` is reserved for a tree this module can PROVE it produced: stamped,
 * with the tree still matching the digest recorded for it. Everything else keeps
 * its tree.
 */
export type FixtureTreeDisposition = "none" | "rollback" | "quarantine";

export function fixtureTreeDisposition(reason: FixtureInstallReason): FixtureTreeDisposition {
  if (reason === "manifest-changed") return "rollback";
  if (reason === "unstamped-tree" || reason === "tree-changed") return "quarantine";
  return "none";
}

/** How a fixture's dependencies get installed. Injected so the decision is testable. */
export type FixtureInstaller = (fixtureDir: string) => void;

export interface InstallFixtureDepsOptions {
  /** Override the installer. Tests inject; production uses npm. */
  readonly install?: FixtureInstaller;
  /** Passed through to the cross-process lock. */
  readonly lock?: FixtureLockOptions;
  /** Where displaced trees are kept. Defaults to {@link fixtureQuarantineRoot}. */
  readonly quarantineRoot?: string;
  /**
   * Fixture directory names whose existing tree may be used as-is. Developer
   * override; defaults to `VERTER_E2E_ADOPT_FIXTURE_DEPS` (comma-separated).
   */
  readonly adoptFixtures?: readonly string[];
  /** Defaults to the `CI`/`GITHUB_ACTIONS` environment. */
  readonly continuousIntegration?: boolean;
}

/** Where displaced trees are kept, relative to the repository root. */
const QUARANTINE_DIRNAME = ".verter-e2e-quarantine";

/** What a rollback's holding directory is called, beside the tree it holds. */
const ROLLBACK_HOLDING_PREFIX = "node_modules.verter-rollback-";

/**
 * What makes a directory identifiable as this harness's, at both levels.
 *
 * The cleanup command deletes directories, so it must be able to prove it is
 * removing something this module put there rather than acting on the strength of
 * a path somebody typed. Written by the code that creates each one, and read by
 * nothing else.
 */
const QUARANTINE_ROOT_MARKER = ".verter-e2e-quarantine-root";
const QUARANTINE_ENTRY_MARKER = ".verter-e2e-quarantined.json";

/**
 * The repository this harness belongs to.
 *
 * Walked up to rather than counted in `..` segments, because this file is loaded
 * from `e2e/lib/` as source and from `out-test/e2e/lib/` once compiled: a fixed
 * relative path is correct for exactly one of them, and silently names
 * `packages/` for the other.
 */
function repositoryRoot(): string {
  let directory = __dirname;
  for (;;) {
    if (fs.existsSync(path.join(directory, "pnpm-workspace.yaml"))) return directory;
    const parent = path.dirname(directory);
    if (parent === directory) {
      throw new FixtureDepsRefusal(
        `cannot find the repository root above ${__dirname}: no ancestor holds a ` +
          `pnpm-workspace.yaml. Displaced dependency trees are kept inside the repository, on ` +
          `the same device as the fixture they came from, and this harness cannot say where ` +
          `that is.`,
      );
    }
    directory = parent;
  }
}

/**
 * Where displaced trees are kept.
 *
 * A gitignored directory at the repository root. Three things decide that, and
 * each of them ruled out somewhere else:
 *
 *   - SAME DEVICE. A tree is displaced by renaming it, and a rename cannot cross
 *     filesystems. Anchored to the system temp directory that is a property of
 *     the machine: on Linux with a tmpfs `/tmp` and the repository on `/home`,
 *     or on Windows with the repository on `D:` and `%TEMP%` on `C:`, the first
 *     run to displace anything refuses with EXDEV and nothing runs until someone
 *     sets an environment variable. Inside the repository it is the same device
 *     as any fixture that is also inside it — which is every fixture the harness
 *     installs IN PLACE, and not every workspace it installs into. `runTests.ts`
 *     materializes the out-of-tree fixture into a fresh `mkdtemp` under the
 *     system temp root and installs its packages there, and nothing makes that
 *     the repository's device.
 *
 *     No rename is reachable from there today, though for a narrower reason than
 *     "the copy has no `node_modules`": the copy filter excludes that literal
 *     name only, so a `node_modules.verter-rollback-*` in the template WOULD be
 *     copied and recovered from the copy. What rules it out is that nothing
 *     creates one there — no launcher installs into those template package
 *     directories in place — and a fresh copy's first install finds no tree to
 *     displace. If either ever changes, the rename fails with EXDEV and both
 *     refusals name the environment variable that moves the quarantine, which is
 *     deliberately the whole mitigation: a loud failure carrying its own
 *     instruction beats machinery for a case that does not occur.
 *   - OUTSIDE EVERY FIXTURE. Node resolves `node_modules` by appending it to a
 *     file's ancestors, so neither a sibling of a fixture nor a dot-directory
 *     inside one is ever a resolution candidate — but a fixture is also opened
 *     as a WORKSPACE, and `monorepo` opens the parent of the directories it
 *     installs, so a quarantine beside an installed directory lands inside a
 *     workspace under test. The repository root is inside none of them.
 *   - NOT RECLAIMED. A quarantine is the only copy of a tree its owner may still
 *     want, and a temp directory is swept by the OS on a schedule nobody here
 *     controls. The one exposure left is `git clean -xdf`, which is somebody
 *     deciding to delete untracked files.
 *
 * `VERTER_E2E_FIXTURE_QUARANTINE_DIR` still overrides it, for a machine that
 * wants quarantines somewhere else — it must be on the repository's device, or
 * the rename that fills it cannot work.
 */
export function fixtureQuarantineRoot(): string {
  const configured = process.env.VERTER_E2E_FIXTURE_QUARANTINE_DIR;
  if (configured) return path.resolve(configured);
  return path.join(repositoryRoot(), QUARANTINE_DIRNAME);
}

/** The manifest bytes an install was produced from. */
function manifestFingerprint(manifestPath: string): string {
  return createHash("sha256").update(fs.readFileSync(manifestPath)).digest("hex");
}

/**
 * A fingerprint of the installed tree: every package's identity, in a stable
 * order.
 *
 * Per entry it records the `package.json` BYTES for a real directory, the LINK
 * TARGET for a symlink, and the CONTENT for a plain file. A symlink is never
 * followed — `node_modules/verter` points at the repository root, so descending
 * would walk the entire workspace, and re-hashing the target's manifest would
 * reinstall the fixture every time an unrelated repo version changed. What
 * matters about a link is where it points.
 *
 * A plain file counts because Node resolves one FIRST: `LOAD_AS_FILE` runs
 * before `LOAD_AS_DIRECTORY`, so `node_modules/vue.js` is what `require("vue")`
 * returns even with `node_modules/vue/` sitting beside it. Skipping non-
 * directories would have left the module's own claim — that a package added
 * without touching the manifest is still detected — false for the one shape that
 * wins resolution.
 *
 * Dot-entries are skipped, which excludes the stamp itself (it is written after
 * the fingerprint is taken and must not perturb it) along with `.bin` and npm's
 * own hidden lockfile.
 *
 * WHAT THIS DELIBERATELY DOES NOT SEE, and what covers it instead: a `file:`
 * dependency is a live link into the repository, so rebuilding the repository's
 * own `dist` behind it changes what the fixture resolves without moving this
 * digest at all. That is correct and intended — re-hashing through the link
 * would reinstall every fixture on every repository edit — and it is sound ONLY
 * because the dependency really is a link. npm's `install-links` decides that
 * and is env-overridable, so {@link npmInstall} passes `--install-links=false`
 * explicitly and {@link assertLocalDepsAreLinked} verifies the outcome. The
 * freshness of a `file:` dependency rests on THOSE two, not on this function; if
 * either goes, this digest does not catch it.
 */
export function installedTreeFingerprint(nodeModules: string): string {
  const records: string[] = [];

  const visitPackage = (packageDir: string, id: string): void => {
    let stat: fs.Stats;
    try {
      stat = fs.lstatSync(packageDir);
    } catch {
      return;
    }
    if (stat.isSymbolicLink()) {
      let target = "<unreadable>";
      try {
        target = fs.readlinkSync(packageDir);
      } catch {
        /* keep the placeholder */
      }
      records.push(`${id} -> ${target.split(path.sep).join("/")}`);
      return;
    }
    if (!stat.isDirectory()) {
      // A bare module file, which Node resolves ahead of a package directory of
      // the same name. Its CONTENT is what gets loaded, so its content is what
      // identifies it.
      try {
        records.push(
          `${id}#${createHash("sha256").update(fs.readFileSync(packageDir)).digest("hex")}`,
        );
      } catch {
        records.push(`${id}#<unreadable>`);
      }
      return;
    }

    try {
      records.push(
        `${id}@${createHash("sha256")
          .update(fs.readFileSync(path.join(packageDir, "package.json")))
          .digest("hex")}`,
      );
    } catch {
      records.push(`${id}@<no-manifest>`);
    }
    // Nested installs only; never into a package's own sources.
    visitTree(path.join(packageDir, "node_modules"), `${id}/node_modules`);
  };

  const visitTree = (tree: string, prefix: string): void => {
    let entries: fs.Dirent[];
    try {
      entries = fs.readdirSync(tree, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      if (entry.name.startsWith(".")) continue;
      const id = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.name.startsWith("@") && entry.isDirectory()) {
        let scoped: fs.Dirent[];
        try {
          scoped = fs.readdirSync(path.join(tree, entry.name), { withFileTypes: true });
        } catch {
          continue;
        }
        for (const inner of scoped) {
          if (inner.name.startsWith(".")) continue;
          visitPackage(path.join(tree, entry.name, inner.name), `${id}/${inner.name}`);
        }
        continue;
      }
      visitPackage(path.join(tree, entry.name), id);
    }
  };

  visitTree(nodeModules, "");
  records.sort();
  return createHash("sha256").update(records.join("\n")).digest("hex");
}

/**
 * Whether `fixtureDir` needs an install, and why.
 *
 * Reads only — a caller that wants to report the plan without acting can. Callers
 * that act must hold the fixture lock, because the answer stops being true the
 * moment another process installs.
 */
export function decideFixtureInstall(fixtureDir: string): FixtureInstallDecision {
  const manifest = path.join(fixtureDir, "package.json");
  if (!fs.existsSync(manifest)) return { install: false, reason: "no-manifest" };

  const nodeModules = path.join(fixtureDir, "node_modules");
  if (!fs.existsSync(nodeModules)) return { install: true, reason: "no-node-modules" };

  const stamp = readStamp(path.join(nodeModules, STAMP_BASENAME));
  // No provenance is not the same as unreadable provenance. Debris and legacy
  // stamps say "this tree is not evidence", which the quarantine answers; a
  // CURRENT-FORMAT stamp that cannot be used says "the harness cannot tell what
  // this tree is", and guessing there costs a tree it may not own.
  if (stamp.kind === "unreadable") {
    throw new FixtureDepsRefusal(
      `the install stamp in ${nodeModules} is this format but cannot be used (${stamp.detail}). ` +
        `Delete or repair ${path.join(nodeModules, STAMP_BASENAME)} to say what this tree is.`,
    );
  }
  if (stamp.kind !== "current") return { install: true, reason: "unstamped-tree" };

  // The TREE is checked first, and it decides. A tree that changed is not this
  // module's output whatever its manifest says, so it is kept; only a tree that
  // still matches its digest can be proven disposable.
  if (stamp.tree !== treeFingerprintOrRefuse(nodeModules)) {
    return { install: true, reason: "tree-changed" };
  }
  if (stamp.manifest !== manifestFingerprintOrRefuse(manifest)) {
    return { install: true, reason: "manifest-changed" };
  }
  return { install: false, reason: "current" };
}

type StampRead =
  | { kind: "absent" }
  /** Present, but not something this module wrote: debris, or a previous format. */
  | { kind: "legacy" }
  | { kind: "unreadable"; detail: string }
  | { kind: "current"; manifest: string; tree: string };

function readStamp(stampPath: string): StampRead {
  let text: string;
  try {
    text = fs.readFileSync(stampPath, "utf-8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return { kind: "absent" };
    return { kind: "unreadable", detail: describeError(error) };
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    // Not JSON at all: debris rather than a stamp of this format. Quarantining
    // is the non-destructive answer, so classifying it as legacy costs nothing.
    return { kind: "legacy" };
  }
  if (!parsed || typeof parsed !== "object") return { kind: "legacy" };
  const { manifest, tree } = parsed as { manifest?: unknown; tree?: unknown };
  if (typeof manifest !== "string" || typeof tree !== "string") return { kind: "legacy" };
  if (!DIGEST.test(manifest) || !DIGEST.test(tree)) {
    return { kind: "unreadable", detail: "manifest/tree are not sha256 digests" };
  }
  return { kind: "current", manifest, tree };
}

function describeError(error: unknown): string {
  const errno = error as NodeJS.ErrnoException;
  return errno?.code ? `${errno.code}: ${errno.message}` : String(errno?.message ?? error);
}

function manifestFingerprintOrRefuse(manifestPath: string): string {
  try {
    return manifestFingerprint(manifestPath);
  } catch (error) {
    throw new FixtureDepsRefusal(`cannot read ${manifestPath} (${describeError(error)}).`);
  }
}

/**
 * The tree digest, or a refusal.
 *
 * A fingerprint that cannot be taken is undecidable, not "changed": treating an
 * unreadable directory as a mismatch would displace a tree on the strength of an
 * IO error. The top-level read is the one that must succeed — deeper failures
 * are RECORDED in the digest (`<no-manifest>`, `<unreadable>`) rather than
 * skipped, so they still discriminate.
 */
function treeFingerprintOrRefuse(nodeModules: string): string {
  try {
    fs.readdirSync(nodeModules);
  } catch (error) {
    throw new FixtureDepsRefusal(
      `cannot read the installed tree at ${nodeModules} (${describeError(error)}).`,
    );
  }
  return installedTreeFingerprint(nodeModules);
}

/**
 * Refuse before touching anything when a declared `file:`/`link:` target is not
 * on this machine.
 *
 * `ecosystem-parity` declares `"verter": "file:../../../../../../verter-release-clean"`
 * — a sibling checkout that simply is not there on most machines. The launcher
 * used to catch npm's failure and warn, which meant the suite ran on against a
 * tree missing the package under test; failing closed is right, but "npm exited
 * 1" names neither what is missing nor what to do about it. Checking first also
 * means a fixture that cannot possibly install does not get its existing tree
 * displaced on the way to finding that out.
 */
function assertLocalDepTargetsExist(fixtureDir: string): void {
  const manifestPath = path.join(fixtureDir, "package.json");
  for (const [name, spec] of localDependencySpecs(manifestPath)) {
    const target = path.resolve(fixtureDir, spec.replace(LOCAL_SPEC, ""));
    if (fs.existsSync(target)) continue;
    throw new FixtureDepsRefusal(
      `${fixtureDir}/package.json declares "${name}": "${spec}", and that target does not ` +
        `exist:\n    ${target}\n  npm cannot install this fixture until it does. Check out or ` +
        `build whatever should be at that path, or change the dependency to point at what this ` +
        `machine actually has. The run stops here rather than continuing against a fixture ` +
        `missing the very package it exists to exercise.`,
    );
  }
}

/** Local (`file:` / `link:`) dependency names declared by a fixture's manifest. */
function localDependencyNames(manifestPath: string): string[] {
  return localDependencySpecs(manifestPath).map(([name]) => name);
}

/** Local (`file:` / `link:`) dependency entries declared by a fixture's manifest. */
function localDependencySpecs(manifestPath: string): Array<[string, string]> {
  let manifest: Record<string, unknown>;
  try {
    manifest = JSON.parse(fs.readFileSync(manifestPath, "utf-8")) as Record<string, unknown>;
  } catch {
    return [];
  }
  const declared: Array<[string, string]> = [];
  for (const field of ["dependencies", "devDependencies", "optionalDependencies"]) {
    const block = manifest[field];
    if (!block || typeof block !== "object") continue;
    for (const [name, spec] of Object.entries(block as Record<string, unknown>)) {
      if (typeof spec === "string" && LOCAL_SPEC.test(spec)) declared.push([name, spec]);
    }
  }
  return declared;
}

/**
 * Fail unless every local dependency was LINKED rather than copied.
 *
 * A fixture that depends on the repository through `file:` is only meaningful if
 * the installed package is a live view of the repository. npm decides that with
 * `install-links`, which defaults to linking but is overridable through
 * `NPM_CONFIG_INSTALL_LINKS`. {@link npmInstall} passes the flag explicitly so the
 * default cannot be moved out from under the suite; this verifies the flag did
 * what it claims, because a COPY would freeze the dependency at install time and
 * no later repository change would ever reinstall it — the stale-tree problem
 * again, in the one place the stamp cannot see.
 */
export function assertLocalDepsAreLinked(fixtureDir: string): void {
  for (const name of localDependencyNames(path.join(fixtureDir, "package.json"))) {
    const installed = path.join(fixtureDir, "node_modules", ...name.split("/"));
    let stat: fs.Stats;
    try {
      stat = fs.lstatSync(installed);
    } catch {
      throw new Error(
        `local dependency "${name}" declared by ${fixtureDir}/package.json is not installed at ` +
          `${installed}. A file:/link: dependency must resolve to a link into the repository.`,
      );
    }
    if (!stat.isSymbolicLink()) {
      throw new Error(
        `local dependency "${name}" was COPIED into ${installed} instead of linked. The fixture ` +
          `would then be pinned to the repository as it was at install time, and no later ` +
          `repository change would reinstall it. Check NPM_CONFIG_INSTALL_LINKS / .npmrc — ` +
          `npm is invoked with --install-links=false precisely to prevent this.`,
      );
    }
  }
}

/** A tree moved out of a fixture, and what is to become of it. */
interface DisplacedTree {
  readonly from: string;
  readonly to: string;
  readonly holding: string;
  readonly disposition: "rollback" | "quarantine";
}

/** A holding directory name that says which fixture, why, and when. */
function holdingName(fixtureDir: string, reason: string): string {
  const when = new Date().toISOString().replace(/[:.]/g, "-");
  return `${path.basename(path.resolve(fixtureDir))}-${reason}-${when}-${randomUUID().slice(0, 8)}`;
}

/**
 * Create a marked holding directory under a marked quarantine root.
 *
 * Both markers exist for {@link cleanFixtureQuarantine}, which deletes
 * directories and so must be able to prove that each one is this harness's own.
 * They are written BEFORE the tree is moved in, so an interrupted displace
 * leaves an empty directory the cleanup command can still collect rather than an
 * unidentifiable one it has to leave forever.
 */
function makeQuarantineHolding(quarantineRoot: string, fixtureDir: string, reason: string): string {
  const holding = path.join(quarantineRoot, holdingName(fixtureDir, reason));
  fs.mkdirSync(holding, { recursive: true });
  fs.writeFileSync(
    path.join(quarantineRoot, QUARANTINE_ROOT_MARKER),
    `Dependency trees the Verter E2E fixture harness moved out of a fixture.\n` +
      `Removed only by: pnpm --filter verter-vscode test:e2e:fixtures:clean-quarantine\n`,
  );
  fs.writeFileSync(
    path.join(holding, QUARANTINE_ENTRY_MARKER),
    `${JSON.stringify({ fixture: path.resolve(fixtureDir), reason, displacedAt: new Date().toISOString() }, null, 2)}\n`,
  );
  return holding;
}

/**
 * Move a fixture's `node_modules` out of the way, and nothing else.
 *
 * This is the one place the harness relocates a directory inside the repository.
 * The path is derived here rather than accepted, it is only reached after
 * {@link decideFixtureInstall} has found a `package.json` beside it, and only
 * while this process owns the fixture lock. The authored tree next to it is
 * untouched; `fixtureDeps.unit.test.ts` asserts a sibling survives.
 *
 * The two dispositions go to different places, because they are different
 * things:
 *
 *   - a QUARANTINE is kept indefinitely for whoever owns that tree, so it goes
 *     to the persistent root, out of every fixture and every workspace.
 *   - a ROLLBACK is this transaction's own undo buffer, alive for the length of
 *     one install and then either restored or deleted. It goes to a sibling of
 *     the tree it holds — the same directory, so the rename cannot cross a
 *     device under any configuration — and it borrows nothing from the
 *     persistent root, whose location an operator can point anywhere. Putting a
 *     fixture back the way it was must not be able to fail because of a setting
 *     that has nothing to do with it.
 *
 * SAFETY, because it is not obvious and must not be re-derived by the next
 * reader: eight fixtures declare `"verter": "file:../../../../.."`, so every
 * tree moved here contains a symlink to the REPOSITORY ROOT. `rename` moves a
 * directory ENTRY — it cannot descend into the tree, let alone follow a link out
 * of it — and the later delete of a proven predecessor uses `rmSync`, which
 * unlinks a symlink rather than following it (verified on Node 26 against a
 * canary tree; `fixtureDeps.unit.test.ts` asserts both directions).
 */
function displaceInstalledTree(
  fixtureDir: string,
  reason: FixtureInstallReason,
  disposition: "rollback" | "quarantine",
  quarantineRoot: string,
): DisplacedTree {
  const from = path.join(fixtureDir, "node_modules");
  const stat = fs.lstatSync(from);
  if (!stat.isDirectory() || stat.isSymbolicLink()) {
    throw new FixtureDepsRefusal(
      `${from} is not a directory this harness can own (it is a ${
        stat.isSymbolicLink() ? "symlink" : "non-directory"
      }). Installing over it would write through it into a tree nobody declared.`,
    );
  }

  const rollback = disposition === "rollback";
  let holding: string;
  try {
    holding = rollback
      ? path.join(fixtureDir, `${ROLLBACK_HOLDING_PREFIX}${randomUUID().slice(0, 8)}`)
      : makeQuarantineHolding(quarantineRoot, fixtureDir, reason);
  } catch (error) {
    throw new FixtureDepsRefusal(
      `cannot prepare a quarantine directory under ${quarantineRoot} ` +
        `(${describeError(error)}). Nothing has been changed.`,
    );
  }
  // A rollback's holding directory IS the tree: one rename out, one back, and
  // nothing left over to remove or to describe wrongly if removing it fails.
  const to = rollback ? holding : path.join(holding, "node_modules");
  try {
    fs.renameSync(from, to);
  } catch (error) {
    throw new FixtureDepsRefusal(
      `cannot move ${from} aside to ${to} (${describeError(error)}).` +
        (rollback
          ? ` That is a sibling of the tree itself, so this is not a cross-device move.`
          : ` A rename cannot cross filesystems: if the quarantine root is on a different ` +
            `device from the repository, set VERTER_E2E_FIXTURE_QUARANTINE_DIR to a path on ` +
            `the same one.`) +
        ` Nothing has been changed.`,
    );
  }
  return { from, to, holding, disposition };
}

/**
 * Whether anything of the holding shape is in this fixture at all.
 *
 * A look, not a decision: what it finds may belong to a LIVE transaction. It
 * exists so a caller that would otherwise take the lock to do nothing does not
 * take it, and every caller re-reads under the lock before touching anything.
 */
function hasRollbackHolding(fixtureDir: string): boolean {
  try {
    return fs.readdirSync(fixtureDir).some((entry) => entry.startsWith(ROLLBACK_HOLDING_PREFIX));
  } catch {
    return false;
  }
}

/**
 * Recover a holding directory a killed transaction left behind.
 *
 * A rollback's undo buffer only exists between the displace and the restore, and
 * only while this process holds the lock — so a `node_modules.verter-rollback-*`
 * found HERE, under the lock, is by construction a predecessor whose transaction
 * died. Which makes it the fixture's real dependency tree, sitting in a
 * directory nothing will ever look in again.
 *
 * It is moved to the quarantine and reported, never deleted and never adopted:
 * the harness cannot tell what state that tree was in when its transaction died,
 * and this is the same answer it gives to every other tree it cannot prove.
 */
function recoverAbandonedRollbackHoldings(fixtureDir: string, quarantineRoot: string): void {
  let entries: string[];
  try {
    entries = fs.readdirSync(fixtureDir);
  } catch {
    return;
  }
  for (const entry of entries) {
    if (!entry.startsWith(ROLLBACK_HOLDING_PREFIX)) continue;
    const abandoned = path.join(fixtureDir, entry);
    // Inside the try for the same reason it is in {@link displaceInstalledTree}:
    // an unwritable quarantine root fails here too, it has moved nothing when it
    // does, and a raw errno out of `mkdir` says neither of those things.
    let to: string;
    try {
      to = path.join(
        makeQuarantineHolding(quarantineRoot, fixtureDir, "abandoned-rollback"),
        "node_modules",
      );
    } catch (error) {
      throw new FixtureDepsRefusal(
        `cannot prepare a quarantine directory under ${quarantineRoot} ` +
          `(${describeError(error)}).\n` +
          `  A previous run was interrupted mid-install and left this fixture's dependency ` +
          `tree at ${abandoned}, which is where it still is. Nothing has been changed.`,
      );
    }
    try {
      fs.renameSync(abandoned, to);
    } catch (error) {
      throw new FixtureDepsRefusal(
        `a previous run was interrupted mid-install and left this fixture's dependency tree ` +
          `at ${abandoned}, and it cannot be moved to the quarantine (${describeError(error)}).\n` +
          `  A rename cannot cross filesystems: if the quarantine root is on a different ` +
          `device from this fixture, set VERTER_E2E_FIXTURE_QUARANTINE_DIR to a path on the ` +
          `same one.\n` +
          `  Otherwise move it back to ${path.join(fixtureDir, "node_modules")} by hand, or ` +
          `delete it if you know it is not wanted. The run stops here rather than installing ` +
          `over a fixture whose real tree is sitting beside it.`,
      );
    }
    reportQuarantine(fixtureDir, "a previous run was interrupted mid-install", to);
  }
}

/**
 * Remove a failed install's own partial output, and report rather than throw.
 *
 * The caller has an obligation that outranks this one — putting the predecessor
 * back — so a removal that fails must hand its error over instead of unwinding
 * past the restore with it.
 */
function removePartialInstall(nodeModules: string): unknown {
  try {
    fs.rmSync(nodeModules, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
    return undefined;
  } catch (error) {
    return error;
  }
}

/**
 * Put a displaced predecessor back, or refuse.
 *
 * The failed install's own partial output has already been dealt with by the
 * caller, so this is a rename back into a path this transaction emptied.
 * Restoration is then PROVEN by reading the tree back, because a restore that
 * silently did not happen leaves the fixture with no dependencies and no
 * explanation.
 *
 * Each failure names where the tree actually IS. A single try/catch around both
 * steps could not: after a successful rename the predecessor is at `from`, and
 * saying it is at `to` sends the reader to a path that no longer exists.
 */
function restoreDisplacedTree(
  displaced: DisplacedTree,
  cause: unknown,
  cleanupFailure?: unknown,
): void {
  try {
    fs.renameSync(displaced.to, displaced.from);
  } catch (error) {
    throw new FixtureDepsRefusal(
      `an install failed AND its predecessor could not be restored (${describeError(error)}).\n` +
        `  the fixture expects its tree at: ${displaced.from}\n` +
        `  the predecessor is at:           ${displaced.to}\n` +
        (cleanupFailure
          ? `  The failed install's own output could not be removed from ${displaced.from} first ` +
            `(${describeError(cleanupFailure)}), which is why there was something in the way.\n`
          : "") +
        `  Move it back by hand. The install failed with: ${describeError(cause)}`,
    );
  }
  if (!fs.existsSync(displaced.from)) {
    throw new FixtureDepsRefusal(
      `an install failed, and the rename that should have put its predecessor back reported ` +
        `success while leaving nothing at ${displaced.from}.\n` +
        `  look for it at: ${displaced.to}\n` +
        `  The install failed with: ${describeError(cause)}`,
    );
  }
}

/**
 * The npm invocation every fixture install uses.
 *
 * `--install-links=false` is npm's default, but the default is overridable
 * through `NPM_CONFIG_INSTALL_LINKS` and an `.npmrc`. Passing it on the command
 * line — which beats both in npm's config precedence — means an ambient setting
 * cannot silently turn a live repository link into a frozen copy.
 * {@link assertLocalDepsAreLinked} then verifies the outcome rather than trusting
 * the flag.
 */
export const NPM_INSTALL_ARGV = [
  "install",
  "--no-package-lock",
  "--ignore-scripts",
  "--install-links=false",
] as const;

/**
 * The real installer: npm, which is present wherever the E2E suite runs.
 *
 * A failure is FATAL rather than a warning. The launchers used to catch it and
 * carry on, which meant a fixture whose install failed was opened anyway — with
 * whatever tree was there, or none — and whatever the suite then reported was
 * about a workspace nobody had assembled. `stdio: "pipe"` means npm's own
 * explanation would otherwise be swallowed with it, so it is put in the error.
 */
function npmInstall(fixtureDir: string): void {
  try {
    execSync(`npm ${NPM_INSTALL_ARGV.join(" ")}`, {
      cwd: fixtureDir,
      stdio: "pipe",
      timeout: 60_000,
    });
  } catch (error) {
    const output = (error as { stderr?: Buffer; stdout?: Buffer }).stderr?.toString().trim();
    throw new Error(
      `npm install failed in ${fixtureDir}.\n` +
        `  command: npm ${NPM_INSTALL_ARGV.join(" ")}\n` +
        `  ${describeError(error)}\n` +
        (output ? `  npm said:\n${output.replace(/^/gm, "    ")}\n` : "") +
        `  The fixture is not usable until this succeeds; a run against a tree npm did not ` +
        `finish assembling reports on a workspace nobody built.`,
    );
  }
}

/**
 * Install `fixtureDir`'s dependencies when the installed tree did not come from
 * the manifest and tree currently recorded.
 *
 * Takes the fixture lock and RE-DECIDES under it, so a process that waited on
 * another's install sees the result of that install rather than acting on a
 * decision that has since become false. The stamp is written only AFTER the
 * installer returns: a failed install leaves no provenance, so the next run
 * retries instead of inheriting a half-written tree it believes in.
 *
 * Returns the decision that was acted on.
 */
export function installFixtureDeps(
  fixtureDir: string,
  options: InstallFixtureDepsOptions = {},
): FixtureInstallDecision {
  const install = options.install ?? npmInstall;

  // Cheap and non-destructive, so it needs no lock — and it is the common case.
  if (!fs.existsSync(path.join(fixtureDir, "package.json"))) {
    return { install: false, reason: "no-manifest" };
  }

  const adopted = adoptDecision(fixtureDir, options);
  if (adopted) {
    // Adopt mode uses the tree on disk as it is — and a holding directory is not
    // that tree. It is a PREVIOUS one, left inside the fixture by a transaction
    // that was killed, and leaving it there is worse than leaving an unstamped
    // tree alone: TypeScript's default `exclude` matches the literal name
    // `node_modules`, so `node_modules.verter-rollback-*` is not excluded by it,
    // and the whole holding enters the program of a fixture that declares no
    // `exclude` of its own.
    //
    // Recovered under the lock, because "found here" means "left by a dead
    // transaction" only while this process owns the fixture — but the lock is
    // TAKEN only when there is something to recover, on the same rule as the
    // manifest check above: this mode mutates nothing otherwise, and waiting out
    // another run's install to do nothing is a cost with nothing on the other
    // side of it. The cheap look is not the deciding one; the recovery re-reads
    // the directory under the lock. Nothing else about adopt mode changes: no
    // decision, no install, no stamp.
    if (hasRollbackHolding(fixtureDir)) {
      withFixtureLock(
        fixtureDir,
        () =>
          recoverAbandonedRollbackHoldings(
            fixtureDir,
            options.quarantineRoot ?? fixtureQuarantineRoot(),
          ),
        options.lock,
      );
    }
    return adopted;
  }

  const quarantineRoot = options.quarantineRoot ?? fixtureQuarantineRoot();

  return withFixtureLock(
    fixtureDir,
    () => {
      // Under the lock, so anything of this shape belongs to a transaction that
      // is over, and before the decision, which reads the tree it may have left.
      recoverAbandonedRollbackHoldings(fixtureDir, quarantineRoot);

      const decision = decideFixtureInstall(fixtureDir);
      if (!decision.install) return decision;

      // Before anything moves: a fixture whose declared local dependency is not
      // on this machine cannot install, and finding that out after displacing
      // its tree helps nobody.
      assertLocalDepTargetsExist(fixtureDir);

      const disposition = fixtureTreeDisposition(decision.reason);
      const displaced =
        disposition === "none"
          ? undefined
          : displaceInstalledTree(fixtureDir, decision.reason, disposition, quarantineRoot);
      console.log(
        displaced
          ? `  Replacing dependencies in ${fixtureDir} (${decision.reason})...`
          : `  Installing dependencies in ${fixtureDir}...`,
      );

      try {
        install(fixtureDir);
        assertLocalDepsAreLinked(fixtureDir);
        writeStamp(fixtureDir);
      } catch (error) {
        // Whatever is at `node_modules` now was created by the install that just
        // failed — either there was nothing there, or the predecessor was moved
        // away first, both under this process's lock — so removing it removes
        // this transaction's own output and nothing else. Left behind, it would
        // be an unstamped tree that the NEXT run quarantines: a permanent
        // recovery copy of a partial install that was never anyone's.
        //
        // A removal that THROWS must not take the rollback with it. Unguarded,
        // one (Windows EPERM after the retries; an unwritable directory) skipped
        // the restore entirely: the predecessor stayed displaced, no quarantine
        // was reported, and the error that surfaced was the cleanup's rather
        // than the install's. Restoring the predecessor is the obligation here,
        // and being unable to prove it happened has to be loud.
        const cleanup = removePartialInstall(path.join(fixtureDir, "node_modules"));
        // A predecessor this module proved it produced goes back. One it could
        // not stays quarantined: putting an unowned tree back would re-adopt
        // exactly the tree the run refused to adopt.
        if (displaced?.disposition === "rollback") restoreDisplacedTree(displaced, error, cleanup);
        else if (displaced) reportQuarantine(fixtureDir, decision.reason, displaced.to);
        if (cleanup) {
          throw new FixtureDepsRefusal(
            `an install failed AND the partial tree it left could not be removed ` +
              `(${describeError(cleanup)}).\n` +
              `  the partial install is at: ${path.join(fixtureDir, "node_modules")}\n` +
              (displaced ? `  the predecessor is at:     ${displaced.to}\n` : "") +
              `  Remove the partial tree by hand. Until it is gone the next run sees a tree ` +
              `with no provenance and quarantines it, which turns a failed install into a ` +
              `permanent recovery copy of nobody's tree.\n` +
              `  The install failed with: ${describeError(error)}`,
          );
        }
        throw error;
      }

      if (displaced?.disposition === "rollback") {
        // Proven unmodified, and superseded by a validated install: the only
        // tree here that is safe to delete outright.
        fs.rmSync(displaced.holding, {
          recursive: true,
          force: true,
          maxRetries: 20,
          retryDelay: 100,
        });
        return decision;
      }
      if (!displaced) return decision;
      reportQuarantine(fixtureDir, decision.reason, displaced.to);
      return { ...decision, quarantined: displaced.to };
    },
    options.lock,
  );
}

/**
 * Write the provenance stamp, atomically.
 *
 * The digests are taken BEFORE the stamp exists (dot-entries are excluded from
 * the tree digest, so the stamp cannot perturb the digest it records), and the
 * file appears whole: a half-written stamp reads as debris, which would
 * quarantine a tree that was in fact correctly installed.
 */
function writeStamp(fixtureDir: string): void {
  const nodeModules = path.join(fixtureDir, "node_modules");
  fs.mkdirSync(nodeModules, { recursive: true });
  const stamp = `${JSON.stringify(
    {
      manifest: manifestFingerprint(path.join(fixtureDir, "package.json")),
      tree: installedTreeFingerprint(nodeModules),
      installedAt: new Date().toISOString(),
    },
    null,
    2,
  )}\n`;
  const pending = path.join(nodeModules, `${STAMP_BASENAME}.pending-${randomUUID().slice(0, 8)}`);
  fs.writeFileSync(pending, stamp);
  fs.renameSync(pending, path.join(nodeModules, STAMP_BASENAME));
}

function reportQuarantine(fixtureDir: string, reason: string, to: string): void {
  console.warn(
    `\n  QUARANTINED the dependency tree of ${fixtureDir} (${reason}).\n` +
      `  It was NOT deleted. Recover it from:\n` +
      `    ${to}\n` +
      `  Quarantines are removed only on request:\n` +
      `    pnpm --filter verter-vscode test:e2e:fixtures:clean-quarantine\n`,
  );
}

/**
 * The deliberate, developer-only override: use what is on disk, as it is.
 *
 * Fixture-scoped by name, never a global "trust everything" switch, and the tree
 * it names is neither mutated nor stamped — so the next ordinary run still sees
 * a tree with no provenance and handles it normally. (Its CALLER still recovers
 * a holding directory a killed transaction left inside the fixture; that is a
 * different tree, and one no mode may leave in a fixture's program.) CI rejects
 * the override outright: a run whose dependencies came from nobody knows where
 * is not a run whose result means anything.
 */
function adoptDecision(
  fixtureDir: string,
  options: InstallFixtureDepsOptions,
): FixtureInstallDecision | undefined {
  const declared =
    options.adoptFixtures ??
    (process.env.VERTER_E2E_ADOPT_FIXTURE_DEPS ?? "")
      .split(",")
      .map((entry) => entry.trim())
      .filter(Boolean);
  if (declared.length === 0) return undefined;

  const ci =
    options.continuousIntegration ??
    [process.env.CI, process.env.GITHUB_ACTIONS].some(
      (value) => value !== undefined && value !== "" && value !== "false" && value !== "0",
    );
  if (ci) {
    throw new FixtureDepsRefusal(
      `NON-HERMETIC fixture dependencies were requested (${declared.join(", ")}) under CI. ` +
        `That override exists so a developer can iterate against a tree they arranged by ` +
        `hand; a CI result produced from an unverified tree means nothing.`,
    );
  }
  const wildcard = declared.find((entry) => entry.includes("*"));
  if (wildcard) {
    throw new FixtureDepsRefusal(
      `NON-HERMETIC fixture dependencies must name each fixture (got ${JSON.stringify(wildcard)}). ` +
        `There is no broad authorisation: the point of naming one is that the others stay checked.`,
    );
  }
  if (!declared.includes(path.basename(path.resolve(fixtureDir)))) return undefined;

  console.warn(
    `\n  NON-HERMETIC: using the dependency tree already in ${fixtureDir} as it is.\n` +
      `  Nothing verified what installed it, nothing was installed, and nothing was\n` +
      `  stamped. This run is not reproducible and must not be quoted as a result.\n`,
  );
  return { install: false, reason: "adopted" };
}

/**
 * Resolve and prepare the fixture workspace a launcher is about to open.
 *
 * The one step every launcher owes before pointing an editor at a fixture:
 * check the selector names a real route, derive the path from it, and make the
 * dependency tree current. The benchmark launchers opened
 * `e2e/fixtures/<name>` straight through `@vscode/test-electron` — which does
 * not read `.vscode-test.mjs` and so never ran any of this — and measured
 * whatever tree happened to be lying there, which is the staleness this module
 * exists to end, on the surface where a stale dependency shows up as a
 * performance number rather than a failure.
 *
 * The selector is validated BEFORE the join, for the same reason `.vscode-test.mjs`
 * validates it: `VERTER_COMPLETION_BENCHMARK_FIXTURE` is an environment value,
 * and the install decision now acts on the tree it finds.
 */
export function prepareFixtureWorkspace(
  fixturesRoot: string,
  selector: { readonly rawFixture?: string; readonly typeProvider?: string },
  options: InstallFixtureDepsOptions = {},
): {
  readonly fixture: string;
  readonly typeProvider: string;
  readonly workspace: string;
  readonly decision: FixtureInstallDecision;
} {
  const { fixture, typeProvider } = resolveE2eFixtureSelection(selector);
  const workspace = path.join(fixturesRoot, fixture);
  return { fixture, typeProvider, workspace, decision: installFixtureDeps(workspace, options) };
}

/**
 * Remove every quarantined tree. The explicit cleanup this module promises, and
 * the ONLY thing that removes one.
 *
 * It deletes only what it can POSITIVELY identify as its own, in two steps,
 * because the root is one environment variable away from being a home directory
 * or a temp root and there must be nothing between a typo and an unrecoverable
 * delete:
 *
 *   - the ROOT must carry the marker this module writes when it first uses one.
 *     A directory that has never held a quarantine is refused, not emptied.
 *   - each ENTRY must be a directory carrying the marker written beside the tree
 *     it holds. Anything else — a file, a directory somebody else made, a tree
 *     whose marker did not survive — is left alone and RETURNED, so a run that
 *     removed less than expected says which things it did not touch rather than
 *     leaving that to be discovered later.
 */
export function cleanFixtureQuarantine(root: string = fixtureQuarantineRoot()): {
  removed: string[];
  skipped: string[];
} {
  let entries: string[];
  try {
    entries = fs.readdirSync(root);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return { removed: [], skipped: [] };
    throw new FixtureDepsRefusal(
      `cannot read the quarantine root ${root} (${describeError(error)}).`,
    );
  }

  if (!fs.existsSync(path.join(root, QUARANTINE_ROOT_MARKER))) {
    throw new FixtureDepsRefusal(
      `${root} is not a quarantine this harness created: it holds no ${QUARANTINE_ROOT_MARKER}.\n` +
        `  Nothing was removed. This command deletes directories, so it acts only where it can ` +
        `prove the harness put them — a mistyped or reused ` +
        `VERTER_E2E_FIXTURE_QUARANTINE_DIR would otherwise empty whatever it named.`,
    );
  }

  const removed: string[] = [];
  const skipped: string[] = [];
  for (const entry of entries) {
    const full = path.join(root, entry);
    if (entry === QUARANTINE_ROOT_MARKER) continue;
    if (!fs.existsSync(path.join(full, QUARANTINE_ENTRY_MARKER))) {
      skipped.push(full);
      continue;
    }
    fs.rmSync(full, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
    removed.push(full);
  }
  return { removed, skipped };
}
