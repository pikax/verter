/**
 * @ai-generated - Exercises the Svelte client executor's process-global DOM setup.
 */

import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { SVELTE_DOMAIN } from "../src/domain-pin.mjs";

const HARNESS_ROOT = path.dirname(path.dirname(fileURLToPath(import.meta.url)));

/**
 * The child's own `spawnSync` deadline. The parent `it(...)` budget below is
 * deliberately LARGER: whichever killer fires first decides what the failure
 * says, and only the child's deadline can report why the child stalled. A
 * parent budget at or below this value kills a cold run mid-flight and reports
 * nothing but "test timed out".
 */
const CHILD_DEADLINE_MS = 30_000;
/** Strictly above {@link CHILD_DEADLINE_MS} — see above. */
const PARENT_BUDGET_MS = 60_000;

const NAVIGATOR_ABSENT_SCRIPT = `
  const navigatorDescriptor = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  if (navigatorDescriptor?.configurable === false) {
    throw new Error("the child process cannot remove its navigator control");
  }
  delete globalThis.navigator;
  const navigatorWasAbsent = !Object.prototype.hasOwnProperty.call(globalThis, "navigator");

  const { compileSvelteFixture } = await import("./src/invoke-svelte-oracle.mjs");
  const { cleanupClientScratch, executeSvelteClient } = await import(
    "./src/execute-svelte-client.mjs"
  );
  const official = compileSvelteFixture("<p>ready</p>", "executor-control.svelte", {
    generate: "client",
    runes: true,
    dev: false,
    sourceMap: false,
  });
  if (official.code === null) throw new Error(JSON.stringify(official.diagnostics));

  const failed = await executeSvelteClient(
    'export default function Broken() { throw new Error("intentional mount failure"); }',
  );
  const control = await executeSvelteClient(official.code);
  cleanupClientScratch();
  process.stdout.write(JSON.stringify({
    navigatorWasAbsent,
    navigatorWasRestored: !Object.prototype.hasOwnProperty.call(globalThis, "navigator"),
    failed,
    control,
  }));
`;

const INITIALIZATION_RETRY_SCRIPT = `
  const { compileSvelteFixture } = await import("./src/invoke-svelte-oracle.mjs");
  const { cleanupClientScratch, executeSvelteClient } = await import(
    "./src/execute-svelte-client.mjs"
  );
  const official = compileSvelteFixture("<p>retry-ready</p>", "executor-retry.svelte", {
    generate: "client",
    runes: true,
    dev: false,
    sourceMap: false,
  });
  if (official.code === null) throw new Error(JSON.stringify(official.diagnostics));

  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  if (originalWindow?.configurable === false) {
    throw new Error("the child process cannot install its window failure plant");
  }
  let windowReads = 0;
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    enumerable: true,
    get() {
      windowReads += 1;
      if (windowReads === 1) throw new Error("intentional shared runtime initialization failure");
      return undefined;
    },
    set(value) {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        enumerable: true,
        writable: true,
        value,
      });
    },
  });

  const originalNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  if (originalNavigator?.configurable === false) {
    throw new Error("the child process cannot install its navigator control");
  }
  const getterOnlyNavigator = { userAgent: "getter-only-control" };
  const navigatorGetter = () => getterOnlyNavigator;
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    enumerable: false,
    get: navigatorGetter,
  });
  const plantedNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");

  const failedInitialization = await executeSvelteClient(official.code);
  const readsAfterFailure = windowReads;
  const retry = await executeSvelteClient(official.code);
  cleanupClientScratch();

  const restoredNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  process.stdout.write(JSON.stringify({
    failedInitialization,
    readsAfterFailure,
    retry,
    windowReads,
    navigatorRestoredExactly:
      restoredNavigator?.configurable === plantedNavigator.configurable &&
      restoredNavigator?.enumerable === plantedNavigator.enumerable &&
      restoredNavigator?.get === plantedNavigator.get &&
      restoredNavigator?.set === plantedNavigator.set,
  }));
`;

describe("Svelte client executor DOM initialization", () => {
  it(
    "installs navigator before the shared runtime initializes and survives a failed mount",
    () => {
      const child = spawnSync(
        process.execPath,
        ["--input-type=module", "--eval", NAVIGATOR_ABSENT_SCRIPT],
        { cwd: HARNESS_ROOT, encoding: "utf8", timeout: CHILD_DEADLINE_MS },
      );

      expect(child.error).toBeUndefined();
      expect(child.status, child.stderr).toBe(0);
      const result = JSON.parse(child.stdout);
      expect(result.navigatorWasAbsent).toBe(true);
      expect(result.navigatorWasRestored).toBe(true);
      expect(result.failed.ok).toBe(false);
      expect(result.failed.error).toContain("intentional mount failure");
      expect(result.failed.error).not.toContain("undefined (reading 'call')");
      expect(result.control.ok, result.control.error).toBe(true);
      expect(result.control.html).toContain("<p>ready</p>");
      // The mount is only evidence about the pinned domain if it bound the
      // pinned runtime. Read the version from the harness's own pin authority,
      // never a literal, so a domain amendment cannot leave this stale.
      expect(result.control.runtime?.version).toBe(SVELTE_DOMAIN.packageVersion);
    },
    PARENT_BUDGET_MS,
  );

  // @ai-generated - Proves a rejected shared-runtime initialization is retryable.
  it(
    "does not memoize a rejected shared runtime initialization",
    () => {
      const child = spawnSync(
        process.execPath,
        ["--input-type=module", "--eval", INITIALIZATION_RETRY_SCRIPT],
        { cwd: HARNESS_ROOT, encoding: "utf8", timeout: CHILD_DEADLINE_MS },
      );

      expect(child.error).toBeUndefined();
      expect(child.status, child.stderr).toBe(0);
      const result = JSON.parse(child.stdout);
      expect(result.failedInitialization.ok).toBe(false);
      expect(result.failedInitialization.error).toContain(
        "the shared client runtime could not initialize",
      );
      expect(result.readsAfterFailure).toBe(1);
      expect(result.windowReads).toBe(2);
      expect(result.retry.ok, result.retry.error).toBe(true);
      expect(result.retry.html).toContain("<p>retry-ready</p>");
      expect(result.navigatorRestoredExactly).toBe(true);
      expect(result.retry.runtime?.version).toBe(SVELTE_DOMAIN.packageVersion);
    },
    PARENT_BUDGET_MS,
  );
});
