/**
 * @ai-generated - Exercises descriptor-safe process-global setup for the Vue Vapor executor.
 */

import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const HARNESS_ROOT = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const CHILD_DEADLINE_MS = 30_000;
const PARENT_BUDGET_MS = 60_000;

const DESCRIPTOR_HELPERS = `
  const DOM_GLOBAL_KEYS = [
    "window",
    "document",
    "navigator",
    "Node",
    "Element",
    "HTMLElement",
    "SVGElement",
    "Text",
    "Comment",
    "DocumentFragment",
    "Event",
    "CustomEvent",
    "MouseEvent",
  ];

  function sameDescriptor(left, right) {
    if (left === undefined || right === undefined) return left === right;
    if (left.configurable !== right.configurable || left.enumerable !== right.enumerable) {
      return false;
    }
    if ("value" in left || "value" in right) {
      return (
        "value" in left &&
        "value" in right &&
        left.value === right.value &&
        left.writable === right.writable
      );
    }
    return left.get === right.get && left.set === right.set;
  }

  function snapshotDescriptors() {
    return new Map(
      DOM_GLOBAL_KEYS.map((key) => [key, Object.getOwnPropertyDescriptor(globalThis, key)]),
    );
  }

  function changedDescriptorKeys(before) {
    return DOM_GLOBAL_KEYS.filter(
      (key) => !sameDescriptor(before.get(key), Object.getOwnPropertyDescriptor(globalThis, key)),
    );
  }
`;

const GETTER_ONLY_NAVIGATOR_SCRIPT = `
  ${DESCRIPTOR_HELPERS}

  const originalNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  if (originalNavigator?.configurable === false) {
    throw new Error("the child process cannot install its navigator control");
  }
  const getterOnlyNavigator = { userAgent: "getter-only-vapor-control" };
  const navigatorGetter = () => getterOnlyNavigator;
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    enumerable: false,
    get: navigatorGetter,
  });
  const plantedNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  const before = snapshotDescriptors();

  const { ensureVaporRuntimePreloaded } = await import("./src/execute-vue-vapor.mjs");
  let bootstrapError = null;
  try {
    await ensureVaporRuntimePreloaded();
  } catch (error) {
    bootstrapError = String(error?.stack ?? error);
  }

  const restoredNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  process.stdout.write(JSON.stringify({
    bootstrapError,
    changedKeys: changedDescriptorKeys(before),
    plantedWasGetterOnly:
      plantedNavigator.get === navigatorGetter &&
      plantedNavigator.set === undefined &&
      plantedNavigator.writable === undefined,
    navigatorGetterPreserved:
      restoredNavigator?.get === navigatorGetter && globalThis.navigator === getterOnlyNavigator,
    navigatorSetterStillAbsent: restoredNavigator?.set === undefined,
  }));
`;

const PARTIAL_INSTALL_ROLLBACK_SCRIPT = `
  ${DESCRIPTOR_HELPERS}

  const originalNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  if (originalNavigator?.configurable === false) {
    throw new Error("the child process cannot install its navigator pass-through control");
  }
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    enumerable: originalNavigator?.enumerable ?? true,
    writable: true,
    value: globalThis.navigator,
  });
  const originalHtmlElement = Object.getOwnPropertyDescriptor(globalThis, "HTMLElement");
  if (originalHtmlElement?.configurable === false) {
    throw new Error("the child process cannot install its HTMLElement failure control");
  }
  Object.defineProperty(globalThis, "HTMLElement", {
    configurable: false,
    enumerable: false,
    writable: false,
    value: { failureControl: true },
  });
  const before = snapshotDescriptors();

  const { ensureVaporRuntimePreloaded } = await import("./src/execute-vue-vapor.mjs");
  let bootstrapError = null;
  try {
    await ensureVaporRuntimePreloaded();
  } catch (error) {
    bootstrapError = String(error?.stack ?? error);
  }

  process.stdout.write(JSON.stringify({
    bootstrapError,
    changedKeys: changedDescriptorKeys(before),
    htmlElementControlPreserved:
      Object.getOwnPropertyDescriptor(globalThis, "HTMLElement")?.value?.failureControl === true,
  }));
`;

function runChild(script) {
  return spawnSync(process.execPath, ["--input-type=module", "--eval", script], {
    cwd: HARNESS_ROOT,
    encoding: "utf8",
    timeout: CHILD_DEADLINE_MS,
  });
}

describe("Vue Vapor executor DOM bootstrap", () => {
  it(
    "runs with a configurable getter-only navigator and restores every descriptor exactly",
    () => {
      const child = runChild(GETTER_ONLY_NAVIGATOR_SCRIPT);

      expect(child.error).toBeUndefined();
      expect(child.status, child.stderr).toBe(0);
      const result = JSON.parse(child.stdout);
      expect(result.plantedWasGetterOnly).toBe(true);
      expect(result.bootstrapError).toBeNull();
      expect(result.changedKeys).toEqual([]);
      expect(result.navigatorGetterPreserved).toBe(true);
      expect(result.navigatorSetterStillAbsent).toBe(true);
    },
    PARENT_BUDGET_MS,
  );

  it(
    "rolls back earlier descriptor installs when a later global cannot be replaced",
    () => {
      const child = runChild(PARTIAL_INSTALL_ROLLBACK_SCRIPT);

      expect(child.error).toBeUndefined();
      expect(child.status, child.stderr).toBe(0);
      const result = JSON.parse(child.stdout);
      expect(result.bootstrapError).toContain("Cannot redefine property: HTMLElement");
      expect(result.changedKeys).toEqual([]);
      expect(result.htmlElementControlPreserved).toBe(true);
    },
    PARENT_BUDGET_MS,
  );
});
