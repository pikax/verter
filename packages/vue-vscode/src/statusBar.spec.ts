import { describe, it, expect } from "vitest";
import { computeStatusBarState } from "./statusBar";

describe("computeStatusBarState", () => {
  it("shows tsserver with check icon", () => {
    const state = computeStatusBarState({ kind: "tsserver" });
    expect(state.text).toContain("tsserver");
    expect(state.text).toContain("$(check)");
    expect(state.warning).toBe(false);
    expect(state.tooltip).toContain("tsserver");
  });

  it("shows tsgo with check icon", () => {
    const state = computeStatusBarState({ kind: "tsgo" });
    expect(state.text).toContain("tsgo");
    expect(state.text).toContain("$(check)");
    expect(state.warning).toBe(false);
    expect(state.tooltip).toContain("tsgo");
  });

  it("shows warning for none with reason", () => {
    const state = computeStatusBarState({
      kind: "none",
      reason: "Node.js not found",
    });
    expect(state.text).toContain("$(warning)");
    expect(state.text).toContain("No TS");
    expect(state.warning).toBe(true);
    expect(state.tooltip).toContain("Node.js not found");
    // Negative: should NOT contain check icon
    expect(state.text).not.toContain("$(check)");
  });

  it("shows generic warning for none without reason", () => {
    const state = computeStatusBarState({ kind: "none" });
    expect(state.text).toContain("$(warning)");
    expect(state.warning).toBe(true);
    expect(state.tooltip).toContain("No TypeScript type provider");
    // Negative: tooltip should not contain "undefined"
    expect(state.tooltip).not.toContain("undefined");
  });
});
