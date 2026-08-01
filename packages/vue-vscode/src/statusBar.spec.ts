import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { describe, it, expect } from "vitest";
import {
  CARRIER_SERVING_REMEDY_PROVIDERS,
  computeCarrierUnsupportedNotice,
  providerServesFrameworkCarriers,
} from "./carrierProviderSupport";
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
      topology: "project-tsserver",
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

// ── `verter.typeProvider: "extension"` must FAIL CLOSED for framework carriers ──
//
// The extension-hosted TypeScript service serves plain `.ts`/`.js` correctly —
// each project from the TypeScript that project installed. It does NOT serve
// `.vue`/`.svelte`: carrier publication is suppressed for the provider kind it
// registers under, so no generated companion ever reaches it and every carrier
// query arrives for a file it has no binding for. The user selected a provider
// advertised "for Vue files" and silently got nothing back.
//
// Until carrier publication reaches this topology, selection is CONTAINED, not
// silent: opening a carrier under `extension` raises a notice naming the gap and
// the providers that do serve carriers, and the status bar holds a persistent
// warning for as long as a carrier is open.
//
// These tests discriminate in BOTH directions. A containment that fired
// unconditionally would be as wrong as none at all: it must not fire for a plain
// `.ts` file under `extension` (that route works and must keep working), and it
// must not fire for a carrier under `auto` / `tsserver` / `tsgo` / `shared-tsgo`
// / `editor-tsserver` (those serve carriers).
//
// They live in THIS file, beside the other "what the user is told about the type
// provider" decisions, rather than in a spec of their own: the two heaviest specs
// in this suite each build real TypeScript language services and run within ~6%
// of vitest's 5s per-test default, so one additional worker file is enough
// parallel load to time one of them out.

/** Every provider the setting offers that DOES serve carriers. */
const CARRIER_SERVING_PROVIDERS = [
  "auto",
  "shared-tsgo",
  "tsgo",
  "tsserver",
  "editor-tsserver",
] as const;

function manifest(): {
  contributes: {
    configuration: {
      properties: Record<string, { enum: string[]; enumDescriptions: string[] }>;
    };
  };
} {
  return JSON.parse(readFileSync(join(dirname(import.meta.dirname), "package.json"), "utf8"));
}

function providerDescription(value: string): string {
  const setting = manifest().contributes.configuration.properties["verter.typeProvider"]!;
  const index = setting.enum.indexOf(value);
  expect(index, `verter.typeProvider must offer ${value}`).toBeGreaterThanOrEqual(0);
  return setting.enumDescriptions[index]!;
}

describe("carrier service under verter.typeProvider", () => {
  it("classifies `extension` as not serving framework carriers", () => {
    expect(providerServesFrameworkCarriers("extension")).toBe(false);
  });

  it("classifies every carrier-serving provider as serving them", () => {
    for (const provider of CARRIER_SERVING_PROVIDERS) {
      expect(providerServesFrameworkCarriers(provider), provider).toBe(true);
    }
  });
});

describe("computeCarrierUnsupportedNotice", () => {
  it("names the gap and the remedy when a Vue carrier opens under `extension`", () => {
    const notice = computeCarrierUnsupportedNotice({
      typeProvider: "extension",
      languageId: "vue",
    });
    expect(notice, "a carrier under `extension` must not be served silently").toBeDefined();
    const message = notice!.message;
    // WHICH provider, WHAT does not work, and WHERE to go instead.
    expect(message).toContain("extension");
    expect(message).toMatch(/\.vue/);
    expect(message).toMatch(/\.svelte/);
    expect(message).toMatch(/diagnostic/i);
    expect(message).toMatch(/hover/i);
    for (const remedy of CARRIER_SERVING_REMEDY_PROVIDERS) {
      expect(message, `the notice must name ${remedy} as a working alternative`).toContain(remedy);
    }
  });

  it("fires for a Svelte carrier too, not just Vue", () => {
    expect(
      computeCarrierUnsupportedNotice({ typeProvider: "extension", languageId: "svelte" }),
    ).toBeDefined();
  });

  // The design judgement. `extension` serves plain TypeScript correctly, from
  // each project's own install — refusing the provider outright would delete a
  // working capability to contain a missing one. Only the carrier half is
  // contained.
  it("stays silent for a plain TypeScript file under `extension`", () => {
    for (const languageId of ["typescript", "javascript", "typescriptreact", undefined]) {
      expect(
        computeCarrierUnsupportedNotice({ typeProvider: "extension", languageId }),
        String(languageId),
      ).toBeUndefined();
    }
  });

  // The half that proves the notice is conditional rather than unconditional.
  it("stays silent for a carrier under every provider that serves carriers", () => {
    for (const typeProvider of CARRIER_SERVING_PROVIDERS) {
      for (const languageId of ["vue", "svelte"]) {
        expect(
          computeCarrierUnsupportedNotice({ typeProvider, languageId }),
          `${typeProvider}/${languageId}`,
        ).toBeUndefined();
      }
    }
  });
});

describe("computeStatusBarState — the persistent carrier-unsupported warning", () => {
  it("holds a warning while a carrier is open under `extension`", () => {
    const state = computeStatusBarState(
      { kind: "tsserver", topology: "extension-hosted" },
      { typeProvider: "extension", carrierOpen: true },
    );
    expect(state.warning, "a carrier that cannot be served must not read as healthy").toBe(true);
    expect(state.text).toContain("$(warning)");
    expect(state.text).not.toContain("$(check)");
    expect(state.tooltip).toMatch(/\.vue/);
    expect(state.tooltip).toMatch(/\.svelte/);
  });

  it("stays healthy under `extension` when no carrier is open", () => {
    const state = computeStatusBarState(
      { kind: "tsserver", topology: "extension-hosted" },
      { typeProvider: "extension", carrierOpen: false },
    );
    expect(state.warning).toBe(false);
    expect(state.text).toContain("$(check)");
  });

  it("stays healthy for an open carrier under a provider that serves carriers", () => {
    for (const [typeProvider, topology] of [
      ["tsserver", "project-tsserver"],
      ["tsgo", "managed-tsgo"],
      ["shared-tsgo", "shared-tsgo"],
    ] as const) {
      const state = computeStatusBarState(
        { kind: "tsserver", topology },
        { typeProvider, carrierOpen: true },
      );
      expect(state.warning, typeProvider).toBe(false);
      expect(state.text, typeProvider).toContain("$(check)");
    }
  });
});

describe("verter.typeProvider — the `extension` option's user-facing copy", () => {
  // "Experimental" communicates instability, not the total absence of Vue and
  // Svelte service. The setting must say what does not work.
  it("states that .vue and .svelte are not served under this provider", () => {
    const description = providerDescription("extension");
    expect(description).toMatch(/\.vue/);
    expect(description).toMatch(/\.svelte/);
    expect(description).toMatch(/not served|does not serve|no .* support/i);
    // And still points somewhere that does work.
    expect(description).toMatch(/auto|tsserver|tsgo/);
  });

  it("does not put that claim on a provider that DOES serve carriers", () => {
    for (const provider of CARRIER_SERVING_PROVIDERS) {
      expect(providerDescription(provider), provider).not.toMatch(/not served|does not serve/i);
    }
  });
});
