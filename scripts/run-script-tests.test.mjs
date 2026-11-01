#!/usr/bin/env node

// Tests for run-script-tests.mjs. Run: node --test scripts/run-script-tests.test.mjs
//
// The resolver and executor are tested against synthetic fragment corpora in
// temporary directories: filename ordering, entry ordering, CI filtering and
// ciIndex placement, stop-at-first-failure exit propagation, per-reason
// loader rejections, and late fragment addition without a package.json edit.
// Fixture entries register real repository files because the loader rejects a
// missing file; one test resolves the real registry as a sanity check.

import assert from "node:assert/strict";
import fs, { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  executeScriptTests,
  resolveScriptTests,
  ScriptTestFragmentError,
} from "./run-script-tests.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// Real files, distinct per corpus so duplicate-registration checks stay honest.
const F = {
  a: "scripts/affected-tests.mjs",
  b: "scripts/bench-bootstrap-ci.mjs",
  c: "scripts/bf2-authoritative.mjs",
  d: "scripts/build-host.mjs",
  e: "scripts/bump.mjs",
  stp: "tests/sfc-projection/STP5/mapper.spec.ts",
  vitest: "scripts/sccache-env.test.mjs",
  nodeTest: "scripts/typescript-pin.test.mjs",
};

function makeFragmentsDir() {
  return mkdtempSync(path.join(tmpdir(), "run-script-tests-selftest-"));
}

function writeFragment(dir, name, fragment) {
  const body = typeof fragment === "string" ? fragment : JSON.stringify(fragment);
  writeFileSync(path.join(dir, name), body + "\n");
}

function rejection(dir, build, reason) {
  writeFragment(dir, "00-bad.json", build());
  assert.throws(
    () => resolveScriptTests({ fragmentsDir: dir }),
    (error) => error instanceof ScriptTestFragmentError && error.reason === reason,
  );
}

