import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  CLEAN_HOLES,
  CLEAN_REALM,
  CLEAN_SCOPE,
  CLEAN_SHARED,
  DIRTY_HOLES,
  DIRTY_REALM,
  DIRTY_REUSE,
  DIRTY_SCOPE,
  assertRealmLeak,
  assertSvelteShape,
  assertTwoWayHoles,
  evaluateRejectTwins,
  evaluateStp7,
  scanSharedOrigins,
  validateRealmClaim,
  validateScopeClaim,
  validateSharedRecord,
  validateStp7Products,
} from "./protocol.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");

test("STP7-svelte-shape: Svelte 5 Component, generic, each-scope, snippet; no Vue constructor", () => {
  const source = fs.readFileSync(path.join(HERE, "probes", "positive.ts"), "utf8");
  const errors = assertSvelteShape(source);
  assert.equal(errors.length, 0, JSON.stringify(errors));
});

test("STP7-svelte-shape dirty twin: a Vue constructor class is rejected", () => {
  const dirty = [
    "export class Comp { constructor(props?: { n?: number }) {}",
    "export type Instance = InstanceType<typeof Comp>;",
  ].join("\n");
  const errors = assertSvelteShape(dirty);
  assert.ok(errors.some((error) => error.caseId === "STP7-svelte-shape"));
});

test("STP7-reuse: shared origins do not require Vue constructor/ref/directive/emit", () => {
  assert.equal(validateSharedRecord(CLEAN_SHARED).length, 0);
  assert.equal(scanSharedOrigins().length, 0, JSON.stringify(scanSharedOrigins()));
  const dirty = validateSharedRecord(DIRTY_REUSE);
  assert.ok(
    dirty.some((error) => error.caseId === "STP7-reuse" && error.code === "vue-only-required"),
  );
});

test("STP7-holes: authored expressions retain two-way mapping across literal holes", () => {
  assert.equal(assertTwoWayHoles(CLEAN_HOLES).length, 0);
  const dirty = assertTwoWayHoles(DIRTY_HOLES);
  assert.ok(dirty.some((error) => error.caseId === "STP7-holes" && error.code === "one-way-hole"));
});

test("STP7-realm: separate supplemental files in one program are not ambient isolation", () => {
  assert.equal(validateRealmClaim(CLEAN_REALM).length, 0);
  const dirty = validateRealmClaim(DIRTY_REALM);
  assert.ok(
    dirty.some((error) => error.caseId === "STP7-realm" && error.code === "false-isolation"),
  );
});

test("STP7-realm: client observes server ambient when both files share one TS program", () => {
  const tsPath = path.join(REPO_ROOT, "packages", "playground", "node_modules", "typescript");
  const require = createRequire(path.join(tsPath, "package.json"));
  const ts = require(path.join(tsPath, "lib", "typescript.js"));
  const errors = assertRealmLeak(
    ts,
    path.join(HERE, "probes", "contexts", "server.ts"),
    path.join(HERE, "probes", "contexts", "client.ts"),
  );
  assert.equal(errors.length, 0, JSON.stringify(errors));
});

test("STP7-scope-claim: receiving obligations are not full Astro/MDX/Lit support", () => {
  assert.equal(validateScopeClaim(CLEAN_SCOPE).length, 0);
  const dirty = validateScopeClaim(DIRTY_SCOPE);
  assert.ok(
    dirty.some(
      (error) => error.caseId === "STP7-scope-claim" && error.code === "full-support-claim",
    ),
  );
});

test("STP7 products name every mandatory case and AC3/AC4 untouched-owner rationale", () => {
  assert.equal(validateStp7Products().length, 0, JSON.stringify(validateStp7Products()));
});

test("STP7 evaluateStp7: clean twins plus reject dirty twins", async () => {
  const result = await evaluateStp7();
  assert.equal(result.errors.length, 0, JSON.stringify(result.errors, null, 2));
  assert.equal(evaluateRejectTwins().length, 0);
});
