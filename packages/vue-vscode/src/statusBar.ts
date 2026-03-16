import type { NotificationParams, NotificationType } from "@verter/language-shared";

export interface StatusBarState {
  text: string;
  tooltip: string;
  warning: boolean;
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
