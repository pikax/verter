/**
 * The fixture-dependency freshness decision.
 *
 * The defect these cover: the launchers skipped installing whenever
 * `node_modules` merely existed, so a package installed once survived every later
 * run. That is not hypothetical — a four-month-old `@verter/types` inside a
 * fixture's gitignored `node_modules` shadowed the workspace package and decided
 * eight test outcomes.
 *
 * Each staleness case asserts the obsolete package is PRESENT before the call and
 * gone after. Asserting only its absence would pass against a fixture that never
 * wrote one.
 */

import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { createRequire } from "node:module";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  assertLocalDepsAreLinked,
  cleanFixtureQuarantine,
  decideFixtureInstall,
  fixtureQuarantineRoot,
  installFixtureDeps,
  installedTreeFingerprint,
  prepareFixtureWorkspace,
  NPM_INSTALL_ARGV,
} from "./fixtureDeps";
import { fixtureLockPath } from "./fixtureLock";

const created: string[] = [];
const children: ChildProcess[] = [];
const environment: Array<[string, string | undefined]> = [];

beforeEach(() => {
  // The adopt list defaults to this variable for every caller that does not
  // pass `adoptFixtures`, which is most of the cases here. Exported in a shell,
  // it puts every one of them through a mode they did not ask for — so it is
  // pinned rather than inherited, and the two cases that are ABOUT it set it.
  setEnv("VERTER_E2E_ADOPT_FIXTURE_DEPS", undefined);
});

afterEach(() => {
  // Killed by collected handle (pid), never by name.
  for (const child of children.splice(0)) {
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  }
  for (const dir of created.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  // Restored in reverse, and from `afterEach` rather than at the end of a test
  // body, so a FAILING test restores too: these tests share one process, and a
  // leaked `CI` decides the outcome of every adopt-mode case after it.
  for (const [name, value] of environment.splice(0).reverse()) {
    if (value === undefined) delete process.env[name];
    else process.env[name] = value;
  }
});

/** Pin one environment variable for the length of a test. */
function setEnv(name: string, value: string | undefined): void {
  environment.push([name, process.env[name]]);
  if (value === undefined) delete process.env[name];
  else process.env[name] = value;
}

/**
 * The environment of the machine the adopt-mode override exists for.
 *
 * `adoptDecision` reads CI from `CI`/`GITHUB_ACTIONS` whenever the caller does
 * not pass `continuousIntegration`, and REFUSES there — deliberately, because a
 * result produced from a tree nobody verified means nothing. A test about what
 * adopt mode DOES therefore has to say where it is standing: inheriting the
 * variable makes the assertion a statement about the machine, passing on a
 * developer's laptop and refusing on every runner. That exact gap is what took
 * four of these red on CI while they were green everywhere they were written.
 */
function developerMachine(): void {
  setEnv("CI", undefined);
  setEnv("GITHUB_ACTIONS", undefined);
}

/**
 * Short lock waits so a bug surfaces as a failure rather than a hung suite, and
 * a quarantine root this test owns so displaced trees are cleaned up with it
 * instead of accumulating in the machine's real one.
 */
function fast(): { lock: { timeoutMs: number; pollMs: number }; quarantineRoot: string } {
  return { lock: { timeoutMs: 3_000, pollMs: 10 }, quarantineRoot: tempDir() };
}

function tempDir(): string {
  const dir = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), "verter-fixdeps-"));
  created.push(dir);
  return dir;
}

