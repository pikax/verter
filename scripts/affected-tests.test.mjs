#!/usr/bin/env node

// Tests for affected-tests.mjs. Run: node --test scripts/affected-tests.test.mjs
//
// decideSelection/buildNextestArgv are pure and tested against synthetic
// fixtures. getChangedFiles and `main` (via --json against a temp git repo)
// are exercised end-to-end against a real, throwaway git repository so the
// git plumbing itself is covered, not just the pure decision core.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildWorkspaceIndex, buildReverseDependencyGraph } from "./lib/crate-graph.mjs";
import {
  decideSelection,
  buildNextestArgv,
  getChangedFiles,
  main,
  EMPTY_SELECTION_EXIT_CODE,
} from "./affected-tests.mjs";

const ROOT = "/repo";

function fixtureMetadata(pkgs) {
  const idOf = (name) => `path+file://${ROOT}/${pkgs.find((p) => p.name === name).dir}#0.0.0`;
  return {
    workspace_root: ROOT,
    workspace_members: pkgs.map((p) => idOf(p.name)),
    packages: pkgs.map((p) => ({
      id: idOf(p.name),
      name: p.name,
      manifest_path: `${ROOT}/${p.dir}/Cargo.toml`,
      targets: [{ kind: p.procMacro ? ["proc-macro"] : ["lib"] }],
    })),
    resolve: {
      nodes: pkgs.map((p) => ({
        id: idOf(p.name),
        deps: (p.deps ?? []).map((d) => ({
          name: d.name,
          pkg: idOf(d.name),
          dep_kinds: [{ kind: d.kind ?? null, target: null }],
        })),
      })),
    },
  };
}

const PKGS = [
  { name: "a", dir: "crates/a" },
  { name: "b", dir: "crates/b", deps: [{ name: "a" }] },
  { name: "c", dir: "crates/c", deps: [{ name: "b" }] },
  { name: "d", dir: "crates/d", deps: [{ name: "a", kind: "dev" }] },
  { name: "e", dir: "crates/e" },
];

function buildFixture() {
  const metadata = fixtureMetadata(PKGS);
  const index = buildWorkspaceIndex(metadata);
  const reverse = buildReverseDependencyGraph(metadata, index);
  return { index, reverse };
}

test("decideSelection: a single crate's own change selects it plus its transitive dependents", () => {
  const { index, reverse } = buildFixture();
  const result = decideSelection(["crates/a/src/lib.rs"], index, reverse);
  assert.equal(result.full, false);
  assert.deepEqual(result.directCrates, ["a"]);
  assert.deepEqual(result.selectedCrates, ["a", "b", "c", "d"]);
});

test("decideSelection: a dev-dependency-only edge is still selected (test-only cross-crate coverage)", () => {
  const { index, reverse } = buildFixture();
  const result = decideSelection(["crates/a/src/lib.rs"], index, reverse);
  assert.ok(result.selectedCrates.includes("d"), "d depends on a only via dev-dependency");
});

test("decideSelection: an isolated crate's change selects only itself", () => {
  const { index, reverse } = buildFixture();
  const result = decideSelection(["crates/e/src/lib.rs"], index, reverse);
  assert.deepEqual(result.selectedCrates, ["e"]);
});

test("decideSelection: multiple changed crates union their transitive dependents", () => {
  const { index, reverse } = buildFixture();
  const result = decideSelection(["crates/c/src/lib.rs", "crates/e/src/lib.rs"], index, reverse);
  assert.deepEqual(result.directCrates, ["c", "e"]);
  assert.deepEqual(result.selectedCrates, ["c", "e"]);
});

test("decideSelection: an escape-hatch file forces full and reports the reason", () => {
  const { index, reverse } = buildFixture();
  const result = decideSelection(["Cargo.toml", "crates/a/src/lib.rs"], index, reverse);
  assert.equal(result.full, true);
  assert.equal(result.fullReasons.length, 1);
  assert.equal(result.fullReasons[0].id, "workspace-manifest");
});

test("decideSelection: an unrecognized path forces full (over-select)", () => {
  const { index, reverse } = buildFixture();
  const result = decideSelection(["mystery-dir/file"], index, reverse);
  assert.equal(result.full, true);
  assert.equal(result.fullReasons[0].id, "unrecognized-path");
});

test("decideSelection: a known non-Rust path alone selects nothing and does not force full", () => {
  const { index, reverse } = buildFixture();
  const result = decideSelection(["docs/readme.md"], index, reverse);
  assert.equal(result.full, false);
  assert.deepEqual(result.selectedCrates, []);
  assert.deepEqual(result.ignoredFiles, ["docs/readme.md"]);
});

test("buildNextestArgv emits one -p flag per selected package", () => {
  assert.deepEqual(buildNextestArgv(["a", "b"]), ["nextest", "run", "-p", "a", "-p", "b"]);
  assert.deepEqual(buildNextestArgv([]), ["nextest", "run"]);
});

// --- Real git plumbing: getChangedFiles + main() against a throwaway repo ---

function git(args, cwd) {
  return execFileSync("git", args, { cwd }).toString("utf8");
}

function makeTempRepo() {
  const dir = mkdtempSync(join(tmpdir(), "affected-tests-"));
  git(["init", "-q"], dir);
  git(["config", "user.email", "test@example.com"], dir);
  git(["config", "user.name", "Test"], dir);
  mkdirSync(join(dir, "crates", "a"), { recursive: true });
  writeFileSync(join(dir, "crates", "a", "lib.rs"), "// a\n");
  git(["add", "-A"], dir);
  git(["commit", "-q", "-m", "init"], dir);
  return dir;
}

