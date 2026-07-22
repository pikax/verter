import { describe, it, expect } from "vitest";
import { computeProviderRecommendationNotice, computeStatusBarState } from "./statusBar";

describe("computeStatusBarState", () => {
  it("shows tsserver with check icon", () => {
    const state = computeStatusBarState({ kind: "tsserver" });
    expect(state.text).toContain("tsserver");
    expect(state.text).toContain("$(check)");
    expect(state.warning).toBe(false);
    expect(state.tooltip).toContain("tsserver");
  });

  it("shows an attested editor-owned tsserver as healthy", () => {
    const state = computeStatusBarState({
      kind: "editor-tsserver",
      reason: "Attested VS Code tsserver process 4242",
    });
    expect(state.text).toContain("Editor TS");
    expect(state.text).toContain("$(check)");
    expect(state.warning).toBe(false);
    expect(state.tooltip).toContain("4242");
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

describe("computeProviderRecommendationNotice", () => {
  const tsserverWithRecommendation = {
    kind: "tsserver" as const,
    recommendation: {
      preferred: "tsgo" as const,
      reason:
        "This workspace is served by a tsserver-family TypeScript service. TSGO is recommended.",
      knownGaps: ["TSGO does not yet provide the 'remove unused declaration' quick fix (TS6133)."],
    },
  };

  it("renders the server recommendation with honest gap details on the tsserver route", () => {
    const notice = computeProviderRecommendationNotice(tsserverWithRecommendation, {
      enabled: true,
      dismissed: false,
    });
    expect(notice).toBeDefined();
    expect(notice?.message).toContain("TSGO");
    expect(notice?.message).toContain("TS6133");
  });

  it("is silent when the server sends no recommendation (tsgo routes)", () => {
    expect(
      computeProviderRecommendationNotice({ kind: "tsgo" }, { enabled: true, dismissed: false }),
    ).toBeUndefined();
    expect(
      computeProviderRecommendationNotice({ kind: "none" }, { enabled: true, dismissed: false }),
    ).toBeUndefined();
  });

  it("respects the user setting", () => {
    expect(
      computeProviderRecommendationNotice(tsserverWithRecommendation, {
        enabled: false,
        dismissed: false,
      }),
    ).toBeUndefined();
  });

  it("respects a prior dismissal (never nags)", () => {
    expect(
      computeProviderRecommendationNotice(tsserverWithRecommendation, {
        enabled: true,
        dismissed: true,
      }),
    ).toBeUndefined();
  });

  it("omits the gap sentence when the server sends no known gaps", () => {
    const notice = computeProviderRecommendationNotice(
      {
        kind: "tsserver",
        recommendation: { preferred: "tsgo", reason: "TSGO is recommended.", knownGaps: [] },
      },
      { enabled: true, dismissed: false },
    );
    expect(notice?.message).toContain("TSGO");
    expect(notice?.message).not.toContain("Note:");
  });
});

describe("computeStatusBarState — the status names the TOPOLOGY, not the family", () => {
  /**
   * Two topologies share the "tsgo" family: an attach to the tsgo the editor is
   * ALREADY running, and a second engine Verter spawned. Reporting both as
   * "Verter: tsgo" made a serving shared attach indistinguishable from a broken
   * one, which is how a working tier was diagnosed as a routing bug.
   */
  it("distinguishes the editor-owned tsgo from a Verter-spawned one", () => {
    const shared = computeStatusBarState({
      kind: "tsgo",
      topology: "shared-tsgo",
      reason: "attested editor-owned Native Preview Program",
    });
    const managed = computeStatusBarState({
      kind: "tsgo",
      topology: "managed-tsgo",
      reason: "managed TSGO resolved to /x/tsgo",
    });

    expect(shared.text).not.toBe(managed.text);
    expect(shared.text).toMatch(/shared|editor/i);
    expect(shared.warning).toBe(false);
    expect(managed.warning).toBe(false);
    expect(shared.tooltip).toContain("Native Preview");
    expect(managed.tooltip).toContain("/x/tsgo");
  });

  it("names the workspace tsserver and the editor plugin distinctly", () => {
    const workspace = computeStatusBarState({
      kind: "tsserver",
      topology: "workspace-tsserver",
    });
    const editor = computeStatusBarState({
      kind: "editor-tsserver",
      topology: "editor-tsserver",
      reason: "attested editor tsserver process 4242",
    });
    expect(workspace.text).not.toBe(editor.text);
    expect(editor.tooltip).toContain("4242");
  });

  it("falls back to the engine family when a server sends no topology", () => {
    const state = computeStatusBarState({ kind: "tsgo" });
    expect(state.text).toContain("tsgo");
    expect(state.warning).toBe(false);
  });
});
