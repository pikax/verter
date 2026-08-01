import type { NotificationParams, NotificationType } from "@verter/language-shared";

import {
  CARRIER_UNSUPPORTED_TOOLTIP,
  providerServesFrameworkCarriers,
} from "./carrierProviderSupport";

export interface StatusBarState {
  text: string;
  tooltip: string;
  warning: boolean;
}

/**
 * The CLIENT-side facts the server's status notification does not carry: which
 * provider the user selected, and whether a framework carrier is currently open.
 * Together they decide whether the selected provider can serve what is on screen.
 */
export interface StatusBarClientContext {
  /** The active `verter.typeProvider` value. */
  readonly typeProvider: string;
  /** Whether at least one `.vue`/`.svelte` document is open. */
  readonly carrierOpen: boolean;
}

export interface ProviderRecommendationNoticeOptions {
  /** The `verter.providerRecommendations` setting. */
  enabled: boolean;
  /** Whether the user already dismissed the notice for this workspace. */
  dismissed: boolean;
}

export interface ProviderRecommendationNotice {
  message: string;
}

/**
 * Render the server-sent provider recommendation (tsgo-preferred flip) into a
 * notification message, honoring the user setting and prior dismissal.
 * Pure function — the server owns the DECISION (structured facts on
 * `$/verter/typeProviderStatus`); this client owns PRESENTATION.
 * Returns `undefined` when nothing should be shown (never nags).
 */
export function computeProviderRecommendationNotice(
  params: NotificationParams[typeof NotificationType.TypeProviderStatus],
  options: ProviderRecommendationNoticeOptions,
): ProviderRecommendationNotice | undefined {
  const recommendation = params.recommendation;
  if (!recommendation || !options.enabled || options.dismissed) return undefined;
  const gaps =
    recommendation.knownGaps.length > 0 ? ` Note: ${recommendation.knownGaps.join(" ")}` : "";
  return { message: `Verter: ${recommendation.reason}${gaps}` };
}

/** Append the server's provenance so the tooltip says WHY this engine was chosen. */
function withReason(base: string, reason: string | undefined): string {
  return reason ? `${base} — ${reason}` : base;
}

/**
 * Compute the status bar display state from TypeProviderStatus params.
 * Pure function — easy to unit test without VS Code API dependency.
 */
export function computeStatusBarState(
  params: NotificationParams[typeof NotificationType.TypeProviderStatus],
  client?: StatusBarClientContext,
): StatusBarState {
  // A provider that cannot serve the carrier on screen outranks every serving
  // state below: an engine answering plain TypeScript while the open `.vue` gets
  // nothing is not a healthy status, and a check mark there is the fail-open the
  // containment exists to remove. Scoped to an OPEN carrier, so a plain-TypeScript
  // session under the same provider still reads as serving — which it is.
  if (client?.carrierOpen && !providerServesFrameworkCarriers(client.typeProvider)) {
    return {
      text: "$(warning) Verter: no Vue/Svelte TS",
      tooltip: CARRIER_UNSUPPORTED_TOOLTIP,
      warning: true,
    };
  }

  // The TOPOLOGY is the honest answer — WHICH engine is serving and who owns
  // it. The engine FAMILY (`kind`) is what behaviour keys on, and two
  // topologies share the "tsgo" family: an attach to the tsgo the editor is
  // already running, and a second engine Verter spawned. Labelling both
  // "Verter: tsgo" made a serving tier look identical to a broken one. A server
  // that sends no topology falls back to the family.
  switch (params.topology ?? "") {
    case "shared-tsgo":
      return {
        text: "$(check) Verter: tsgo (shared)",
        tooltip: withReason(
          "Verter type provider: the tsgo your editor is already running (no second engine)",
          params.reason,
        ),
        warning: false,
      };
    case "managed-tsgo":
      return {
        text: "$(check) Verter: tsgo (managed)",
        tooltip: withReason(
          "Verter type provider: a tsgo process Verter started and owns",
          params.reason,
        ),
        warning: false,
      };
    case "project-tsserver":
      return {
        text: "$(check) Verter: tsserver",
        tooltip: withReason(
          "Verter type provider: a Node tsserver per configured project, each started from that project's own TypeScript",
          params.reason,
        ),
        warning: false,
      };
    case "extension-hosted":
      return {
        text: "$(check) Verter: in-extension TS",
        tooltip: withReason(
          "Verter type provider: a TypeScript language service hosted in the extension process",
          params.reason,
        ),
        warning: false,
      };
  }

  switch (params.kind) {
    case "tsgo":
      return {
        text: "$(check) Verter: tsgo",
        tooltip: "Verter type provider: tsgo (Go-based TypeScript server)",
        warning: false,
      };
    case "tsserver":
      return {
        text: "$(check) Verter: tsserver",
        tooltip: "Verter type provider: tsserver (Node.js-based TypeScript server)",
        warning: false,
      };
    case "editor-tsserver":
      return {
        text: "$(check) Verter: Editor TS",
        tooltip: params.reason
          ? `Verter type provider: editor-owned tsserver - ${params.reason}`
          : "Verter type provider: editor-owned tsserver plugin",
        warning: false,
      };
    case "none":
    default:
      return {
        text: "$(warning) Verter: No TS",
        tooltip: params.reason
          ? `Verter: No type provider — ${params.reason}`
          : "Verter: No TypeScript type provider available",
        warning: true,
      };
  }
}
