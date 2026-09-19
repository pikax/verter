/**
 * WSP1L.1/WSP1L.2 stamp channel — the machine-readable instrumentation lines the
 * volt (and an instrumented Lapce client build) write to the driven client's
 * host stderr:
 *
 * - `verter launch-stamp {json}` — the volt's one-time initialize epoch marker
 *   (default off behind `uiTrace.enabled`; never a UI notification).
 * - `verter ui-stamp {json}` — one input-dispatch/decode/apply/paint
 *   observation emitted by an instrumented client event loop.
 *
 * These parsers are the dx-harness side of that contract. They validate every
 * field and throw loud on a malformed stamp line; a missing or unparsable
 * observation stays missing — timestamps, epochs and stages are never guessed.
 *
 * Real-client rendering (verified against Lapce 0.4.6): the client does not
 * forward plugin stderr verbatim. `lapce-proxy` routes every plugin stderr
 * write through its `tracing` log (target
 * `lapce_proxy::plugin::wasi::<author>::<name>`, DEBUG level), so a stamp
 * reaches a consumable channel only as the TRAILING message field of a
 * formatted host log line — on the daily `data/logs/lapce.*.log` (always on)
 * or on the client process's own stderr when it runs with
 * `LAPCE_LOG=lapce_proxy=debug` (that rendering interleaves ANSI escapes,
 * thread ids and source line numbers before the message). The message body is
 * verbatim, so `collectStampMessages`/`isStampMessage` accept a stamp both as a
 * whole line and embedded as a host log line's trailing field; extraction never
 * repairs a malformed body — that still fails loud.
 */

import {
  SCRIPTED_STEP_KINDS,
  UI_STAGE_ORDER,
  type ScriptedStepKind,
  type UiStage,
} from "./types.js";

export const LAUNCH_STAMP_MESSAGE_PREFIX = "verter launch-stamp " as const;
export const UI_STAMP_MESSAGE_PREFIX = "verter ui-stamp " as const;

export type LaunchStampPhase = "server_launch_issued" | "launch_refused";

/** Parsed `verter launch-stamp` payload (the volt's `LaunchStamp::to_json`). */
export interface LaunchStampPayload {
  readonly phase: LaunchStampPhase;
  readonly atUnixMs: number;
  readonly serverUri: string;
  readonly workspaceRoot: string;
  readonly documentLanguages: readonly string[];
  /** Present only on `launch_refused`; why the launch did not happen. */
  readonly refusal?: string;
}

/**
 * Parsed `verter ui-stamp` payload: one UI stage observation from an
 * instrumented client event loop, on the same request/source epoch basis as a
 * WSP1 `InteractionTrace`.
 */
export interface UiStampPayload {
  readonly kind: ScriptedStepKind;
  readonly requestEpoch: number;
  readonly sourceEpoch: number | null;
  readonly stage: UiStage;
  readonly atUnixMs: number;
}

/** The captured stderr lines of a stamp kind, with unparsable lines failing loud. */
export interface StampMessages {
  readonly launchStamps: readonly LaunchStampPayload[];
  readonly uiStamps: readonly UiStampPayload[];
}

function stampJsonAfter(raw: string, prefix: string): Record<string, unknown> {
  if (!raw.startsWith(prefix)) {
    throw new Error(`not a '${prefix.trim()}' line: ${raw.slice(0, 80)}`);
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw.slice(prefix.length));
  } catch (error) {
    throw new Error(`malformed ${prefix.trim()} JSON: ${(error as Error).message}`);
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    throw new Error(`${prefix.trim()} payload must be a JSON object`);
  }
  return parsed as Record<string, unknown>;
}

