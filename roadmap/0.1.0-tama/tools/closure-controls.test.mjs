/**
 * The control lane.
 *
 * The instrument suite re-applies the negative controls whose bound record runs
 * under `node`. This lane re-applies the rest — the records that run under
 * `cargo`, the one whose command IS the instrument suite, and the command-shaped
 * controls beside them, whose mutation is an argument rather than a file edit.
 * Both halves use the same plant/run/restore routine, and the instrument suite
 * resolves the partition between them, so a control cannot be owned by neither
 * lane and this one cannot quietly cover a subset of what it claims.
 *
 * Two reasons the split is here rather than one suite:
 *
 *   - The roadmap job runs on a checkout with no Rust toolchain and a budget
 *     sized for node tools. Folding cargo re-application into it would either
 *     fail for a missing runner or turn into the skip this register refuses
 *     everywhere else. A separate job installs the toolchain and pays the build
 *     once, under a budget of its own that this lane's deadlines nest inside.
 *   - The control whose command is the instrument suite cannot be driven BY the
 *     instrument suite: that re-enters it. Driven from here it terminates by
 *     construction — the suite this lane spawns drives no control whose command
 *     is itself.
 *
 * The mirror is the whole repository minus its build and dependency output,
 * because a cargo record resolves a workspace rather than a file list. Every
 * installed `node_modules` tree — the repository root and each workspace
 * package's own tree — is linked in rather than copied: a re-applied cargo
 * record can run tests that resolve the workspace's JavaScript toolchain
 * (including nested package `node_modules` canonicalization), and linking keeps
 * the mirror a mutation copy without multiplying gigabytes of installed
 * output. A mutation belongs in a copy, never in the tree under review, where
 * an interrupted run would leave it behind as a real edit.
 *
 * One delegated record transcribes counters shaped by THIS lane's CI platform:
 * its three-package selection contains a `#[cfg(target_os)]`-gated set, so
 * selected, executed, and skipped all move together when the host compiles a
 * different set. Linux and Windows are not symmetric. The transcript is the CI
 * platform's (linux), and the lane that re-applies it in CI re-derives it
 * exactly; a lane run on another OS fails that record's clean-run comparison
 * by exactly those cases, which is the loud, named failure rather than a
 * silent one — re-derive the record on linux, never from another host, and
 * never widen the comparison to tolerate the difference.
 */

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test, { after } from "node:test";
import { fileURLToPath } from "node:url";

import {
  CONTROL_LANE,
  CONTROL_LANE_COMMAND_DEADLINE_MS,
  CONTROL_LANE_DEADLINE_MS,
  CONTROL_LANE_ENTRY,
  MIRROR_OUTPUT_BASENAMES,
  controlsFor,
  installedModuleRelatives,
  linkInstalledModuleTrees,
  reapply,
} from "./closure-controls.mjs";
import { analyze } from "./closure-register.mjs";
import { PACKAGE_ROOT } from "./lib.mjs";

const REPO_ROOT = path.resolve(PACKAGE_ROOT, "..", "..");

/** Output trees, not inputs: copying them would multiply the mirror by gigabytes. */
const EXCLUDED = MIRROR_OUTPUT_BASENAMES;

const MIRRORS = [];
after(() => {
  for (const dir of MIRRORS) fs.rmSync(dir, { recursive: true, force: true });
});

