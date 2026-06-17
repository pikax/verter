/**
 * @ai-generated - Shared helpers for startup timing segmentation.
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