test("getChangedFiles: sees a tracked modification against HEAD", () => {
  const dir = makeTempRepo();
  try {
    writeFileSync(join(dir, "crates", "a", "lib.rs"), "// changed\n");
    const files = getChangedFiles(dir, null);
    assert.deepEqual(files, ["crates/a/lib.rs"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("getChangedFiles: sees an untracked new file", () => {
  const dir = makeTempRepo();
  try {
    mkdirSync(join(dir, "crates", "b"), { recursive: true });
    writeFileSync(join(dir, "crates", "b", "lib.rs"), "// new\n");
    const files = getChangedFiles(dir, null);
    assert.deepEqual(files, ["crates/b/lib.rs"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("getChangedFiles: --range diffs an explicit commit range instead of the working tree", () => {
  const dir = makeTempRepo();
  try {
    writeFileSync(join(dir, "crates", "a", "lib.rs"), "// second commit\n");
    git(["commit", "-aq", "-m", "second"], dir);
    const files = getChangedFiles(dir, "HEAD~1..HEAD");
    assert.deepEqual(files, ["crates/a/lib.rs"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("getChangedFiles: reports both source and destination path on a cross-crate git mv", () => {
  const dir = makeTempRepo();
  try {
    mkdirSync(join(dir, "crates", "b"), { recursive: true });
    git(["mv", "crates/a/lib.rs", "crates/b/lib.rs"], dir);
    const files = getChangedFiles(dir, null);
    // With rename detection on (git's default), this single move would report
    // only "crates/b/lib.rs" — silently dropping crate a's deletion from the
    // change set. --no-renames must force it to report both paths so both
    // crate a and crate b get classified.
    assert.ok(files.includes("crates/a/lib.rs"), "source path must be reported as deleted");
    assert.ok(files.includes("crates/b/lib.rs"), "destination path must be reported as added");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("getChangedFiles: --range reports both source and destination path on a cross-crate git mv", () => {
  const dir = makeTempRepo();
  try {
    mkdirSync(join(dir, "crates", "b"), { recursive: true });
    git(["mv", "crates/a/lib.rs", "crates/b/lib.rs"], dir);
    git(["commit", "-q", "-m", "move a to b"], dir);
    const files = getChangedFiles(dir, "HEAD~1..HEAD");
    assert.ok(files.includes("crates/a/lib.rs"), "source path must be reported as deleted");
    assert.ok(files.includes("crates/b/lib.rs"), "destination path must be reported as added");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("main --help exits 0 and prints the non-canonical-gate banner", () => {
  let out = "";
  const originalWrite = process.stdout.write.bind(process.stdout);
  process.stdout.write = (chunk) => {
    out += chunk;
    return true;
  };
  let code;
  try {
    code = main(["--help"]);
  } finally {
    process.stdout.write = originalWrite;
  }
  assert.equal(code, 0);
  assert.match(out, /NOT the canonical gate/);
  assert.match(out, /gate\.mjs/);
});

// --- Empty-selection coherence: real cargo workspace, docs-only change ---

function makeCargoTempRepo() {
  const dir = mkdtempSync(join(tmpdir(), "affected-tests-cargo-"));
  git(["init", "-q"], dir);
  git(["config", "user.email", "test@example.com"], dir);
  git(["config", "user.name", "Test"], dir);
  writeFileSync(join(dir, "Cargo.toml"), '[workspace]\nmembers = ["crates/a"]\n');
  mkdirSync(join(dir, "crates", "a", "src"), { recursive: true });
  writeFileSync(
    join(dir, "crates", "a", "Cargo.toml"),
    '[package]\nname = "a"\nversion = "0.0.1"\nedition = "2021"\n',
  );
  writeFileSync(join(dir, "crates", "a", "src", "lib.rs"), "");
  git(["add", "-A"], dir);
  git(["commit", "-q", "-m", "init"], dir);
  return dir;
}

test("main: a docs-only change selects zero crates, never runs a bare nextest, and fails the exit code", () => {
  const dir = makeCargoTempRepo();
  try {
    mkdirSync(join(dir, "docs"), { recursive: true });
    writeFileSync(join(dir, "docs", "readme.md"), "# notes\n");

    let out = "";
    const originalWrite = process.stdout.write.bind(process.stdout);
    process.stdout.write = (chunk) => {
      out += chunk;
      return true;
    };
    let runCalled = false;
    const spyRun = () => {
      runCalled = true;
      throw new Error("run() must never be invoked when selection is genuinely empty");
    };
    let code;
    try {
      code = main(["--run", "--json"], { cwd: dir, run: spyRun });
    } finally {
      process.stdout.write = originalWrite;
    }

    // A bare `cargo nextest run` (no -p, no --workspace) is NOT "run
    // nothing" — with no default-members it silently runs the WHOLE
    // workspace. Zero selected crates must never reach that command.
    assert.equal(runCalled, false, "cargo must never be invoked for an empty selection");
    assert.equal(code, EMPTY_SELECTION_EXIT_CODE);
    const report = JSON.parse(out);
    assert.equal(report.full, false);
    assert.deepEqual(report.selectedCrates, []);
    assert.equal(report.nothingToTest, true);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("main --range requires a value", () => {
  let err = "";
  const originalWrite = process.stderr.write.bind(process.stderr);
  process.stderr.write = (chunk) => {
    err += chunk;
    return true;
  };
  let code;
  try {
    code = main(["--range"]);
  } finally {
    process.stderr.write = originalWrite;
  }
  assert.equal(code, 2);
  assert.match(err, /--range requires a value/);
});