function mirrorRepository() {
  // The mirror lives at a STABLE path, recreated whole on every run, not a
  // fresh `mkdtemp`: a cargo build bakes its source root into the binary —
  // `env!("CARGO_MANIFEST_DIR")` is resolved at compile time — and cargo's
  // fingerprints key on content and mtime, which a byte- and mtime-identical
  // copy of the tree satisfies. A per-run mirror path beside a persistent
  // target directory therefore REUSES yesterday's exe, whose baked root points
  // into a mirror that no longer exists, and the clean run panics on a path
  // that is neither the checkout nor the mirror under mutation. A stable path
  // keeps every baked root the target directory remembers pointing at a tree
  // that is recreated at exactly that path. The recreation is wholesale, so an
  // interrupted run can leave no half-edited mirror behind for the next one.
  const root = path.join(os.tmpdir(), "closure-control-lane");
  fs.rmSync(root, { recursive: true, force: true });
  fs.mkdirSync(root, { recursive: true });
  MIRRORS.push(root);
  fs.cpSync(REPO_ROOT, root, {
    recursive: true,
    filter: (source) => !EXCLUDED.has(path.basename(source)),
  });
  // A cargo record the lane re-applies can run tests that resolve the
  // workspace's `node_modules` — the tsgo-backed typecheck gate locates its rc
  // typescript launcher at the root, and real-tsserver recovery canonicalizes
  // `packages/typescript-plugin/node_modules` (and every other workspace
  // package tree) before spawning. The copy filter drops every basename
  // `node_modules`, so only linking the root leaves those nested trees absent
  // and the clean nextest run panics on a path the checkout still has. The
  // dependency trees are gigabytes of installed output, not source, so each
  // is linked rather than copied: a mutation belongs in a copy, and nothing
  // the lane mutates lives under `node_modules`.
  assert.ok(
    fs.existsSync(path.join(REPO_ROOT, "node_modules")),
    "CONTROL-LANE PREREQUISITE MISSING: the checkout has no `node_modules`, so tests that resolve the workspace JavaScript toolchain cannot run. Install the locked JavaScript toolchain rather than reading this lane as green.",
  );
  linkInstalledModuleTrees(REPO_ROOT, root);
  const nestedPluginModules = path.join("packages", "typescript-plugin", "node_modules");
  assert.ok(
    fs.existsSync(path.join(root, nestedPluginModules)),
    `CONTROL-LANE PREREQUISITE MISSING: the mirror has no ${nestedPluginModules.replaceAll("\\", "/")} after linking installed module trees, so cargo records that canonicalize that path will panic. Link every workspace package tree, not only the repository root.`,
  );
  // ...and tests that ask GIT about the tree they run in. A grep gate walks up
  // from its manifest to a `.git`-bearing ancestor and searches TRACKED files,
  // so a mirror with no git identity panics before it searches anything, and
  // a `git init` with an empty index would pass it vacuously. The mirror
  // therefore gets a repository of its own whose index is staged from exactly
  // the tracked set of the checkout it copies — `git grep` then searches the
  // same bytes it would in the checkout, and a mutation planted in the mirror
  // is as visible to those gates as an edit in the checkout would be. The
  // checkout may be a linked worktree whose `.git` is a pointer file, so its
  // git directory cannot be copied and the index is rebuilt instead.
  const mirrorGit = (args) =>
    spawnSync("git", args, { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  const init = mirrorGit(["init", "-q"]);
  assert.ok(
    init.status === 0,
    `CONTROL-LANE PREREQUISITE MISSING: \`git init\` failed in the mirror, so tests that ask git about the tree cannot run: ${init.stderr}`,
  );
  const tracked = spawnSync("git", ["ls-files", "-z"], {
    cwd: REPO_ROOT,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  assert.equal(
    tracked.status,
    0,
    `\`git ls-files\` failed in the checkout, so the mirror's tracked set cannot be staged: ${tracked.stderr}`,
  );
  const list = path.join(root, "..", `${path.basename(root)}-tracked.list`);
  fs.writeFileSync(
    list,
    `${tracked.stdout
      .split("\0")
      .filter(Boolean)
      .filter((relative) => fs.existsSync(path.join(root, relative)))
      .join("\0")}\0`,
  );
  // `-f` because the checkout tracks paths its own .gitignore also matches —
  // files committed before the rule that matches them landed — and a plain
  // `git add` refuses the whole list over them, so the lane would fail on
  // files that are tracked in the tree under review. The list is the
  // checkout's closed tracked set, so `-f` cannot stage anything outside it.
  const staged = mirrorGit(["add", "-f", "--pathspec-from-file", list, "--pathspec-file-nul"]);
  fs.rmSync(list, { force: true });
  assert.equal(staged.status, 0, `staging the mirror's tracked set failed: ${staged.stderr}`);
  return root;
}

test("the control-lane mirror links nested package node_modules", () => {
  // The copy filter drops every basename `node_modules`. Linking only the
  // repository root leaves `packages/<pkg>/node_modules` absent, and any cargo
  // record that canonicalizes a workspace package's own tree panics in the
  // mirror while passing against the checkout. The link must be a junction to
  // the checkout tree, not a copy: a write through the source is visible in
  // the dest.
  const src = fs.mkdtempSync(path.join(os.tmpdir(), "ctl-modules-src-"));
  const dest = fs.mkdtempSync(path.join(os.tmpdir(), "ctl-modules-dest-"));
  try {
    fs.mkdirSync(path.join(src, "node_modules"));
    fs.writeFileSync(path.join(src, "node_modules", "root.txt"), "root");
    fs.mkdirSync(path.join(src, "packages", "pkg", "node_modules"), { recursive: true });
    fs.writeFileSync(path.join(src, "packages", "pkg", "node_modules", "nested.txt"), "nested");
    fs.mkdirSync(path.join(src, "packages", "pkg", "src"), { recursive: true });
    fs.writeFileSync(path.join(src, "packages", "pkg", "src", "index.ts"), "export {};\n");
    fs.mkdirSync(path.join(src, "target", "foo"), { recursive: true });
    fs.writeFileSync(path.join(src, "target", "foo", "built"), "no");

    fs.cpSync(src, dest, {
      recursive: true,
      filter: (source) => !EXCLUDED.has(path.basename(source)),
    });
    linkInstalledModuleTrees(src, dest);

    assert.equal(fs.readFileSync(path.join(dest, "node_modules", "root.txt"), "utf8"), "root");
    const nestedDest = path.join(dest, "packages", "pkg", "node_modules");
    assert.ok(
      fs.existsSync(nestedDest),
      "nested package node_modules must be linked into the mirror",
    );
    assert.equal(fs.readFileSync(path.join(nestedDest, "nested.txt"), "utf8"), "nested");
    assert.equal(
      fs.realpathSync(nestedDest),
      fs.realpathSync(path.join(src, "packages", "pkg", "node_modules")),
    );
    fs.writeFileSync(path.join(src, "packages", "pkg", "node_modules", "nested.txt"), "updated");
    assert.equal(fs.readFileSync(path.join(nestedDest, "nested.txt"), "utf8"), "updated");
    assert.equal(fs.existsSync(path.join(dest, "target")), false);
  } finally {
    fs.rmSync(src, { recursive: true, force: true });
    fs.rmSync(dest, { recursive: true, force: true });
  }
});

test("installedModuleRelatives collects workspace package trees, not only the root", () => {
  // Recovery tests canonicalize `packages/typescript-plugin/node_modules`.
  // Collecting only the repository root leaves that path absent in the mirror
  // and the clean nextest run panics while the checkout still passes.
  const found = installedModuleRelatives(REPO_ROOT);
  assert.ok(found.includes("node_modules"), "repository root node_modules must be collected");
  const nested = "packages/typescript-plugin/node_modules";
  assert.ok(
    fs.existsSync(path.join(REPO_ROOT, nested)),
    `CONTROL-LANE PREREQUISITE MISSING: ${nested} is absent from the checkout`,
  );
  assert.ok(
    found.includes(nested),
    "a workspace package's own node_modules must be collected, not only the repository root",
  );
  assert.ok(
    found.every((rel) => !rel.split("/").slice(0, -1).includes("node_modules")),
    "discovery must not descend into a collected node_modules tree",
  );
});

// A control this lane owns names a runner that is not on `PATH` in every
// environment. That is a PREREQUISITE, never a skip: a lane that quietly passes
// when its runner is missing reports exactly the false green this register
// exists to refuse. It fails loudly instead, naming what to install.
function requireRunner(runner) {
  const probe = spawnSync(runner, ["--version"], { encoding: "utf8", shell: false });
  assert.ok(
    !probe.error && probe.status === 0,
    `CONTROL-LANE PREREQUISITE MISSING: \`${runner}\` is not runnable, so the controls whose records run under it cannot be re-applied. Install it rather than reading this lane as green.`,
  );
}

// A runnable `cargo` is not yet a runnable `cargo nextest`: the subcommand is
// installed beside the toolchain, not with it, and a lane that failed on a
// missing subcommand would report the record's clean run as refused — the
// shape of a control that proves nothing — instead of naming the prerequisite.
function requireSubcommand(adapterIds) {
  if (!adapterIds.includes("cargo-nextest")) return;
  const probe = spawnSync("cargo", ["nextest", "--version"], { encoding: "utf8", shell: false });
  assert.ok(
    !probe.error && probe.status === 0,
    "CONTROL-LANE PREREQUISITE MISSING: `cargo nextest` is not runnable, so the record whose command is a nextest run cannot be re-applied. Install cargo-nextest rather than reading this lane as green.",
  );
}

test(
  "every control the instrument suite delegates is re-applied and refused",
  { timeout: CONTROL_LANE_DEADLINE_MS },
  (t) => {
    const laneStarted = Date.now();
    const { model } = analyze(PACKAGE_ROOT);
    const instrumentEntry = "roadmap/0.1.0-tama/tools/closure-register.test.mjs";
    const owned = controlsFor(model, CONTROL_LANE, instrumentEntry);
    assert.ok(owned.length >= 1, "the live register delegates no control to this lane");

    const runners = new Set(
      owned.map(
        (row) =>
          model.register.adapter.find((row2) => row2.id === boundAdapterId(model, row)).runner,
      ),
    );
    for (const runner of [...runners].sort()) requireRunner(runner);
    // The mirror carries a git identity of its own for the grep gates inside
    // the lane's cargo records, so git is a prerequisite of the lane itself
    // rather than of any one record's runner.
    requireRunner("git");
    requireSubcommand(owned.map((row) => boundAdapterId(model, row)));

    const mirror = mirrorRepository();
    // The lane's cargo work builds into a directory of its OWN under the
    // checkout's target root — never the checkout's default `target/` itself,
    // and never a directory inside the per-run mirror. Three reasons, and the
    // first is the one that bites hardest:
    //
    //   - a cargo build bakes its source root into the binary
    //     (`env!("CARGO_MANIFEST_DIR")` is a compile-time read), and cargo's
    //     fingerprints are content- and mtime-based, so a byte-identical tree
    //     at a different path reuses whatever exe the target directory last
    //     built. Sharing one target directory between the checkout and
    //     per-run mirrors therefore runs exes whose baked root is some other
    //     tree — a deleted mirror, or the checkout where a planted mutation
    //     does not exist — and lets a lane build leave binaries that poison
    //     the checkout's own `cargo run` of the same crate. A lane-owned
    //     directory keeps the two bake sources from ever meeting, and the
    //     mirror's stable path (above) keeps this directory's own exes valid
    //     across runs.
    //   - the hosting job restores a cached build under `target/`
    //     (Swatinem/rust-cache with a shared key), and the lane's directory
    //     lives inside that tree, so its second and later runs are warm: each
    //     cargo control pays a clean run plus a mutated one, against a budget
    //     this lane's deadlines nest inside.
    //   - a fresh target dir per mirror would instead multiply gigabytes of
    //     build output under the temp root and compile cold on every run.
    //
    // Build OUTPUT only ever flows into the mirror through this variable; the
    // tree under review is never written to, and the mirror still excludes
    // `target` from its copy.
    const env = {
      ...process.env,
      CARGO_TARGET_DIR: path.join(REPO_ROOT, "target", "closure-controls"),
      CARGO_TERM_COLOR: "never",
    };
    // The runner must not inherit this process's test context: a nested node:test
    // run would report into this one instead of returning its own terminal block,
    // and the block is what the refusal is derived from.
    delete env.NODE_TEST_CONTEXT;
    // The register declares the runner; the argument vector starts at a
    // subcommand, so reading the program out of it would be a guess.
    const spawn = (argv, adapter) =>
      spawnSync(adapter.runner === "node" ? process.execPath : adapter.runner, argv, {
        cwd: mirror,
        encoding: "utf8",
        env,
        timeout: CONTROL_LANE_COMMAND_DEADLINE_MS,
        maxBuffer: 64 * 1024 * 1024,
      });

    let reApplied = 0;
    for (const control of owned) {
      const started = Date.now();
      const { argv, adapter } = reapply({ model, control, mirror, spawn });
      // Named, not counted. A lane that reports only a total cannot be told apart
      // from one that ran a subset, and "every control executes" is the property
      // this file exists to make checkable by whoever replays it.
      const mutated =
        control.kind === "source"
          ? control.subject
          : `command plus ${control.argv_delta.join(" ")}`;
      t.diagnostic(
        `re-applied ${control.id} against ${mutated} via ${adapter.runner} ${argv.join(" ")} in ${Date.now() - started}ms`,
      );
      reApplied += 1;
    }

    // An exact count, not a floor: a control delegated here without being
    // re-applied would otherwise pass on the strength of the ones that were.
    assert.equal(
      reApplied,
      owned.length,
      `every delegated control must be re-applied; ran ${reApplied} of ${owned.length}`,
    );

    // The lane mirrors the repository from scratch on every run and its cargo
    // work compiles the selection a mutation touches, so its cost grows with
    // every cargo-bound control added to it. Reported against the deadline it
    // is nested inside, so whoever adds the control that makes this lane too
    // big for its budget reads how much headroom was left rather than
    // discovering it as a runner kill months later.
    const elapsed = Date.now() - laneStarted;
    t.diagnostic(
      `re-applied ${reApplied} delegated controls in ${elapsed}ms, ${Math.round((elapsed / CONTROL_LANE_DEADLINE_MS) * 100)}% of this lane's ${CONTROL_LANE_DEADLINE_MS}ms deadline`,
    );
  },
);

test("this lane is the file the instrument suite delegates to", () => {
  // `fileURLToPath`, never `URL.pathname`: a pathname is percent-encoded, so a
  // checkout under a directory containing a space, `#`, `%` or `?` yields
  // `.../Jane%20Doe/...` and this comparison fails on a developer machine while
  // staying green on a runner whose paths happen to have none of those.
  assert.equal(
    path.relative(REPO_ROOT, fileURLToPath(import.meta.url)).replaceAll("\\", "/"),
    CONTROL_LANE_ENTRY,
  );
});

/** The adapter id of the record a control is bound to. */
function boundAdapterId(model, control) {
  const proof = model.register.proof.find((row) => row.control === control.id);
  assert.ok(proof, `control ${control.id} is bound to no record`);
  return proof.adapter;
}