function requireNonNegativeIntegerMs(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${field} must be a non-negative integer of Unix milliseconds`);
  }
  return value;
}

function requireEpoch(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 1) {
    throw new Error(`${field} must be a positive integer request/source epoch`);
  }
  return value;
}

function requireString(value: unknown, field: string): string {
  if (typeof value !== "string") {
    throw new Error(`${field} must be a string`);
  }
  return value;
}

/** Parse one structured launch-stamp value (already stripped of the prefix). */
export function parseLaunchStampValue(value: unknown): LaunchStampPayload {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("launch-stamp payload must be an object");
  }
  const record = value as Record<string, unknown>;
  const phase = requireString(record.phase, "launch-stamp phase");
  if (phase !== "server_launch_issued" && phase !== "launch_refused") {
    throw new Error(`launch-stamp phase must be 'server_launch_issued' or 'launch_refused'`);
  }
  const languages = record.documentLanguages;
  if (!Array.isArray(languages) || languages.some((lang) => typeof lang !== "string")) {
    throw new Error("launch-stamp documentLanguages must be an array of strings");
  }
  const refusal = record.refusal;
  if (phase === "launch_refused") {
    if (typeof refusal !== "string" || refusal.length === 0) {
      throw new Error("a refused launch stamp must carry a non-empty refusal reason");
    }
  } else if (refusal !== undefined) {
    throw new Error("a server_launch_issued stamp must not carry a refusal");
  }
  const payload: LaunchStampPayload = {
    phase,
    atUnixMs: requireNonNegativeIntegerMs(record.atUnixMs, "launch-stamp atUnixMs"),
    serverUri: requireString(record.serverUri, "launch-stamp serverUri"),
    workspaceRoot: requireString(record.workspaceRoot, "launch-stamp workspaceRoot"),
    documentLanguages: languages as readonly string[],
  };
  if (refusal !== undefined) {
    return { ...payload, refusal: requireString(refusal, "launch-stamp refusal") };
  }
  return payload;
}

/** Parse one captured `verter launch-stamp {json}` stderr line. */
export function parseLaunchStampMessage(raw: string): LaunchStampPayload {
  return parseLaunchStampValue(stampJsonAfter(raw, LAUNCH_STAMP_MESSAGE_PREFIX));
}

/** Parse one structured UI-stamp value (already stripped of the prefix). */
export function parseUiStampValue(value: unknown): UiStampPayload {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("ui-stamp payload must be an object");
  }
  const record = value as Record<string, unknown>;
  const kind = requireString(record.kind, "ui-stamp kind");
  if (!(SCRIPTED_STEP_KINDS as readonly string[]).includes(kind)) {
    throw new Error(`ui-stamp kind must be one of ${SCRIPTED_STEP_KINDS.join(", ")}`);
  }
  const stage = requireString(record.stage, "ui-stamp stage");
  if (!(UI_STAGE_ORDER as readonly string[]).includes(stage)) {
    throw new Error(`ui-stamp stage must be one of ${UI_STAGE_ORDER.join(", ")}`);
  }
  const sourceEpoch =
    record.sourceEpoch === null ? null : requireEpoch(record.sourceEpoch, "ui-stamp sourceEpoch");
  return {
    kind: kind as ScriptedStepKind,
    requestEpoch: requireEpoch(record.requestEpoch, "ui-stamp requestEpoch"),
    sourceEpoch,
    stage: stage as UiStage,
    atUnixMs: requireNonNegativeIntegerMs(record.atUnixMs, "ui-stamp atUnixMs"),
  };
}

/** Parse one captured `verter ui-stamp {json}` stderr line. */
export function parseUiStampMessage(raw: string): UiStampPayload {
  return parseUiStampValue(stampJsonAfter(raw, UI_STAMP_MESSAGE_PREFIX));
}

export function isStampMessage(raw: string): boolean {
  return stampMessageInLine(raw) !== null;
}

/**
 * The stamp message carried by one captured line: either the whole line (a raw
 * stamp channel) or the trailing message field of a host log line (the real
 * client's tracing rendering). Returns `null` for lines carrying no stamp
 * marker; a marker whose body does not parse still fails loud in the parsers.
 */
function stampMessageInLine(raw: string): string | null {
  if (raw.startsWith(LAUNCH_STAMP_MESSAGE_PREFIX) || raw.startsWith(UI_STAMP_MESSAGE_PREFIX)) {
    return raw;
  }
  for (const prefix of [LAUNCH_STAMP_MESSAGE_PREFIX, UI_STAMP_MESSAGE_PREFIX]) {
    // The space delimiter keeps `xverter launch-stamp`-style prose from matching;
    // the LAST occurrence is the message body (any earlier one would be inside a
    // host log prefix, never inside the stamp's own controlled-vocabulary JSON).
    const at = raw.lastIndexOf(` ${prefix.trim()}`);
    if (at !== -1) {
      return raw.slice(at + 1);
    }
  }
  return null;
}

/**
 * Scan the driven client's captured stderr/log lines: lines without a stamp
 * marker are ordinary host noise and are skipped; a verter stamp — raw or
 * embedded in a host log line — that does not parse throws loud instead of
 * being guessed into the capture.
 */
export function collectStampMessages(lines: readonly string[]): StampMessages {
  const launchStamps: LaunchStampPayload[] = [];
  const uiStamps: UiStampPayload[] = [];
  for (const raw of lines) {
    const message = stampMessageInLine(raw);
    if (message === null) continue;
    if (message.startsWith(LAUNCH_STAMP_MESSAGE_PREFIX)) {
      launchStamps.push(parseLaunchStampMessage(message));
    } else {
      uiStamps.push(parseUiStampMessage(message));
    }
  }
  return { launchStamps, uiStamps };
}

/**
 * WSP1L.2 clock-domain join. The stamps carry Unix milliseconds while the WSP1
 * server `InteractionTrace` and the UI timeline carry the run's own clock, so
 * the two domains are joined ONLY through an anchor the capture session
 * recorded by observing both clocks at one instant — never by assuming they
 * share an origin.
 */
export interface StampClockAnchor {
  /** The capture session's Unix-ms reading at the anchor instant. */
  readonly stampUnixMs: number;
  /** The same instant on the correlated timeline's clock. */
  readonly timelineMs: number;
  /** Where the anchor was recorded (capture artifact reference). */
  readonly recordedAs: string;
}

/** Map a stamp's Unix-ms reading onto the correlated timeline's clock. */
export function stampUnixMsToTimelineMs(atUnixMs: number, anchor: StampClockAnchor): number {
  if (
    !Number.isSafeInteger(anchor.stampUnixMs) ||
    anchor.stampUnixMs < 0 ||
    !Number.isFinite(anchor.timelineMs)
  ) {
    throw new Error("the clock anchor must carry a recorded stampUnixMs and timelineMs");
  }
  return anchor.timelineMs + (atUnixMs - anchor.stampUnixMs);
}
