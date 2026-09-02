// Tests for packages/unplugin/scripts/probe-bundler-route-isolation.mjs.
// Run with: node --test scripts/probe-bundler-route-isolation.test.mjs
//
// Locks two probe-run contracts that previously false-greened:
//   1. each invocation allocates a unique fixture directory (a shared
//      `.verter-probe-recompile` path is a collision);
//   2. any required lane that records outcome:"error" — including an
//      exportCase — fails the run rather than exiting 0 with fresh:true.

import { test, after } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, basename } from "node:path";

const ISOLATION = new URL(
  "../packages/unplugin/scripts/probe-bundler-route-isolation.mjs",
  import.meta.url,
);

const scratch = mkdtempSync(join(tmpdir(), "probe-isolation-"));
after(() => {
  rmSync(scratch, { recursive: true, force: true });
});

test("concurrent fixture allocations never share a path", async () => {
  const { allocateRecompileFixture } = await import(ISOLATION.href);
  const parent = join(scratch, "fixtures");
  const roots = await Promise.all(
    Array.from({ length: 6 }, () => allocateRecompileFixture(parent)),
  );
  assert.equal(new Set(roots).size, 6, `colliding fixture roots: ${roots.join(", ")}`);
  for (const root of roots) {
    assert.equal(
      basename(root).startsWith("recompile-"),
      true,
      `fixture leaf is not per-invocation: ${root}`,
    );
    assert.notEqual(
      basename(root),
      ".verter-probe-recompile",
      `shared probe-recompile path is a collision: ${root}`,
    );
  }
});

test("an errored exportCase fails the run — scanning only cases is a false green", async () => {
  const { collectErroredCaseLabels, probeExitCode } = await import(ISOLATION.href);
  const labels = collectErroredCaseLabels(
    { vueRecompileLane: { outcome: "buildStarted" } },
    { VerterVue: { outcome: "error", message: "ENOENT" } },
  );
  assert.deepEqual(labels, ["exportCase.VerterVue"]);
  assert.equal(probeExitCode(labels), 1);
});

test("an errored required case fails the run even when fresh is true", async () => {
  const { collectErroredCaseLabels, probeExitCode } = await import(ISOLATION.href);
  const labels = collectErroredCaseLabels(
    { vueRecompileLane: { outcome: "error", message: "ENOENT: Parent.vue" } },
    { VerterVue: { outcome: "transformed" } },
  );
  assert.deepEqual(labels, ["vueRecompileLane"]);
  assert.equal(probeExitCode(labels), 1);
});

test("a clean record exits 0", async () => {
  const { collectErroredCaseLabels, probeExitCode } = await import(ISOLATION.href);
  const labels = collectErroredCaseLabels(
    { vueRecompileLane: { outcome: "buildStarted" } },
    { VerterVue: { outcome: "transformed" } },
  );
  assert.deepEqual(labels, []);
  assert.equal(probeExitCode(labels), 0);
});