// A scratch directory inside the repository: registered `file` paths are
// validated as repo-relative, so execution fixtures must live under the root.
function makeScratchScripts(t) {
  const dir = mkdtempSync(path.join(repoRoot, "run-script-tests-selftest-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  return path.basename(dir);
}

test("fragments load in byte-wise filename order, entries in array order", (t) => {
  const dir = makeFragmentsDir();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const entry = (file) => ({ runner: "node", file, ci: false });
  writeFragment(dir, "02-second.json", { owner: "b", tests: [entry(F.c)] });
  writeFragment(dir, "10-third.json", { owner: "c", tests: [entry(F.d)] });
  writeFragment(dir, "01-first.json", {
    owner: "a",
    tests: [entry(F.a), entry(F.b)],
  });
  const files = resolveScriptTests({ fragmentsDir: dir }).map((e) => e.file);
  assert.deepEqual(files, [F.a, F.b, F.c, F.d]);
});

test("execution stops at the first failure and propagates its exit code", (t) => {
  const scratch = makeScratchScripts(t);
  writeFileSync(path.join(repoRoot, scratch, "ok.mjs"), "process.exit(0);\n");
  writeFileSync(path.join(repoRoot, scratch, "fail-7.mjs"), "process.exit(7);\n");
  const marker = path.join(repoRoot, scratch, "ran");
  writeFileSync(
    path.join(repoRoot, scratch, "never.mjs"),
    `import { writeFileSync } from "node:fs";\nwriteFileSync(${JSON.stringify(marker)}, "1");\n`,
  );
  const dir = makeFragmentsDir();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const entry = (file) => ({ runner: "node", file, ci: false });
  writeFragment(dir, "00-run.json", {
    owner: "exec",
    tests: [
      entry(`${scratch}/ok.mjs`),
      entry(`${scratch}/fail-7.mjs`),
      entry(`${scratch}/never.mjs`),
    ],
  });
  const entries = resolveScriptTests({ fragmentsDir: dir });
  assert.equal(executeScriptTests(entries), 7);
  assert.equal(fs.existsSync(marker), false);
});

test("a fully passing corpus exits 0 in both modes", (t) => {
  const scratch = makeScratchScripts(t);
  writeFileSync(path.join(repoRoot, scratch, "ok.mjs"), "process.exit(0);\n");
  writeFileSync(path.join(repoRoot, scratch, "ok2.mjs"), "process.exit(0);\n");
  const dir = makeFragmentsDir();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFragment(dir, "00-run.json", {
    owner: "exec",
    tests: [
      { runner: "node", file: `${scratch}/ok.mjs`, ci: false },
      { runner: "node", file: `${scratch}/ok2.mjs`, ci: true },
    ],
  });
  assert.equal(executeScriptTests(resolveScriptTests({ fragmentsDir: dir })), 0);
  assert.equal(executeScriptTests(resolveScriptTests({ ci: true, fragmentsDir: dir })), 0);
});

test("--ci selects exactly the ci-marked entries", (t) => {
  const dir = makeFragmentsDir();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFragment(dir, "00-mixed.json", {
    owner: "mixed",
    tests: [
      { runner: "node", file: F.a, ci: true },
      { runner: "node", file: F.b, ci: false },
      { runner: "node", file: F.c, ci: true },
    ],
  });
  const ciFiles = resolveScriptTests({ ci: true, fragmentsDir: dir }).map((e) => e.file);
  assert.deepEqual(ciFiles, [F.a, F.c]);
});

test("ciIndex orders the CI lane; unindexed entries follow in fragment order", (t) => {
  const dir = makeFragmentsDir();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  // Mirrors the baseline shape: an entry can run early in the CI lane
  // (ciIndex 2) while sitting late in the full chain.
  writeFragment(dir, "00-ordered.json", {
    owner: "order",
    tests: [
      { runner: "node", file: F.a, ci: true, ciIndex: 1 },
      { runner: "node", file: F.b, ci: true, ciIndex: 3 },
      { runner: "node", file: F.c, ci: false },
      { runner: "node", file: F.d, ci: true, ciIndex: 2 },
      { runner: "node", file: F.e, ci: true },
    ],
  });
  const ciFiles = resolveScriptTests({ ci: true, fragmentsDir: dir }).map((e) => e.file);
  assert.deepEqual(ciFiles, [F.a, F.d, F.b, F.e]);
  const fullFiles = resolveScriptTests({ fragmentsDir: dir }).map((e) => e.file);
  assert.deepEqual(fullFiles, [F.a, F.b, F.c, F.d, F.e]);
});

test("commands render exactly as the replaced chain spelled them", (t) => {
  const dir = makeFragmentsDir();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFragment(dir, "00-render.json", {
    owner: "render",
    tests: [
      { runner: "node-test", file: F.stp, ci: false, nodeFlags: ["--experimental-strip-types"] },
      { runner: "node-test", file: F.nodeTest, ci: false },
      { runner: "vitest", file: F.vitest, ci: false },
      { runner: "node", file: F.a, ci: false },
      { runner: "pnpm", script: "test:generic-interface-identity-oracle", ci: false },
    ],
  });
  const displays = resolveScriptTests({ fragmentsDir: dir }).map((e) => e.display);
  assert.deepEqual(displays, [
    `node --experimental-strip-types --test ${F.stp}`,
    `node --test ${F.nodeTest}`,
    `vitest run ${F.vitest}`,
    `node ${F.a}`,
    "pnpm run test:generic-interface-identity-oracle",
  ]);
});

test("loader rejections each carry their own reason", (t) => {
  const dir = makeFragmentsDir();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  rejection(dir, () => "{ not json", "malformed-json");
  rejection(dir, () => ({ tests: [{ runner: "node", file: F.a, ci: false }] }), "missing-owner");
  rejection(
    dir,
    () => ({ owner: "x", tests: [{ runner: "node-planet", file: F.a, ci: false }] }),
    "unknown-runner",
  );
  rejection(
    dir,
    () => ({
      owner: "x",
      tests: [{ runner: "node", file: "scripts/does-not-exist.mjs", ci: false }],
    }),
    "missing-file",
  );
  rejection(
    dir,
    () => ({
      owner: "x",
      tests: [
        { runner: "node", file: F.a, ci: false },
        { runner: "vitest", file: F.a, ci: true },
      ],
    }),
    "duplicate-file",
  );
  rejection(
    dir,
    () => ({
      owner: "x",
      tests: [
        { runner: "pnpm", script: "fmt", ci: false },
        { runner: "pnpm", script: "fmt", ci: true },
      ],
    }),
    "duplicate-script",
  );
  rejection(dir, () => ({ owner: "x", tests: [{ runner: "node", file: F.a }] }), "missing-ci");
  rejection(
    dir,
    () => ({ owner: "x", tests: [{ runner: "node", file: F.a, ci: false, oops: 1 }] }),
    "unknown-entry-key",
  );
  rejection(
    dir,
    () => ({ owner: "x", tests: [{ runner: "pnpm", file: F.a, script: "fmt", ci: false }] }),
    "invalid-entry-shape",
  );
  rejection(
    dir,
    () => ({ owner: "x", tests: [{ runner: "node", file: F.a, ci: false, ciIndex: 2 }] }),
    "invalid-entry-shape",
  );
  rejection(
    dir,
    () => ({ owner: "x", tests: [{ runner: "node", file: "scripts", ci: false }] }),
    "invalid-file",
  );
});

test("a duplicate registration across two fragments is rejected", (t) => {
  const dir = makeFragmentsDir();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFragment(dir, "01-a.json", {
    owner: "a",
    tests: [{ runner: "node", file: F.a, ci: false }],
  });
  writeFragment(dir, "02-b.json", {
    owner: "b",
    tests: [{ runner: "node", file: F.a, ci: false }],
  });
  assert.throws(
    () => resolveScriptTests({ fragmentsDir: dir }),
    (error) => error instanceof ScriptTestFragmentError && error.reason === "duplicate-file",
  );
});

test("a fragment added later runs without any edit to package.json", (t) => {
  const dir = makeFragmentsDir();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const pkgBefore = fs.readFileSync(path.join(repoRoot, "package.json"));
  writeFragment(dir, "01-first.json", {
    owner: "a",
    tests: [{ runner: "node", file: F.a, ci: false }],
  });
  assert.equal(resolveScriptTests({ fragmentsDir: dir }).length, 1);
  writeFragment(dir, "02-late.json", {
    owner: "late-node",
    tests: [{ runner: "node", file: F.b, ci: false }],
  });
  const entries = resolveScriptTests({ fragmentsDir: dir });
  assert.deepEqual(
    entries.map((e) => [e.owner, e.file]),
    [
      ["a", F.a],
      ["late-node", F.b],
    ],
  );
  assert.equal(fs.readFileSync(path.join(repoRoot, "package.json")).equals(pkgBefore), true);
});

test("the real registry resolves with the CI lane a subset of the full lane", () => {
  const full = resolveScriptTests({ ci: false });
  const ci = resolveScriptTests({ ci: true });
  assert.ok(full.length > 0);
  assert.ok(ci.length > 0 && ci.length < full.length);
  for (const entry of ci) {
    assert.equal(entry.ci, true);
    assert.ok(full.some((e) => e.file === entry.file && e.runner === entry.runner));
  }
  const displays = new Set(full.map((e) => e.display));
  assert.equal(displays.size, full.length);
});
