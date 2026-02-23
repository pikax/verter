/**
 * @ai-generated - Tests for @verter/native exports.
 * Verifies that VerterHost and processStyle work correctly with both string and Buffer inputs.
 */
import { describe, it, expect } from "vitest";
import { VerterHost, processStyle } from "./index.js";

const SFC_INPUT =
  '<script setup>\nconst msg = "hello"\n</script>\n<template><div>{{ msg }}</div></template>';

describe("VerterHost", () => {
  it("should compile a simple SFC via upsert + getVirtualFile (string source)", () => {
    const host = new VerterHost();
    const result = host.upsert({
      inputId: "Test.vue",
      source: SFC_INPUT,
    });

    expect(result.canonicalId).toBeTruthy();
    expect(result.changed).toBe(true);

    const mainFile = host.getVirtualFile({
      canonicalId: result.canonicalId,
      nodeKind: { kind: "main" },
    });

    expect(mainFile.code).toBeTruthy();
    expect(mainFile.code).toContain("_sfc_main");
  });

  it("should accept Buffer as source in upsert", () => {
    const host = new VerterHost();
    const result = host.upsert({
      inputId: "BufferTest.vue",
      source: Buffer.from(SFC_INPUT, "utf-8"),
    });

    expect(result.canonicalId).toBeTruthy();
    expect(result.changed).toBe(true);

    const mainFile = host.getVirtualFile({
      canonicalId: result.canonicalId,
      nodeKind: { kind: "main" },
    });
    expect(mainFile.code).toContain("_sfc_main");
  });

  it("should strip TypeScript when forceJs is set in compile profile", () => {
    const host = new VerterHost();
    host.upsert({
      inputId: "TypedComponent.vue",
      source: '<script setup lang="ts">\nconst x: number = 1;\n</script>\n<template><div>{{ x }}</div></template>',
    });

    const mainFile = host.getVirtualFile({
      canonicalId: "TypedComponent.vue",
      nodeKind: { kind: "main" },
      compileProfile: { forceJs: true },
    });

    expect(mainFile.code).toContain("const x");
    expect(mainFile.code).not.toContain(": number");
  });
});

describe("processStyle", () => {
  it("should scope CSS selectors (string input)", () => {
    const result = processStyle(".foo { color: red }", {
      scopeId: "abc123",
      scoped: true,
    });

    expect(result.code).toContain("abc123");
  });

  it("should scope CSS selectors (Buffer input)", () => {
    const result = processStyle(Buffer.from(".foo { color: red }"), {
      scopeId: "abc123",
      scoped: true,
    });

    expect(result.code).toContain("abc123");
  });
});
