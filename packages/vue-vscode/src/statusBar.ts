import type { NotificationParams, NotificationType } from "@verter/language-shared";

export interface StatusBarState {
  text: string;
  tooltip: string;
  warning: boolean;
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

/**
 * Compute the status bar display state from TypeProviderStatus params.
 * Pure function — easy to unit test without VS Code API dependency.
 */
export function computeStatusBarState(
  params: NotificationParams[typeof NotificationType.TypeProviderStatus],
): StatusBarState {
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
