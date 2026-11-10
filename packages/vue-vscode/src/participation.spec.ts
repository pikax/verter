import { describe, expect, it } from "vitest";
import { CLIENT_FRAMEWORKS } from "@verter/language-shared";
import {
  buildParticipationInput,
  COMPETING_TOOLS,
  declarationProblem,
  isParticipationRefusal,
  ParticipationCells,
  type ParticipationObservation,
  type ParticipationReceipt,
} from "./participation";

const visualsOn = {
  apiCallDecorations: true,
  bindingColorDecorations: true,
  propConstnessDecorations: true,
};

function observe(active: string[], preset = "auto"): ParticipationObservation {
  return { preset, visuals: visualsOn, enabledExtensionIds: new Set(active) };
}

const vue = CLIENT_FRAMEWORKS.find((framework) => framework.frameworkId === "vue")!;
const svelte = CLIENT_FRAMEWORKS.find((framework) => framework.frameworkId === "svelte")!;

describe("participation input (COX0)", () => {
  it("ships only declarations that name served frameworks and known facets", () => {
    for (const declaration of COMPETING_TOOLS) {
      expect(declarationProblem(declaration), declaration.key).toBeUndefined();
    }
    expect(
      declarationProblem({
        key: "bogus",
        extensionId: "x.y",
        frameworkId: "vue",
        operations: ["hover", "everything"],
      }),
    ).toBe("unknown operations everything");
  });

  it("installation alone withdraws nothing; an enabled tool declares only its own cells", () => {
    expect(buildParticipationInput(observe([])).ownership).toEqual([]);

    const input = buildParticipationInput(observe(["Vue.volar"]));
    expect(new Set(input.ownership.map((cell) => cell.profile.id))).toEqual(
      new Set([vue.participationProfileId]),
    );
    expect(input.ownership.map((cell) => cell.operation)).toContain("diagnostics.typescript");
    expect(input.ownership.map((cell) => cell.operation)).not.toContain("decoration.apiCalls");
  });

  it("sends no editor identity: evidence is an opaque digest, ordered by the table", () => {
    const forward = buildParticipationInput(observe(["Vue.volar", "svelte.svelte-vscode"]));
    const reversed = buildParticipationInput(observe(["svelte.svelte-vscode", "Vue.volar"]));
    expect(reversed).toEqual(forward);
    const wire = JSON.stringify(forward);
    for (const declaration of COMPETING_TOOLS) {
      expect(wire).not.toContain(declaration.extensionId);
    }
    for (const cell of forward.ownership) {
      expect(cell.evidence).toMatch(/^[0-9a-f]{64}$/);
    }
    expect(new Set(forward.ownership.map((cell) => cell.profile.id))).toEqual(
      new Set([vue.participationProfileId, svelte.participationProfileId]),
    );
  });

  it("passes the authored preset through for the server to validate", () => {
    expect(buildParticipationInput(observe([], "full")).presets).toEqual([
      { target: { kind: "all" }, selector: { kind: "all" }, preset: "full" },
    ]);
    expect(buildParticipationInput(observe([], "off")).presets[0].preset).toBe("off");
  });

  it("reports turned-off visual layers as disabled presentations", () => {
    const input = buildParticipationInput({
      preset: "auto",
      visuals: { ...visualsOn, apiCallDecorations: false, propConstnessDecorations: false },
      enabledExtensionIds: new Set(),
    });
    expect(input.disabledPresentations).toEqual([
      { kind: "operation", operation: "decoration.apiCalls" },
      { kind: "operation", operation: "decoration.propConstness" },
    ]);
  });
});

describe("participation cells (COX0)", () => {
  const receipt: ParticipationReceipt = {
    mask: "0".repeat(64),
    fence: 3,
    changed: true,
    invalidEvidence: [],
    cells: [
      {
        profile: { kind: "framework", id: vue.participationProfileId },
        operation: "hover",
        regionRole: null,
        mode: "full",
        admitted: true,
      },
      {
        profile: { kind: "framework", id: vue.participationProfileId },
        operation: "hover",
        regionRole: "style",
        mode: "workspaceOnly",
        admitted: false,
      },
      {
        profile: { kind: "framework", id: vue.participationProfileId },
        operation: "workspaceSymbol",
        regionRole: null,
        mode: "workspaceOnly",
        admitted: false,
      },
    ],
  };

  it("gates a framework document on its own cell, region by region", () => {
    const cells = new ParticipationCells(receipt);
    expect(cells.admits("vue", "hover")).toBe(true);
    expect(cells.admits("vue", "hover", "style")).toBe(false);
    // Another framework's cell is unaffected, and an absent cell is the default.
    expect(cells.admits("svelte", "hover", "style")).toBe(true);
    expect(cells.admits("vue", "completion")).toBe(true);
    // Documents no framework carrier owns are not gated by the host.
    expect(cells.admits("typescript", "hover")).toBe(true);
    // A workspace demand is admitted while any served framework admits it.
    expect(cells.anyAdmits("workspaceSymbol")).toBe(true);
  });

  it("admits everything before the first receipt", () => {
    const cells = new ParticipationCells();
    expect(cells.admits("vue", "hover", "style")).toBe(true);
  });

  it("recognises the server's typed refusal", () => {
    expect(isParticipationRefusal({ data: { verterParticipation: { primary: "disabled" } } })).toBe(
      true,
    );
    expect(isParticipationRefusal({ data: {} })).toBe(false);
    expect(isParticipationRefusal(new Error("boom"))).toBe(false);
  });
});