function tempFixture(manifest: Record<string, unknown> = { name: "f", private: true }): string {
  const dir = tempDir();
  fs.writeFileSync(path.join(dir, "package.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return dir;
}

/** A package that an earlier run left behind. Written by hand, never by an install. */
function plantObsoletePackage(dir: string, name = "@verter/types", version = "0.0.1"): string {
  const pkg = path.join(dir, "node_modules", ...name.split("/"));
  fs.mkdirSync(pkg, { recursive: true });
  fs.writeFileSync(path.join(pkg, "package.json"), JSON.stringify({ name, version }));
  fs.writeFileSync(path.join(pkg, "index.d.ts"), "// four months old\n");
  return pkg;
}

/** An installer that records its calls and writes a marker instead of running npm. */
function recordingInstaller(): { calls: string[]; run: (dir: string) => void } {
  const calls: string[] = [];
  return {
    calls,
    run: (dir: string) => {
      calls.push(dir);
      const installed = path.join(dir, "node_modules", "vue");
      fs.mkdirSync(installed, { recursive: true });
      fs.writeFileSync(
        path.join(installed, "package.json"),
        JSON.stringify({ name: "vue", version: "3.5.0" }),
      );
    },
  };
}

/** A real second process holding the fixture lock, as a concurrent run would. */
function childHoldingLock(subject: string, holdMs: number): Promise<ChildProcess> {
  const lockPath = fixtureLockPath(subject);
  // The child is SIGKILLed while it still owns the file, so the file outlives
  // the test unless the test owns its removal.
  created.push(lockPath);
  const script = `
    const fs = require("node:fs");
    const os = require("node:os");
    fs.writeFileSync(${JSON.stringify(lockPath)}, JSON.stringify({
      token: "child", pid: process.pid, host: os.hostname(),
      startedAt: new Date().toISOString(), subject: ${JSON.stringify(subject)},
    }));
    setTimeout(() => {}, ${holdMs});
  `;
  const child = spawn(process.execPath, ["-e", script], { stdio: "ignore" });
  children.push(child);
  return new Promise((resolve, reject) => {
    const started = Date.now();
    const poll = (): void => {
      if (fs.existsSync(lockPath)) return resolve(child);
      if (Date.now() - started > 10_000) return reject(new Error("child never took the lock"));
      setTimeout(poll, 10);
    };
    poll();
  });
}

describe("decideFixtureInstall", () => {
  it("does nothing for a directory with no package.json", () => {
    expect(decideFixtureInstall(tempDir())).toEqual({ install: false, reason: "no-manifest" });
  });

  it("installs when nothing is installed yet", () => {
    expect(decideFixtureInstall(tempFixture())).toEqual({
      install: true,
      reason: "no-node-modules",
    });
  });

  it("REPLACES a node_modules that carries no provenance", () => {
    // The state every fixture is in today: a tree accumulated by earlier runs,
    // with nothing recording what produced it.
    const dir = tempFixture();
    plantObsoletePackage(dir);
    expect(decideFixtureInstall(dir)).toEqual({ install: true, reason: "unstamped-tree" });
  });

  it("reinstalls when the manifest changed since the install", () => {
    const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
    installFixtureDeps(dir, { ...fast(), install: recordingInstaller().run });
    expect(decideFixtureInstall(dir).install).toBe(false);

    fs.writeFileSync(
      path.join(dir, "package.json"),
      `${JSON.stringify({ name: "f", dependencies: { vue: "3.6.0" } }, null, 2)}\n`,
    );
    expect(decideFixtureInstall(dir)).toEqual({ install: true, reason: "manifest-changed" });
  });

  it("treats an unparseable or wrong-shaped stamp as no provenance", () => {
    const dir = tempFixture();
    fs.mkdirSync(path.join(dir, "node_modules"), { recursive: true });
    fs.writeFileSync(path.join(dir, "node_modules", ".verter-e2e-install.json"), "{ not json");
    expect(decideFixtureInstall(dir).reason).toBe("unstamped-tree");

    fs.writeFileSync(
      path.join(dir, "node_modules", ".verter-e2e-install.json"),
      JSON.stringify({ installedAt: "2026-01-01" }),
    );
    expect(decideFixtureInstall(dir).reason).toBe("unstamped-tree");
  });

  it("treats a manifest-only stamp as no provenance", () => {
    // What the previous revision of this module wrote. A manifest hash alone
    // cannot see a package injected into the tree, so those stamps are not
    // evidence and must not be honoured.
    const dir = tempFixture();
    fs.mkdirSync(path.join(dir, "node_modules"), { recursive: true });
    fs.writeFileSync(
      path.join(dir, "node_modules", ".verter-e2e-install.json"),
      JSON.stringify({ manifest: "a".repeat(64), installedAt: "2026-01-01" }),
    );
    expect(decideFixtureInstall(dir).reason).toBe("unstamped-tree");
  });

  describe("installed-tree provenance (the manifest alone is not enough)", () => {
    function installedFixture(): string {
      const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
      installFixtureDeps(dir, { ...fast(), install: recordingInstaller().run });
      expect(decideFixtureInstall(dir)).toEqual({ install: false, reason: "current" });
      return dir;
    }

    it("detects a package ADDED without touching the manifest", () => {
      const dir = installedFixture();
      plantObsoletePackage(dir);
      expect(decideFixtureInstall(dir)).toEqual({ install: true, reason: "tree-changed" });
    });

    it("detects a package REMOVED without touching the manifest", () => {
      const dir = installedFixture();
      fs.rmSync(path.join(dir, "node_modules", "vue"), { recursive: true, force: true });
      expect(decideFixtureInstall(dir)).toEqual({ install: true, reason: "tree-changed" });
    });

    it("detects a package SWAPPED for a different version in place", () => {
      const dir = installedFixture();
      fs.writeFileSync(
        path.join(dir, "node_modules", "vue", "package.json"),
        JSON.stringify({ name: "vue", version: "2.7.0" }),
      );
      expect(decideFixtureInstall(dir)).toEqual({ install: true, reason: "tree-changed" });
    });

    it("detects a package NESTED under another one", () => {
      const dir = installedFixture();
      const nested = path.join(dir, "node_modules", "vue", "node_modules", "@verter", "types");
      fs.mkdirSync(nested, { recursive: true });
      fs.writeFileSync(
        path.join(nested, "package.json"),
        JSON.stringify({ name: "@verter/types", version: "0.0.1" }),
      );
      expect(decideFixtureInstall(dir)).toEqual({ install: true, reason: "tree-changed" });
    });

    it("keeps a tree that changed even when the manifest changed too", () => {
      // The single case that discriminates the order these are checked in, and
      // the reason the tree is checked FIRST. Both changed: read the manifest
      // first and the verdict is `manifest-changed`, whose disposition is
      // rollback — the predecessor is DELETED after a successful reinstall,
      // because a manifest-only change means the tree is provably this module's
      // own output. Here it is not: something was put in it. The tree decides,
      // so this is `tree-changed`, and what was put there is recoverable.
      const dir = installedFixture();
      const root = tempDir();
      const injected = plantObsoletePackage(dir, "svelte");
      fs.writeFileSync(path.join(injected, "irreplaceable.txt"), "not the harness's\n");
      fs.writeFileSync(
        path.join(dir, "package.json"),
        `${JSON.stringify({ name: "f", dependencies: { vue: "3.6.0" } }, null, 2)}\n`,
      );

      // Positive control: the manifest really did change too, so the ordering is
      // what this is about rather than a tree-only case.
      expect(decideFixtureInstall(dir)).toEqual({ install: true, reason: "tree-changed" });

      const decision = installFixtureDeps(dir, {
        ...fast(),
        quarantineRoot: root,
        install: recordingInstaller().run,
      });

      expect(decision.reason).toBe("tree-changed");
      expect(fs.existsSync(injected)).toBe(false);
      expect(
        fs.readFileSync(
          path.join(decision.quarantined as string, "svelte", "irreplaceable.txt"),
          "utf-8",
        ),
      ).toBe("not the harness's\n");
    });

    it("stays quiet when nothing changed", () => {
      // The control: without it the four cases above would pass against a
      // fingerprint that simply never matches, which would reinstall every run.
      const dir = installedFixture();
      expect(decideFixtureInstall(dir)).toEqual({ install: false, reason: "current" });
      expect(decideFixtureInstall(dir)).toEqual({ install: false, reason: "current" });
    });
  });
});

describe("fixtureQuarantineRoot", () => {
  const root = fixtureQuarantineRoot();
  const repository = path.resolve(__dirname, "..", "..", "..", "..");

  it("is on the repository's device, so a fixture inside it cannot cross one", () => {
    // A quarantine is produced by renaming a fixture's `node_modules` out of the
    // repository tree, and a rename cannot cross filesystems. Anchored to the
    // system temp directory, that is a coin toss decided by the machine: on
    // Linux with a tmpfs /tmp and the repo on /home, or on Windows with the repo
    // on D: and %TEMP% on C:, the FIRST run refuses with EXDEV and nothing runs
    // until somebody sets an environment variable by hand. Inside the
    // repository it is the same device by construction — for every fixture that
    // is also inside it, which is what this asserts and the limit of what it
    // asserts. A workspace materialized outside the repository is a separate
    // question, answered in `fixtureQuarantineRoot`'s own note.
    //
    // This cannot fail on the machine it was written on — macOS puts the repo
    // and os.tmpdir() on one device — which is exactly why it is asserted
    // structurally rather than by moving something and seeing.
    expect(path.relative(repository, root).startsWith("..")).toBe(false);
    expect(fs.statSync(path.dirname(root)).dev).toBe(fs.statSync(repository).dev);
  });

  it("is gitignored, so a displaced tree cannot be committed by `git add -A`", () => {
    // git decides this, not a reading of .gitignore: a rule that does not
    // actually match is the failure mode being guarded against.
    const probe = path.join(root, "some-fixture-unstamped-2026", "node_modules", "vue");
    const check = spawnSync("git", ["check-ignore", "-q", probe], { cwd: repository });
    expect({ path: probe, ignored: check.status }).toEqual({ path: probe, ignored: 0 });
  });

  it("is outside every fixture, so nothing under test can resolve into it", () => {
    // Node walks a file's ancestors appending `node_modules`, so a quarantine
    // beside or inside a fixture is never a resolution candidate — but the
    // fixture IS opened as a workspace root, and `monorepo` opens the parent of
    // the directories it installs. A displaced tree must not land in a workspace
    // under test, whatever the resolver would have done with it.
    const fixtures = path.join(repository, "packages", "vue-vscode", "e2e", "fixtures");
    expect(path.relative(fixtures, root).startsWith("..")).toBe(true);
  });
});

describe("cleanFixtureQuarantine", () => {
  it("REFUSES a root the harness did not create, and removes nothing", () => {
    // The command took a directory and `rm -rf`d every entry in it. The root is
    // an environment variable away from being a home directory or a temp root,
    // and there was nothing between a typo and an unrecoverable delete. In the
    // one item whose entire purpose is not destroying things.
    const root = tempDir();
    const precious = path.join(root, "Documents");
    fs.mkdirSync(precious, { recursive: true });
    fs.writeFileSync(path.join(precious, "thesis.txt"), "eight years of work\n");

    expect(() => cleanFixtureQuarantine(root)).toThrow(/refus/i);
    expect(fs.readFileSync(path.join(precious, "thesis.txt"), "utf-8")).toBe(
      "eight years of work\n",
    );
  });

  it("removes quarantined trees, keeps everything else, and says what it kept", () => {
    const root = tempDir();
    const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
    plantObsoletePackage(dir);
    const decision = installFixtureDeps(dir, {
      ...fast(),
      quarantineRoot: root,
      install: recordingInstaller().run,
    });
    const holding = path.dirname(decision.quarantined as string);

    // Things the harness did not put there. Removing either is a delete of
    // something nobody asked it to touch.
    const strayFile = path.join(root, "notes.txt");
    fs.writeFileSync(strayFile, "mine\n");
    const strayDir = path.join(root, "someone-elses-directory");
    fs.mkdirSync(strayDir, { recursive: true });

    // Positive control: there really is a quarantine here to remove.
    expect(fs.existsSync(decision.quarantined as string)).toBe(true);

    const { removed, skipped } = cleanFixtureQuarantine(root);

    expect(removed).toEqual([holding]);
    expect(fs.existsSync(holding)).toBe(false);
    expect([...skipped].sort()).toEqual([strayDir, strayFile].sort());
    expect(fs.readFileSync(strayFile, "utf-8")).toBe("mine\n");
    expect(fs.existsSync(strayDir)).toBe(true);
  });

  it("says nothing and does nothing for a root that was never used", () => {
    expect(cleanFixtureQuarantine(path.join(tempDir(), "never-created"))).toEqual({
      removed: [],
      skipped: [],
    });
  });
});

describe("prepareFixtureWorkspace", () => {
  it("resolves the workspace and installs its dependencies before it is opened", () => {
    // What every launcher owes before it points an editor at a fixture. The
    // benchmark launchers opened one directly through @vscode/test-electron,
    // which does not read `.vscode-test.mjs`, so they measured whatever tree
    // happened to be lying there.
    const root = tempDir();
    const dir = path.join(root, "single-project");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(
      path.join(dir, "package.json"),
      `${JSON.stringify({ name: "e2e-single-project", dependencies: { vue: "3.5.0" } }, null, 2)}\n`,
    );

    const installer = recordingInstaller();
    const prepared = prepareFixtureWorkspace(
      root,
      { rawFixture: "single-project" },
      {
        ...fast(),
        install: installer.run,
      },
    );

    expect(prepared).toEqual({
      fixture: "single-project",
      typeProvider: "",
      workspace: dir,
      decision: { install: true, reason: "no-node-modules" },
    });
    expect(installer.calls).toEqual([dir]);
    expect(fs.existsSync(path.join(dir, "node_modules", ".verter-e2e-install.json"))).toBe(true);

    // Second time it costs nothing, so a benchmark is not measuring an install.
    const again = recordingInstaller();
    expect(
      prepareFixtureWorkspace(
        root,
        { rawFixture: "single-project" },
        { ...fast(), install: again.run },
      ).decision,
    ).toEqual({ install: false, reason: "current" });
    expect(again.calls).toEqual([]);
  });

  it("refuses an unknown fixture before joining it onto anything", () => {
    const root = tempDir();
    const installer = recordingInstaller();
    expect(() =>
      prepareFixtureWorkspace(root, { rawFixture: "../.." }, { ...fast(), install: installer.run }),
    ).toThrow(/matched nothing/i);
    expect(() =>
      prepareFixtureWorkspace(
        root,
        { rawFixture: "not-a-fixture" },
        { ...fast(), install: installer.run },
      ),
    ).toThrow(/matched nothing/i);
    expect(installer.calls).toEqual([]);
  });

  it("carries the provider through, so a launcher does not re-parse the selector", () => {
    const root = tempDir();
    const dir = path.join(root, "monorepo");
    fs.mkdirSync(dir, { recursive: true });
    const prepared = prepareFixtureWorkspace(root, { rawFixture: "monorepo@tsgo" }, { ...fast() });
    expect(prepared).toEqual({
      fixture: "monorepo",
      typeProvider: "tsgo",
      workspace: dir,
      // No package.json: nothing to install, and nothing to distrust.
      decision: { install: false, reason: "no-manifest" },
    });
  });
});

describe("installedTreeFingerprint", () => {
  it("records a symlinked package by its TARGET and never walks through it", () => {
    const dir = tempDir();
    const nodeModules = path.join(dir, "node_modules");
    const target = path.join(dir, "repo");
    // A deep tree behind the link: if the walk followed it, these would be read.
    fs.mkdirSync(path.join(target, "node_modules", "deep-behind-the-link"), { recursive: true });
    fs.writeFileSync(path.join(target, "package.json"), JSON.stringify({ name: "verter" }));
    fs.writeFileSync(
      path.join(target, "node_modules", "deep-behind-the-link", "package.json"),
      JSON.stringify({ name: "deep-behind-the-link" }),
    );
    fs.mkdirSync(nodeModules, { recursive: true });
    fs.symlinkSync(target, path.join(nodeModules, "verter"), "junction");

    // Positive control: the link exists, so "did not descend" is a real result.
    expect(fs.lstatSync(path.join(nodeModules, "verter")).isSymbolicLink()).toBe(true);

    const before = installedTreeFingerprint(nodeModules);

    // Changing content BEHIND the link must not move the fingerprint: the repo
    // is live by design, and re-hashing it would reinstall on every repo edit.
    fs.writeFileSync(
      path.join(target, "package.json"),
      JSON.stringify({ name: "verter", version: "9.9.9" }),
    );
    expect(installedTreeFingerprint(nodeModules)).toBe(before);

    // Re-pointing the link IS an identity change and must move it.
    const other = path.join(dir, "other-repo");
    fs.mkdirSync(other, { recursive: true });
    fs.rmSync(path.join(nodeModules, "verter"), { force: true });
    fs.symlinkSync(other, path.join(nodeModules, "verter"), "junction");
    expect(installedTreeFingerprint(nodeModules)).not.toBe(before);
  });

  it("is order-independent and stable across repeated reads", () => {
    const dir = tempDir();
    const nodeModules = path.join(dir, "node_modules");
    for (const name of ["b-pkg", "a-pkg", "@scope/c-pkg"]) {
      const pkg = path.join(nodeModules, ...name.split("/"));
      fs.mkdirSync(pkg, { recursive: true });
      fs.writeFileSync(path.join(pkg, "package.json"), JSON.stringify({ name }));
    }
    expect(installedTreeFingerprint(nodeModules)).toBe(installedTreeFingerprint(nodeModules));
    expect(installedTreeFingerprint(nodeModules)).not.toBe(
      installedTreeFingerprint(path.join(tempDir(), "node_modules")),
    );
  });

  it("sees a bare module FILE, which Node resolves ahead of the package directory", () => {
    // The module claims a package added without touching the manifest is still
    // detected. A non-directory entry was skipped outright, so
    // `node_modules/vue.js` was invisible — and `LOAD_AS_FILE` runs before
    // `LOAD_AS_DIRECTORY`, so that file is what `require("vue")` actually gets.
    const dir = tempDir();
    const nodeModules = path.join(dir, "node_modules");
    const pkg = path.join(nodeModules, "vue");
    fs.mkdirSync(pkg, { recursive: true });
    fs.writeFileSync(
      path.join(pkg, "package.json"),
      JSON.stringify({ name: "vue", main: "index.js" }),
    );
    fs.writeFileSync(path.join(pkg, "index.js"), `module.exports = "from-the-DIRECTORY";`);
    const before = installedTreeFingerprint(nodeModules);

    fs.writeFileSync(path.join(nodeModules, "vue.js"), `module.exports = "from-the-BARE-FILE";`);

    // Positive control, proved here rather than asserted: with both present,
    // Node resolves the file. This is a real shadow, not a hypothetical one.
    fs.writeFileSync(path.join(dir, "probe.cjs"), `module.exports = require("vue");`);
    expect(createRequire(path.join(dir, "probe.cjs"))("./probe.cjs")).toBe("from-the-BARE-FILE");

    const shadowed = installedTreeFingerprint(nodeModules);
    expect(shadowed).not.toBe(before);
    // Its CONTENT counts too: swapping what the shadow exports is a different
    // tree, and recording only the name would miss it.
    fs.writeFileSync(path.join(nodeModules, "vue.js"), `module.exports = "something-else";`);
    expect(installedTreeFingerprint(nodeModules)).not.toBe(shadowed);
  });

  it("ignores the stamp it will itself be written into", () => {
    const dir = tempDir();
    const nodeModules = path.join(dir, "node_modules");
    fs.mkdirSync(nodeModules, { recursive: true });
    const before = installedTreeFingerprint(nodeModules);
    fs.writeFileSync(
      path.join(nodeModules, ".verter-e2e-install.json"),
      JSON.stringify({ manifest: "x", tree: "y" }),
    );
    expect(installedTreeFingerprint(nodeModules)).toBe(before);
  });
});

describe("local dependency link semantics", () => {
  it("invokes npm with --install-links=false", () => {
    // The fixtures reach the repository through `file:` specs. npm's
    // `install-links` decides link-vs-copy and is env-overridable, so the flag is
    // passed rather than assumed.
    expect(NPM_INSTALL_ARGV).toContain("--install-links=false");
  });

  it("accepts a file: dependency that was LINKED", () => {
    const dir = tempFixture({ name: "f", dependencies: { verter: "file:../repo" } });
    const target = path.join(dir, "repo");
    fs.mkdirSync(target, { recursive: true });
    fs.mkdirSync(path.join(dir, "node_modules"), { recursive: true });
    fs.symlinkSync(target, path.join(dir, "node_modules", "verter"), "junction");
    expect(() => assertLocalDepsAreLinked(dir)).not.toThrow();
  });

  it("rejects a file: dependency that was COPIED", () => {
    // What NPM_CONFIG_INSTALL_LINKS=true produces: the fixture is frozen at the
    // repository's install-time state, and no later repository change reinstalls
    // it — the stale-tree problem in the one place the stamp cannot see.
    const dir = tempFixture({ name: "f", dependencies: { verter: "file:../repo" } });
    const copied = path.join(dir, "node_modules", "verter");
    fs.mkdirSync(copied, { recursive: true });
    fs.writeFileSync(path.join(copied, "package.json"), JSON.stringify({ name: "verter" }));
    expect(() => assertLocalDepsAreLinked(dir)).toThrow(/was COPIED into/);
  });

  it("rejects a declared local dependency that is missing entirely", () => {
    const dir = tempFixture({ name: "f", dependencies: { verter: "file:../repo" } });
    fs.mkdirSync(path.join(dir, "node_modules"), { recursive: true });
    expect(() => assertLocalDepsAreLinked(dir)).toThrow(/is not installed at/);
  });

  it("names a file: target that does not exist, and displaces nothing", () => {
    // A local checkout can be absent on a contributor or runner. The launcher
    // used to catch npm's failure and warn, which meant the suite ran against a
    // tree missing a declared package; it now fails closed, which is right — but
    // "npm exited 1" names neither what is missing nor what to do about it.
    const dir = tempFixture({
      name: "f",
      dependencies: { verter: "file:../verter-release-clean" },
    });
    const absent = path.resolve(dir, "../verter-release-clean");
    const stale = plantObsoletePackage(dir);
    const installer = recordingInstaller();

    let message = "";
    try {
      installFixtureDeps(dir, { ...fast(), install: installer.run });
    } catch (error) {
      message = String((error as Error).message);
    }

    expect(message).toMatch(/refus/i);
    // The absolute path, the manifest entry that asks for it, and the fixture.
    expect(message).toContain(absent);
    expect(message).toContain("verter");
    expect(message).toContain(dir);
    // Nothing was installed and nothing was moved: a missing dependency target
    // is decided before the tree is touched.
    expect(installer.calls).toEqual([]);
    expect(fs.existsSync(stale)).toBe(true);
  });

  it("says nothing about a file: target that is there", () => {
    // The control: this check must not fire on the ordinary case, or every
    // fixture that reaches the repository through `file:` would refuse.
    const dir = tempFixture({ name: "f", dependencies: { verter: "file:../repo" } });
    const target = path.resolve(dir, "../repo");
    fs.mkdirSync(target, { recursive: true });
    created.push(target);
    const base = recordingInstaller();
    const install = (into: string): void => {
      base.run(into);
      // What npm does with `--install-links=false`: a live link, not a copy.
      fs.symlinkSync(target, path.join(into, "node_modules", "verter"), "junction");
    };
    expect(installFixtureDeps(dir, { ...fast(), install }).install).toBe(true);
    expect(base.calls).toEqual([dir]);
    expect(fs.lstatSync(path.join(dir, "node_modules", "verter")).isSymbolicLink()).toBe(true);
  });

  it("says nothing about registry dependencies", () => {
    // The control: the check must not fire on the ordinary case, or every
    // fixture would fail.
    const dir = tempFixture({ name: "f", dependencies: { vue: "^3.5.0" } });
    expect(() => assertLocalDepsAreLinked(dir)).not.toThrow();
  });
});

describe("installFixtureDeps", () => {
  it("keeps the ecosystem fixture independent of an unprovisioned sibling checkout", () => {
    const manifestPath = path.resolve(__dirname, "../fixtures/ecosystem-parity/package.json");
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8")) as {
      dependencies?: Record<string, string>;
    };
    expect(manifest.dependencies).not.toHaveProperty("verter");
  });

  it("does not run the installer for a directory with no package.json", () => {
    const dir = tempDir();
    const installer = recordingInstaller();
    expect(installFixtureDeps(dir, { ...fast(), install: installer.run }).install).toBe(false);
    expect(installer.calls).toEqual([]);
    // And it took no lock: a no-op must not serialise against other runs.
    expect(fs.existsSync(fixtureLockPath(dir))).toBe(false);
  });

  it("evicts a stale package left by an earlier run", () => {
    const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
    const obsolete = plantObsoletePackage(dir);

    // Positive control: the tree really does carry the obsolete package. Without
    // this, the absence assertion below would hold for a fixture that never
    // wrote one, and would keep holding if the eviction were removed.
    expect(fs.existsSync(obsolete)).toBe(true);

    const installer = recordingInstaller();
    const decision = installFixtureDeps(dir, { ...fast(), install: installer.run });
    expect(decision.install).toBe(true);
    expect(decision.reason).toBe("unstamped-tree");
    expect(installer.calls).toEqual([dir]);
    // Gone from the fixture — but recoverable, not destroyed. Which is which is
    // asserted where the quarantine is.
    expect(fs.existsSync(obsolete)).toBe(false);
    expect(fs.existsSync(decision.quarantined as string)).toBe(true);
    expect(fs.existsSync(path.join(dir, "node_modules", "vue"))).toBe(true);
  });

  it("evicts a package the edited manifest no longer asks for", () => {
    // A dependency dropped from `package.json`. The INSTALLER stops producing it,
    // exactly as npm would — nothing is planted by hand here, so the tree still
    // matches the digest stamped for it and this is the manifest-only case: the
    // predecessor is provably this module's own output, replaced and then
    // deleted rather than kept.
    const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0", svelte: "5.0.0" } });
    const first = recordingInstaller();
    installFixtureDeps(dir, {
      ...fast(),
      install: (target) => {
        first.run(target);
        plantObsoletePackage(target, "svelte");
      },
    });
    const dropped = path.join(dir, "node_modules", "svelte");
    expect(fs.existsSync(dropped)).toBe(true);
    expect(decideFixtureInstall(dir)).toEqual({ install: false, reason: "current" });

    fs.writeFileSync(
      path.join(dir, "package.json"),
      `${JSON.stringify({ name: "f", dependencies: { vue: "3.6.0" } }, null, 2)}\n`,
    );
    expect(installFixtureDeps(dir, { ...fast(), install: recordingInstaller().run }).reason).toBe(
      "manifest-changed",
    );
    // Replaced, not merged: `npm install` over the old tree would have left this.
    expect(fs.existsSync(dropped)).toBe(false);
    expect(fs.existsSync(path.join(dir, "node_modules", "vue"))).toBe(true);
  });

  it("does not reinstall when the tree already came from this manifest", () => {
    // The property the existence check got right, and that a naive "always
    // install" fix would lose: a route must not pay an install per run.
    const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
    const installer = recordingInstaller();

    expect(installFixtureDeps(dir, { ...fast(), install: installer.run }).install).toBe(true);
    expect(installFixtureDeps(dir, { ...fast(), install: installer.run })).toEqual({
      install: false,
      reason: "current",
    });
    expect(installFixtureDeps(dir, { ...fast(), install: installer.run }).install).toBe(false);
    expect(installer.calls).toEqual([dir]);
  });

  it("leaves no provenance behind when the install fails", () => {
    const dir = tempFixture();
    const root = tempDir();
    expect(() =>
      installFixtureDeps(dir, {
        ...fast(),
        quarantineRoot: root,
        install: (target) => {
          // Half a tree, as an interrupted npm leaves.
          fs.mkdirSync(path.join(target, "node_modules", "vue"), { recursive: true });
          throw new Error("npm exploded");
        },
      }),
    ).toThrow(/npm exploded/);

    // Unstamped, so the next run retries rather than trusting a half-written tree.
    expect(decideFixtureInstall(dir).install).toBe(true);
    // And there is nothing there to retry AROUND. The fixture had no tree before
    // this call, so whatever the failed install left is this call's own output —
    // keeping it would make the next run quarantine a tree that was never
    // anyone's, and quarantines are kept forever.
    expect(decideFixtureInstall(dir).reason).toBe("no-node-modules");
    expect(fs.existsSync(path.join(dir, "node_modules"))).toBe(false);
    expect(fs.existsSync(root) ? fs.readdirSync(root) : []).toEqual([]);
    // And the lock did not leak, or every later run would block on it.
    expect(fs.existsSync(fixtureLockPath(dir))).toBe(false);
  });

  it("stamps both the manifest and the tree it installed", () => {
    const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
    installFixtureDeps(dir, { ...fast(), install: recordingInstaller().run });

    const stamp = JSON.parse(
      fs.readFileSync(path.join(dir, "node_modules", ".verter-e2e-install.json"), "utf-8"),
    ) as { manifest?: unknown; tree?: unknown };
    expect(stamp.manifest).toMatch(/^[0-9a-f]{64}$/);
    expect(stamp.tree).toMatch(/^[0-9a-f]{64}$/);
    expect(stamp.tree).not.toBe(stamp.manifest);
  });

  it("removes only node_modules — the authored fixture tree survives", () => {
    // This is the one place the harness deletes a directory inside the
    // repository, and the fixture sources beside it are git-tracked.
    const dir = tempFixture();
    plantObsoletePackage(dir);
    fs.mkdirSync(path.join(dir, "src"), { recursive: true });
    fs.writeFileSync(path.join(dir, "src", "App.vue"), "<template />\n");
    fs.writeFileSync(path.join(dir, "tsconfig.json"), "{}\n");

    installFixtureDeps(dir, { ...fast(), install: recordingInstaller().run });

    expect(fs.readFileSync(path.join(dir, "src", "App.vue"), "utf-8")).toBe("<template />\n");
    expect(fs.existsSync(path.join(dir, "tsconfig.json"))).toBe(true);
    expect(fs.existsSync(path.join(dir, "package.json"))).toBe(true);
  });

  describe("displacing a tree the harness does not own", () => {
    function quarantined(root: string): string[] {
      if (!fs.existsSync(root)) return [];
      return fs.readdirSync(root).map((entry) => path.join(root, entry));
    }

    /** A fixture whose install produces both a package and a link out of the tree. */
    function linkingInstaller(canary: string): { calls: string[]; run: (dir: string) => void } {
      const base = recordingInstaller();
      return {
        calls: base.calls,
        run: (dir: string) => {
          base.run(dir);
          fs.symlinkSync(canary, path.join(dir, "node_modules", "verter"), "junction");
        },
      };
    }

    it("QUARANTINES an unstamped tree rather than deleting it", () => {
      // The ruling: a tree the harness cannot prove it produced is not developer
      // state to be destroyed, and not evidence to be adopted. It is moved aside,
      // kept, and its recovery path reported.
      const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
      const root = tempDir();
      const obsolete = plantObsoletePackage(dir);
      fs.writeFileSync(
        path.join(obsolete, "irreplaceable.txt"),
        "work that predates the harness\n",
      );

      expect(fs.existsSync(obsolete)).toBe(true);
      expect(quarantined(root)).toEqual([]);

      const decision = installFixtureDeps(dir, {
        ...fast(),
        install: recordingInstaller().run,
        quarantineRoot: root,
      });

      expect(decision.reason).toBe("unstamped-tree");
      // Gone from the fixture, and PRESENT in the quarantine — the file too, so
      // this is a move rather than a delete plus an empty directory.
      expect(fs.existsSync(obsolete)).toBe(false);
      expect(decision.quarantined).toBeDefined();
      const recovered = path.join(
        decision.quarantined as string,
        "@verter",
        "types",
        "irreplaceable.txt",
      );
      expect(fs.readFileSync(recovered, "utf-8")).toBe("work that predates the harness\n");
      // Outside every fixture resolution path: not under the fixture at all.
      expect((decision.quarantined as string).startsWith(path.resolve(dir))).toBe(false);
      // And the fixture got a clean tree, not a merged one.
      expect(fs.existsSync(path.join(dir, "node_modules", "vue"))).toBe(true);
    });

    it("QUARANTINES a tree that changed under a valid stamp", () => {
      const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
      const root = tempDir();
      installFixtureDeps(dir, { ...fast(), install: recordingInstaller().run });
      const injected = plantObsoletePackage(dir, "svelte");
      expect(fs.existsSync(injected)).toBe(true);

      const decision = installFixtureDeps(dir, {
        ...fast(),
        install: recordingInstaller().run,
        quarantineRoot: root,
      });

      expect(decision.reason).toBe("tree-changed");
      expect(fs.existsSync(injected)).toBe(false);
      expect(fs.existsSync(path.join(decision.quarantined as string, "svelte"))).toBe(true);
    });

    it("DELETES the predecessor only after a manifest-only reinstall succeeds", () => {
      // The one tree the harness CAN prove it produced: stamped, and the tree
      // still exactly matches the digest recorded for it. Keeping a quarantine of
      // every dependency bump would double fixture storage for no recoverable
      // information.
      const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
      const root = tempDir();
      installFixtureDeps(dir, { ...fast(), install: recordingInstaller().run });

      fs.writeFileSync(
        path.join(dir, "package.json"),
        `${JSON.stringify({ name: "f", dependencies: { vue: "3.6.0" } }, null, 2)}\n`,
      );
      const decision = installFixtureDeps(dir, {
        ...fast(),
        install: recordingInstaller().run,
        quarantineRoot: root,
      });

      expect(decision.reason).toBe("manifest-changed");
      expect(decision.quarantined).toBeUndefined();
      // Nothing kept: the predecessor was proven unmodified before it was removed.
      expect(quarantined(root)).toEqual([]);
      // Not in the fixture either — the undo buffer goes with the transaction.
      expect(rollbackHoldings(dir)).toEqual([]);
      expect(decideFixtureInstall(dir)).toEqual({ install: false, reason: "current" });
    });

    /** What a rollback leaves inside a fixture if it leaves anything. */
    function rollbackHoldings(fixtureDir: string): string[] {
      return fs.readdirSync(fixtureDir).filter((entry) => entry.startsWith("node_modules."));
    }

    it("rolls back through the fixture itself, never through the quarantine root", () => {
      // A rollback moves a tree the harness PROVED it produced out of the way and
      // back again. That has nothing to do with where unowned trees are kept, and
      // borrowing that root makes an operator's `VERTER_E2E_FIXTURE_QUARANTINE_DIR`
      // — or any accident to it — able to break a path whose whole job is putting
      // a fixture back the way it was. A sibling of `node_modules` is the same
      // directory, so the rename cannot cross a device either.
      const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
      installFixtureDeps(dir, { ...fast(), install: recordingInstaller().run });

      // A quarantine root that CANNOT be created: a path under a regular file.
      const wall = path.join(tempDir(), "not-a-directory");
      fs.writeFileSync(wall, "");
      const unusable = path.join(wall, "quarantine");

      fs.writeFileSync(
        path.join(dir, "package.json"),
        `${JSON.stringify({ name: "f", dependencies: { vue: "3.6.0" } }, null, 2)}\n`,
      );
      const decision = installFixtureDeps(dir, {
        ...fast(),
        quarantineRoot: unusable,
        install: recordingInstaller().run,
      });

      expect(decision.reason).toBe("manifest-changed");
      expect(decideFixtureInstall(dir)).toEqual({ install: false, reason: "current" });
      // Nothing was left inside the fixture either.
      expect(rollbackHoldings(dir)).toEqual([]);
      expect(fs.existsSync(unusable)).toBe(false);
    });

    it("recovers a holding directory a killed rollback left inside the fixture", () => {
      // A rollback's undo buffer only exists between the displace and the
      // restore, and only under the lock — so one found here belongs to a
      // transaction that died, which makes it the fixture's real dependency
      // tree sitting in a directory nothing will ever look in again. It is
      // moved to the quarantine, not deleted and not adopted: the harness
      // cannot tell what state it was in when its transaction was killed.
      const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
      const root = tempDir();
      // The production name, spelled out: a killed run leaves exactly this.
      const abandoned = path.join(dir, "node_modules.verter-rollback-deadbeef");
      fs.mkdirSync(path.join(abandoned, "vue"), { recursive: true });
      fs.writeFileSync(path.join(abandoned, "vue", "package.json"), `{"name":"vue"}`);
      fs.writeFileSync(path.join(abandoned, "irreplaceable.txt"), "the predecessor\n");

      const decision = installFixtureDeps(dir, {
        ...fast(),
        quarantineRoot: root,
        install: recordingInstaller().run,
      });

      // The transaction itself went ahead normally.
      expect(decision.reason).toBe("no-node-modules");
      expect(fs.existsSync(path.join(dir, "node_modules", "vue"))).toBe(true);
      // And the abandoned tree is out of the fixture and recoverable.
      expect(fs.existsSync(abandoned)).toBe(false);
      const recovered = quarantined(root).map((entry) =>
        path.join(entry, "node_modules", "irreplaceable.txt"),
      );
      expect(recovered.filter((file) => fs.existsSync(file))).toHaveLength(1);
    });

    it("RESTORES the predecessor when the reinstall fails", () => {
      const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
      const root = tempDir();
      installFixtureDeps(dir, { ...fast(), install: recordingInstaller().run });
      const stampBefore = fs.readFileSync(
        path.join(dir, "node_modules", ".verter-e2e-install.json"),
        "utf-8",
      );

      fs.writeFileSync(
        path.join(dir, "package.json"),
        `${JSON.stringify({ name: "f", dependencies: { vue: "3.6.0" } }, null, 2)}\n`,
      );
      expect(() =>
        installFixtureDeps(dir, {
          ...fast(),
          quarantineRoot: root,
          install: () => {
            throw new Error("npm exploded");
          },
        }),
      ).toThrow(/npm exploded/);

      // The predecessor is back, byte for byte, where the fixture expects it.
      expect(fs.existsSync(path.join(dir, "node_modules", "vue"))).toBe(true);
      expect(
        fs.readFileSync(path.join(dir, "node_modules", ".verter-e2e-install.json"), "utf-8"),
      ).toBe(stampBefore);
      // A failed install leaves no holding area behind either, in the quarantine
      // root or beside the tree it put back.
      expect(quarantined(root)).toEqual([]);
      expect(rollbackHoldings(dir)).toEqual([]);
    });

    // Windows chmod does not model directory write permission this way, and root
    // ignores it altogether — the failure would not happen, and the test would
    // pass while proving nothing.
    const canRefuseRemoval = process.platform !== "win32" && process.getuid?.() !== 0;

    it.skipIf(!canRefuseRemoval)(
      "still restores the predecessor when the failed install cannot be cleaned up",
      () => {
        // The partial tree was removed BEFORE the rollback, unguarded. A removal
        // that throws — Windows EPERM after the retries, an unwritable directory
        // here — therefore skipped the rollback entirely: the predecessor stayed
        // displaced, no quarantine was reported, and the error that surfaced was
        // the cleanup's, not the install's. An install failure has to put the
        // predecessor back, and an inability to prove it must say so loudly.
        const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
        const root = tempDir();
        installFixtureDeps(dir, { ...fast(), install: recordingInstaller().run });
        const locked = path.join(dir, "node_modules", "unremovable");

        fs.writeFileSync(
          path.join(dir, "package.json"),
          `${JSON.stringify({ name: "f", dependencies: { vue: "3.6.0" } }, null, 2)}\n`,
        );

        let message = "";
        try {
          installFixtureDeps(dir, {
            ...fast(),
            quarantineRoot: root,
            install: (target) => {
              recordingInstaller().run(target);
              // A directory whose contents cannot be unlinked, which is what
              // `rmSync` meets on a real machine as EPERM or EACCES.
              fs.mkdirSync(path.join(target, "node_modules", "unremovable"), { recursive: true });
              fs.writeFileSync(path.join(target, "node_modules", "unremovable", "held"), "");
              fs.chmodSync(path.join(target, "node_modules", "unremovable"), 0o500);
              throw new Error("npm exploded");
            },
          });
        } catch (error) {
          message = String((error as Error).message);
        } finally {
          fs.chmodSync(locked, 0o700);
        }

        // Positive control: the removal really was refused, so this is the
        // branch under test.
        expect(fs.existsSync(path.join(locked, "held"))).toBe(true);
        // It refused, rather than surfacing a bare errno from the cleanup.
        expect(message).toMatch(/refus/i);
        // And it named everything needed to recover by hand: the install's own
        // failure, the cleanup's, and BOTH places a tree is now sitting.
        expect(message).toContain("npm exploded");
        expect(message).toMatch(/EACCES|EPERM|ENOTEMPTY/);
        expect(message).toContain(path.join(dir, "node_modules"));
        const holdings = rollbackHoldings(dir);
        expect(holdings).toHaveLength(1);
        expect(message).toContain(path.join(dir, holdings[0]));
      },
      // `rmSync` is given 20 retries with a linear backoff, so a removal that
      // cannot succeed takes as long as that budget — which is the production
      // behaviour this case is about, not a slow test.
      45_000,
    );

    it.skipIf(!canRefuseRemoval)(
      "REFUSES in the harness's own voice when a recovery cannot be prepared",
      () => {
        // Recovering an abandoned holding and displacing a live tree prepare the
        // same quarantine directory, and an unwritable quarantine root fails
        // both. One of them said so as a refusal naming the root and stating
        // that nothing had changed; the other let the raw errno out, because the
        // call was outside its try. Same cause, same absence of consequence — a
        // recovery has moved nothing when it fails there — and two different
        // answers, only one of which tells the reader what to do.
        const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
        const root = path.join(tempDir(), "unwritable-quarantine");
        fs.mkdirSync(root);
        const abandoned = path.join(dir, "node_modules.verter-rollback-deadbeef");
        fs.mkdirSync(abandoned, { recursive: true });
        fs.writeFileSync(path.join(abandoned, "irreplaceable.txt"), "the predecessor\n");
        fs.chmodSync(root, 0o500);

        const installer = recordingInstaller();
        let error: unknown;
        try {
          installFixtureDeps(dir, { ...fast(), quarantineRoot: root, install: installer.run });
        } catch (caught) {
          error = caught;
        } finally {
          // Restored so the fixture teardown can remove it.
          fs.chmodSync(root, 0o700);
        }

        // The harness's own refusal, not a bare errno from `mkdir`.
        expect((error as Error | undefined)?.name).toBe("FixtureDepsRefusal");
        const message = String((error as Error).message);
        expect(message).toMatch(/refusing to touch fixture dependencies/);
        // Naming the root that could not be prepared, the cause, and where the
        // tree still is — everything needed to act on it.
        expect(message).toContain(root);
        expect(message).toMatch(/EACCES|EPERM/);
        expect(message).toContain(abandoned);
        // Positive control: it really did stop before doing anything. The tree
        // is untouched and no install ran.
        expect(fs.readFileSync(path.join(abandoned, "irreplaceable.txt"), "utf-8")).toBe(
          "the predecessor\n",
        );
        expect(installer.calls).toEqual([]);
      },
    );

    it("REFUSES a current-format stamp it cannot read, and touches nothing", () => {
      // Undecidable is not the same as unstamped. A stamp that IS this format but
      // whose values are unusable means the harness cannot tell what produced the
      // tree, and guessing costs a tree it may not own.
      const dir = tempFixture();
      const root = tempDir();
      const obsolete = plantObsoletePackage(dir);
      fs.writeFileSync(
        path.join(dir, "node_modules", ".verter-e2e-install.json"),
        JSON.stringify({ manifest: "not-a-digest", tree: "also-not", installedAt: "2026-01-01" }),
      );

      const installer = recordingInstaller();
      expect(() =>
        installFixtureDeps(dir, { ...fast(), install: installer.run, quarantineRoot: root }),
      ).toThrow(/refus/i);

      // No install, no move, no delete.
      expect(installer.calls).toEqual([]);
      expect(fs.existsSync(obsolete)).toBe(true);
      expect(quarantined(root)).toEqual([]);
    });

    it("REFUSES when node_modules is a symlink rather than a tree", () => {
      const dir = tempFixture();
      const root = tempDir();
      const elsewhere = path.join(dir, "somewhere-else");
      fs.mkdirSync(elsewhere, { recursive: true });
      fs.writeFileSync(path.join(elsewhere, "keep.txt"), "not the harness's\n");
      fs.symlinkSync(elsewhere, path.join(dir, "node_modules"), "junction");

      expect(() =>
        installFixtureDeps(dir, {
          ...fast(),
          install: recordingInstaller().run,
          quarantineRoot: root,
        }),
      ).toThrow(/refus/i);
      expect(fs.readFileSync(path.join(elsewhere, "keep.txt"), "utf-8")).toBe(
        "not the harness's\n",
      );
      expect(fs.lstatSync(path.join(dir, "node_modules")).isSymbolicLink()).toBe(true);
    });

    it("never follows a link out of the tree, when quarantining OR when deleting", () => {
      // Eight fixtures declare `"verter": "file:../../../../.."`, so every tree
      // displaced here contains a symlink to the REPOSITORY ROOT. Both mechanisms
      // must act on the link, never through it: `rename` moves a directory entry
      // and cannot descend, and `rmSync` unlinks a symlink rather than following
      // it. This is the assertion that stops a future reader having to re-derive
      // that "rm -rf a tree containing a link to the whole repo" is safe.
      const canary = tempDir();
      fs.writeFileSync(path.join(canary, "the-repository.txt"), "must survive\n");
      const root = tempDir();

      // (1) QUARANTINE: an unstamped tree containing the link is moved aside.
      const first = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
      fs.mkdirSync(path.join(first, "node_modules"), { recursive: true });
      fs.symlinkSync(canary, path.join(first, "node_modules", "verter"), "junction");
      const moved = installFixtureDeps(first, {
        ...fast(),
        install: recordingInstaller().run,
        quarantineRoot: root,
      });
      expect(moved.reason).toBe("unstamped-tree");
      expect(fs.readFileSync(path.join(canary, "the-repository.txt"), "utf-8")).toBe(
        "must survive\n",
      );
      // The link went with the tree and still points where it pointed.
      expect(fs.lstatSync(path.join(moved.quarantined as string, "verter")).isSymbolicLink()).toBe(
        true,
      );

      // (2) DELETE: a proven predecessor containing the link is removed outright.
      const second = tempFixture({ name: "g", dependencies: { vue: "3.5.0" } });
      installFixtureDeps(second, { ...fast(), install: linkingInstaller(canary).run });
      fs.writeFileSync(
        path.join(second, "package.json"),
        `${JSON.stringify({ name: "g", dependencies: { vue: "3.6.0" } }, null, 2)}\n`,
      );
      const deleted = installFixtureDeps(second, {
        ...fast(),
        install: linkingInstaller(canary).run,
        quarantineRoot: root,
      });
      expect(deleted.reason).toBe("manifest-changed");
      expect(fs.readFileSync(path.join(canary, "the-repository.txt"), "utf-8")).toBe(
        "must survive\n",
      );
      expect(fs.readdirSync(canary)).toEqual(["the-repository.txt"]);
    });
  });

  describe("the NON-HERMETIC override", () => {
    it("uses an existing tree without mutating or stamping it", () => {
      developerMachine();
      const dir = tempFixture({ name: "adopt-me", dependencies: { vue: "3.5.0" } });
      const obsolete = plantObsoletePackage(dir);
      const installer = recordingInstaller();

      const decision = installFixtureDeps(dir, {
        ...fast(),
        install: installer.run,
        adoptFixtures: [path.basename(dir)],
      });

      expect(decision).toEqual({ install: false, reason: "adopted" });
      expect(installer.calls).toEqual([]);
      // Untouched AND unstamped: the next ordinary run must not inherit provenance
      // this one did not establish.
      expect(fs.existsSync(obsolete)).toBe(true);
      expect(decideFixtureInstall(dir).reason).toBe("unstamped-tree");
    });

    it("still recovers a holding a killed run left inside the fixture", () => {
      // Adopt mode uses the tree on disk as it is. A `node_modules.verter-
      // rollback-*` is not that tree: it is a PREVIOUS one, left inside the
      // fixture by a transaction that was killed between the displace and the
      // restore, and it is the fixture's real dependencies sitting in a
      // directory nothing will ever look in again.
      //
      // Leaving it costs more than an unrecovered tree. TypeScript's default
      // `exclude` matches the literal name `node_modules`, and these fixtures
      // declare no `exclude` of their own — so a directory called
      // `node_modules.verter-rollback-deadbeef` is not excluded by it, and the
      // whole holding enters the fixture's program. Several of these trees
      // contain a `verter` symlink to the repository root.
      //
      // The recovery is the ordinary one and runs under the ordinary lock:
      // "found here" only means "left by a dead transaction" while this process
      // owns the fixture. Everything else about adopt mode is unchanged.
      developerMachine();
      const dir = tempFixture({ name: "adopt-me", dependencies: { vue: "3.5.0" } });
      const root = tempDir();
      const abandoned = path.join(dir, "node_modules.verter-rollback-deadbeef");
      fs.mkdirSync(path.join(abandoned, "vue"), { recursive: true });
      fs.writeFileSync(path.join(abandoned, "irreplaceable.txt"), "the predecessor\n");
      const adoptedTree = plantObsoletePackage(dir);
      const installer = recordingInstaller();

      const decision = installFixtureDeps(dir, {
        ...fast(),
        quarantineRoot: root,
        install: installer.run,
        adoptFixtures: [path.basename(dir)],
      });

      // Adopt mode itself is untouched: nothing installed, nothing stamped, and
      // the tree it was told to use is exactly as it was.
      expect(decision).toEqual({ install: false, reason: "adopted" });
      expect(installer.calls).toEqual([]);
      expect(fs.existsSync(adoptedTree)).toBe(true);
      expect(decideFixtureInstall(dir).reason).toBe("unstamped-tree");
      // And the holding is out of the fixture, in the quarantine, recoverable.
      expect(fs.existsSync(abandoned)).toBe(false);
      const recovered = fs
        .readdirSync(root)
        .map((entry) => path.join(root, entry, "node_modules", "irreplaceable.txt"));
      expect(recovered.filter((file) => fs.existsSync(file))).toHaveLength(1);
    });

    it("does not wait on the lock when there is nothing to recover", async () => {
      // The lock is for mutation. Adopt mode with no holding to recover changes
      // nothing on disk, so making it wait out another run's install would be a
      // cost with nothing on the other side of it — and adopt mode exists to
      // iterate quickly against a tree somebody arranged by hand.
      //
      // The cheap check is not the deciding one: `recoverAbandonedRollbackHoldings`
      // re-reads the directory under the lock, which is where "found here means
      // left by a dead transaction" is true. A holding appearing between the two
      // belongs to a LIVE transaction, and not touching that is the right answer
      // rather than a missed one.
      developerMachine();
      const dir = tempFixture({ name: "adopt-me", dependencies: { vue: "3.5.0" } });
      plantObsoletePackage(dir);
      const child = await childHoldingLock(dir, 60_000);

      // Positive control: a real other process really holds this fixture's lock.
      expect(child.exitCode).toBeNull();
      expect(fs.existsSync(fixtureLockPath(dir))).toBe(true);

      const installer = recordingInstaller();
      const decision = installFixtureDeps(dir, {
        install: installer.run,
        // Short enough that waiting would fail the test rather than hang it.
        lock: { timeoutMs: 250, pollMs: 25 },
        adoptFixtures: [path.basename(dir)],
      });

      expect(decision).toEqual({ install: false, reason: "adopted" });
      expect(installer.calls).toEqual([]);
      // And the other process still owns the lock: this did not take it either.
      expect(fs.existsSync(fixtureLockPath(dir))).toBe(true);
    });

    it("is fixture-scoped: a different fixture's name authorises nothing", () => {
      // Pinned like the cases above: the CI refusal is reached before the name
      // is compared, so this one is decided by the environment too.
      developerMachine();
      const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
      const root = tempDir();
      const obsolete = plantObsoletePackage(dir);
      const decision = installFixtureDeps(dir, {
        ...fast(),
        install: recordingInstaller().run,
        quarantineRoot: root,
        adoptFixtures: ["some-other-fixture"],
      });
      expect(decision.reason).toBe("unstamped-tree");
      expect(fs.existsSync(obsolete)).toBe(false);
    });

    it("is REJECTED under CI", () => {
      // The INJECTED flag. The environment default it stands in for is the case
      // below, and only that one is what a runner actually takes.
      const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
      const installer = recordingInstaller();
      expect(() =>
        installFixtureDeps(dir, {
          ...fast(),
          install: installer.run,
          adoptFixtures: [path.basename(dir)],
          continuousIntegration: true,
        }),
      ).toThrow(/NON-HERMETIC/);
      expect(installer.calls).toEqual([]);
    });

    for (const variable of ["CI", "GITHUB_ACTIONS"] as const) {
      it(`is REJECTED when ${variable} says so and nothing was passed`, () => {
        // The clause a runner reaches, and the one nothing guarded: no caller in
        // this repository passes `continuousIntegration`, so the refusal that
        // makes a CI result mean something is the ENVIRONMENT default alone.
        // Its absence is why four adopt-mode cases could inherit the variable
        // and go red the first time they ran anywhere that sets it.
        developerMachine();
        setEnv(variable, "true");
        const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
        const installer = recordingInstaller();

        expect(() =>
          installFixtureDeps(dir, {
            ...fast(),
            install: installer.run,
            adoptFixtures: [path.basename(dir)],
          }),
        ).toThrow(/NON-HERMETIC/);
        expect(installer.calls).toEqual([]);
      });
    }

    for (const value of ["", "false", "0"]) {
      it(`still adopts when CI is set to ${JSON.stringify(value)}`, () => {
        // The other half, and the reason the refusal reads the VALUE rather than
        // the variable's presence: `CI=false` is how a developer says this is
        // not a runner, and reading it as one would take the override away from
        // the machines it exists for.
        developerMachine();
        setEnv("CI", value);
        const dir = tempFixture({ name: "adopt-me", dependencies: { vue: "3.5.0" } });
        const obsolete = plantObsoletePackage(dir);
        const installer = recordingInstaller();

        expect(
          installFixtureDeps(dir, {
            ...fast(),
            install: installer.run,
            adoptFixtures: [path.basename(dir)],
          }),
        ).toEqual({ install: false, reason: "adopted" });
        expect(installer.calls).toEqual([]);
        expect(fs.existsSync(obsolete)).toBe(true);
      });
    }

    it("takes the fixture list from VERTER_E2E_ADOPT_FIXTURE_DEPS when none is passed", () => {
      // Every case above hands the list in directly, so none of them reaches the
      // way a developer actually turns this on.
      developerMachine();
      const dir = tempFixture({ name: "adopt-me", dependencies: { vue: "3.5.0" } });
      setEnv("VERTER_E2E_ADOPT_FIXTURE_DEPS", ` other-fixture, ${path.basename(dir)} `);
      const installer = recordingInstaller();

      expect(installFixtureDeps(dir, { ...fast(), install: installer.run })).toEqual({
        install: false,
        reason: "adopted",
      });
      expect(installer.calls).toEqual([]);
    });

    it("REFUSES a wildcard: there is no broad authorisation", () => {
      developerMachine();
      const dir = tempFixture({ name: "adopt-me", dependencies: { vue: "3.5.0" } });
      setEnv("VERTER_E2E_ADOPT_FIXTURE_DEPS", "adopt-*");
      const installer = recordingInstaller();

      expect(() => installFixtureDeps(dir, { ...fast(), install: installer.run })).toThrow(
        /must name each fixture/,
      );
      expect(installer.calls).toEqual([]);
    });
  });

  describe("concurrency", () => {
    it("will not touch a tree another LIVE process owns", async () => {
      // The hazard the lock exists for: two runs both decide to replace, and one
      // deletes the tree the other is still installing into. A second process
      // must fail loudly instead of deleting.
      const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
      const obsolete = plantObsoletePackage(dir);
      const child = await childHoldingLock(dir, 60_000);

      // Positive controls: a real process really holds the lock, and there really
      // is a tree that the pre-lock code would have deleted here.
      expect(child.exitCode).toBeNull();
      expect(fs.existsSync(fixtureLockPath(dir))).toBe(true);
      expect(fs.existsSync(obsolete)).toBe(true);

      const installer = recordingInstaller();
      expect(() =>
        installFixtureDeps(dir, {
          install: installer.run,
          lock: { timeoutMs: 250, pollMs: 25 },
        }),
      ).toThrow(/waiting for the fixture dependency lock/);

      expect(installer.calls).toEqual([]);
      expect(fs.existsSync(obsolete)).toBe(true);
    });

    it("is idempotent once a tree is current", () => {
      // Not the lock property — this is the plain repeat call, and it says so.
      // Read as "re-decides under the lock" it would prove nothing: nothing
      // changes between the two calls, so it holds identically whether the
      // decision is taken inside the lock or hoisted above it.
      const dir = tempFixture({ name: "f", dependencies: { vue: "3.5.0" } });
      installFixtureDeps(dir, { ...fast(), install: recordingInstaller().run });

      const second = recordingInstaller();
      expect(installFixtureDeps(dir, { ...fast(), install: second.run })).toEqual({
        install: false,
        reason: "current",
      });
      expect(second.calls).toEqual([]);
      expect(fs.existsSync(path.join(dir, "node_modules", "vue"))).toBe(true);
    });

    it("re-decides under the lock, seeing what the process ahead of it installed", async () => {
      // The real property: a caller that WAITED must act on what it finds after
      // waiting, not on what it saw before. For that, the tree has to change
      // while it waits — which means a second process holding the lock, doing
      // the install, and releasing.
      const manifest = { name: "f", dependencies: { vue: "3.5.0" } };
      const dir = tempFixture(manifest);

      // The tree the other process will install, produced by the production path
      // on an identical manifest so its stamp is valid for `dir` too: both
      // digests are over the manifest BYTES and the package identities, neither
      // of which depends on where the fixture lives.
      const twin = tempFixture(manifest);
      installFixtureDeps(twin, {
        ...fast(),
        install: (target) => {
          recordingInstaller().run(target);
          // A package only the OTHER process installs, so "its tree survived" is
          // a statement about a tree this caller could not have produced. It goes
          // in before the stamp, because it is part of the tree being stamped.
          plantObsoletePackage(target, "installed-by-the-other-process");
        },
      });
      const prepared = path.join(twin, "node_modules");

      const workspace = tempDir();
      const source = path.join(__dirname, "fixtureLock.ts");
      const copy = path.join(workspace, "fixtureLock.mts");
      fs.copyFileSync(source, copy);
      expect(fs.readFileSync(copy, "utf-8")).toBe(fs.readFileSync(source, "utf-8"));
      const held = path.join(workspace, "held");
      const script = path.join(workspace, "holder.mjs");
      fs.writeFileSync(
        script,
        `
        import * as fs from "node:fs";
        import * as path from "node:path";
        import { acquireFixtureLock, releaseFixtureLock } from "./fixtureLock.mts";
        const [subject, prepared, held, holdMs] = process.argv.slice(2);
        const lock = acquireFixtureLock(subject, { timeoutMs: 15000, pollMs: 5 });
        fs.writeFileSync(held, "held");
        const until = Date.now() + Number(holdMs);
        while (Date.now() < until) {}
        fs.cpSync(prepared, path.join(subject, "node_modules"), { recursive: true });
        releaseFixtureLock(lock);
      `,
      );
      const child = spawn(process.execPath, [script, dir, prepared, held, "400"], {
        stdio: "ignore",
      });
      children.push(child);
      await new Promise<void>((resolve, reject) => {
        const started = Date.now();
        const poll = (): void => {
          if (fs.existsSync(held)) return resolve();
          if (Date.now() - started > 20_000) return reject(new Error("holder never took the lock"));
          setTimeout(poll, 5);
        };
        poll();
      });

      // Positive control: at the moment this caller starts, the fixture really
      // does need an install — so a decision taken now says "install", and the
      // test is about what happens to that answer.
      expect(decideFixtureInstall(dir).install).toBe(true);

      const installer = recordingInstaller();
      const decision = installFixtureDeps(dir, {
        install: installer.run,
        lock: { timeoutMs: 20_000, pollMs: 10 },
        quarantineRoot: tempDir(),
      });

      expect(decision).toEqual({ install: false, reason: "current" });
      expect(installer.calls).toEqual([]);
      // The other process's tree is the one still there, untouched.
      expect(fs.existsSync(path.join(dir, "node_modules", "installed-by-the-other-process"))).toBe(
        true,
      );
    });
  });
});
