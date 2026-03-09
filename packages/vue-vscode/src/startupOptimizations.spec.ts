/**
 * @ai-generated - Tests startup optimization helpers used by benchmark timing and lazy startup.
 */
import { describe, expect, it } from "vitest";
import {
  computeStartupSegments,
  shouldConfigureBuiltInTypeScriptPlugin,
} from "./startupOptimizations";

describe("computeStartupSegments", () => {
  it("computes direct provider startup segments", () => {
    const result = computeStartupSegments({
      activationStartMs: 100,
      typeProviderStartedMs: 350,
      lspReadyMs: 900,
      firstTypedCompletionMs: 700,
      firstDiagnosticMs: 950,
    });

    expect(result.activationToReadyMs).toBe(800);
    expect(result.activationToFirstTypedCompletionMs).toBe(600);
    expect(result.readyToFirstTypedCompletionMs).toBe(-200);
    expect(result.activationToTypeProviderStartedMs).toBe(250);
    expect(result.typeProviderStartedToFirstTypedCompletionMs).toBe(350);
    expect(result.typeProviderStartedToReadyMs).toBe(550);
  });

  it("keeps derived segments undefined when source markers are missing", () => {
    const result = computeStartupSegments({
      activationStartMs: 100,
      lspReadyMs: 400,
      firstTypedCompletionMs: 250,
    });

    expect(result.activationToReadyMs).toBe(300);
    expect(result.activationToFirstTypedCompletionMs).toBe(150);
    expect(result.readyToFirstTypedCompletionMs).toBe(-150);
    expect(result.activationToTypeProviderStartedMs).toBeUndefined();
    expect(result.typeProviderStartedToFirstTypedCompletionMs).toBeUndefined();
    expect(result.typeProviderStartedToReadyMs).toBeUndefined();
  });
});

describe("shouldConfigureBuiltInTypeScriptPlugin", () => {
  it("configures only for TS/JS editor demand", () => {
    expect(shouldConfigureBuiltInTypeScriptPlugin("typescript")).toBe(true);
    expect(shouldConfigureBuiltInTypeScriptPlugin("typescriptreact")).toBe(true);
    expect(shouldConfigureBuiltInTypeScriptPlugin("javascript")).toBe(true);
    expect(shouldConfigureBuiltInTypeScriptPlugin("javascriptreact")).toBe(true);
  });

  it("does not treat Vue as a built-in TS plugin trigger", () => {
    expect(shouldConfigureBuiltInTypeScriptPlugin("vue")).toBe(false);
    expect(shouldConfigureBuiltInTypeScriptPlugin(undefined)).toBe(false);
    expect(shouldConfigureBuiltInTypeScriptPlugin("json")).toBe(false);
  });
});
