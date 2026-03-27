/**
 * @ai-generated - Shared helpers for startup timing segmentation and lazy built-in TS plugin activation.
 */

export interface StartupTimingLike {
  activationStartMs?: number | null;
  typeProviderStartedMs?: number | null;
  lspReadyMs?: number | null;
  firstTypedCompletionMs?: number | null;
  firstDiagnosticMs?: number | null;
}

export interface StartupSegments {
  activationToReadyMs?: number;
  activationToFirstTypedCompletionMs?: number;
  readyToFirstTypedCompletionMs?: number;
  activationToTypeProviderStartedMs?: number;
  typeProviderStartedToFirstTypedCompletionMs?: number;
  typeProviderStartedToReadyMs?: number;
}

const BUILT_IN_TS_PLUGIN_LANGUAGE_IDS = new Set([
  "typescript",
  "typescriptreact",
  "javascript",
  "javascriptreact",
]);

export function computeStartupSegments(timing: StartupTimingLike): StartupSegments {
  return {
    activationToReadyMs: diff(timing.activationStartMs, timing.lspReadyMs),
    activationToFirstTypedCompletionMs: diff(
      timing.activationStartMs,
      timing.firstTypedCompletionMs,
    ),
    readyToFirstTypedCompletionMs: diff(timing.lspReadyMs, timing.firstTypedCompletionMs),
    activationToTypeProviderStartedMs: diff(timing.activationStartMs, timing.typeProviderStartedMs),
    typeProviderStartedToFirstTypedCompletionMs: diff(
      timing.typeProviderStartedMs,
      timing.firstTypedCompletionMs,
    ),
    typeProviderStartedToReadyMs: diff(timing.typeProviderStartedMs, timing.lspReadyMs),
  };
}

export function shouldConfigureBuiltInTypeScriptPlugin(languageId?: string): boolean {
  return languageId !== undefined && BUILT_IN_TS_PLUGIN_LANGUAGE_IDS.has(languageId);
}

function diff(
  startMs: number | null | undefined,
  endMs: number | null | undefined,
): number | undefined {
  if (!isFiniteNumber(startMs) || !isFiniteNumber(endMs)) {
    return undefined;
  }
  return endMs - startMs;
}

function isFiniteNumber(value: number | null | undefined): value is number {
  return typeof value === "number" && Number.isFinite(value);
}
