import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import {
  loadGeneratedComponentRegistry,
  readComponentSourceForTrace,
  resolveComponentFile,
} from "./trace-component-resolver.js";

const uiRoot = resolve(import.meta.dirname, "../../../.integration-tests/repos/nuxt-ui");

describe("loadGeneratedComponentRegistry", () => {
  it("maps generated prose component names to nested source files", () => {
    const registry = loadGeneratedComponentRegistry(uiRoot);

    expect(registry.get("ProseCaution")?.replace(/\\/g, "/")).toContain(
      "/src/runtime/components/prose/callout/Caution.vue",
    );
  });
});

describe("resolveComponentFile", () => {
  it("accepts benchmark-relative vue paths directly", () => {
    const file = resolveComponentFile("src/runtime/components/prose/callout/Caution.vue", {
      uiRoot,
    });

    expect(file.replace(/\\/g, "/")).toContain("/src/runtime/components/prose/callout/Caution.vue");
  });

  it("preserves an exact relative vue path even when the basename is duplicated elsewhere", () => {
    const file = resolveComponentFile("src/runtime/components/prose/Card.vue", { uiRoot });

    expect(file.replace(/\\/g, "/")).toContain("/src/runtime/components/prose/Card.vue");
  });

  it("accepts absolute vue paths directly", () => {
    const absolutePath = resolve(uiRoot, "src/runtime/components/prose/callout/Caution.vue");
    const file = resolveComponentFile(absolutePath, { uiRoot });

    expect(file.replace(/\\/g, "/")).toBe(absolutePath.replace(/\\/g, "/"));
  });

  it("resolves prose subdirectory components from their docs token", () => {
    const file = resolveComponentFile("Caution", { uiRoot });

    expect(file.replace(/\\/g, "/")).toContain("/src/runtime/components/prose/callout/Caution.vue");
  });

  it("resolves explicit generated prose component names", () => {
    const file = resolveComponentFile("ProseCaution", { uiRoot });

    expect(file.replace(/\\/g, "/")).toContain("/src/runtime/components/prose/callout/Caution.vue");
  });

  it("returns a resolved component path that the trace helper can read directly", () => {
    const file = resolveComponentFile("CheckboxGroup", { uiRoot });

    expect(readComponentSourceForTrace(file)).toContain("export interface CheckboxGroupProps");
    expect(() => readFileSync(file, "utf-8")).not.toThrow();
  });
});
