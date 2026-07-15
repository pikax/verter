/**
 * Log collector (raw-LSP).
 *
 * Reads the child server's buffered stderr and scans for WARN/ERROR. A generic
 * WARN/ERROR is surfaced directly (an ERROR is user-visible, a WARN a candidate). A
 * POSITION-MAPPING string is treated differently: it is a ROOT-CAUSE HINT only when it
 * correlates with a semantic failure another collector observed for the SAME
 * `(uri, position, method)`. Mapping failures are routinely benign at transitional
 * positions (verter falls back through strict mapping, then a member-boundary helper),
 * so an uncorrelated mapping string is NOT a failure — the collector never fails on
 * debug mapping text alone.
 */

import { type CollectorLspClient } from "./client.js";
import {
  collectorEvent,
  type CollectorEvent,
  type CollectorEventKey,
  type EventSink,
  type SignalProvenance,
} from "./event.js";

const RAW_LSP: SignalProvenance = { detectedBy: "rawLsp" };

/** EOL matcher mirroring the stderr line splitter (`@verter/lsp-test-client`). */
const STDERR_EOL = /\r\n|\n|\r/;

// The tracing level token is uppercase; requiring the bounded token avoids matching the
// lowercase word inside a message body.
const ERROR_LEVEL = /(?:^|[^A-Za-z])ERROR(?:[^A-Za-z]|$)/;
const WARN_LEVEL = /(?:^|[^A-Za-z])WARN(?:ING)?(?:[^A-Za-z]|$)/;

/**
 * The mapping-failure string shape: `<method>: position mapping failed for
 * <uri>:<line><sep><char>`, where `<sep>` is `,` (the completion family) OR `:` (the
 * definition / type_definition / references family). Both separators must parse — the
 * comma-only form silently dropped every definition-family mapping failure.
 */
const MAPPING_FAILURE = /(\w+):\s*position mapping failed for (.+):(\d+)[,:](\d+)\s*$/;

/** The flagged log levels. */
export type LogLevel = "error" | "warn";

/** A scanned WARN/ERROR stderr line. */
export interface LogObservation {
  readonly level: LogLevel;
  readonly line: string;
}

/** A parsed position-mapping failure, with its method/uri/position and the raw line. */
export interface MappingFailure {
  readonly method: string;
  readonly uri: string;
  readonly line: number;
  readonly character: number;
  readonly raw: string;
}

/** A semantic failure another collector observed, for mapping-hint correlation. */
export interface SemanticFailureKey {
  readonly method: string;
  readonly uri: string;
  readonly line: number;
  readonly character: number;
}

/** The level of a log line (ERROR dominates WARN), or `null` for INFO/DEBUG/other. */
export function logLevel(line: string): LogLevel | null {
  if (ERROR_LEVEL.test(line)) return "error";
  if (WARN_LEVEL.test(line)) return "warn";
  return null;
}

/** Parse a position-mapping failure line, or `null` if the line is not one. */
export function parseMappingFailure(line: string): MappingFailure | null {
  const match = MAPPING_FAILURE.exec(line.trim());
  if (match === null) return null;
  return {
    method: match[1],
    uri: match[2],
    line: Number.parseInt(match[3], 10),
    character: Number.parseInt(match[4], 10),
    raw: line.trim(),
  };
}

/** Scan stderr lines, returning the WARN/ERROR observations (INFO/DEBUG ignored). */
export function scanLogLines(lines: readonly string[]): LogObservation[] {
  const out: LogObservation[] = [];
  for (const line of lines) {
    const level = logLevel(line);
    if (level !== null) out.push({ level, line });
  }
  return out;
}

/** Split a buffered stderr blob into lines (the same EOL rule as the stderr buffer). */
export function splitStderr(text: string): string[] {
  return text.split(STDERR_EOL);
}

/** Whether a mapping failure correlates with a semantic failure at the same method/uri/position. */
function correlates(mf: MappingFailure, semanticFailures: readonly SemanticFailureKey[]): boolean {
  return semanticFailures.some(
    (failure) =>
      failure.method === mf.method &&
      failure.uri === mf.uri &&
      failure.line === mf.line &&
      failure.character === mf.character,
  );
}

/** The pure inputs to one log classification. */
export interface LogsInput {
  readonly key: CollectorEventKey;
  /** The stderr lines to scan. */
  readonly lines: readonly string[];
  /** Semantic failures observed by other collectors this window, for mapping-hint correlation. */
  readonly semanticFailures: readonly SemanticFailureKey[];
}

/**
 * Classify stderr lines. A mapping-failure line becomes a `mapping_root_cause_hint`
 * (`candidate`) when it correlates with a semantic failure, otherwise a benign
 * (`ok`) `mapping_failure_benign` — never a failure on its own. A non-mapping ERROR is
 * a user-visible `server_error`; a non-mapping WARN is a candidate `server_warn`.
 */
export function classifyLogs(input: LogsInput): CollectorEvent[] {
  const { key, lines, semanticFailures } = input;
  const events: CollectorEvent[] = [];

  for (const line of lines) {
    const mf = parseMappingFailure(line);
    if (mf !== null) {
      // A mapping string is a hint only when it correlates with a real semantic failure.
      if (correlates(mf, semanticFailures)) {
        events.push(
          collectorEvent({
            collector: "logs",
            signal: "mapping_root_cause_hint",
            ok: false,
            severity: "candidate",
            provenance: {
              detectedBy: "rawLsp",
              note: "correlates with a semantic failure at the same (uri, position, method)",
            },
            key,
            detail: `position mapping failed at ${mf.uri}:${mf.line},${mf.character} for ${mf.method} — correlates with a semantic failure`,
            data: mf,
          }),
        );
      } else {
        events.push(
          collectorEvent({
            collector: "logs",
            signal: "mapping_failure_benign",
            ok: true,
            severity: "candidate",
            provenance: {
              detectedBy: "rawLsp",
              note: "uncorrelated mapping failure — benign at transitional positions, not a failure",
            },
            key,
            detail: `position mapping failed at ${mf.uri}:${mf.line},${mf.character} for ${mf.method} — no correlated semantic failure`,
            data: mf,
          }),
        );
      }
      continue;
    }

    const level = logLevel(line);
    if (level === "error") {
      events.push(
        collectorEvent({
          collector: "logs",
          signal: "server_error",
          ok: false,
          severity: "userVisible",
          provenance: RAW_LSP,
          key,
          detail: `server logged an error: ${line.trim()}`,
          data: { line: line.trim() },
        }),
      );
    } else if (level === "warn") {
      events.push(
        collectorEvent({
          collector: "logs",
          signal: "server_warn",
          ok: false,
          severity: "candidate",
          provenance: RAW_LSP,
          key,
          detail: `server logged a warning: ${line.trim()}`,
          data: { line: line.trim() },
        }),
      );
    }
  }

  return events;
}

// ── live raw-LSP driver ──────────────────────────────────────────────────────

/** Options for the live {@link collectLogs} run. */
export interface CollectLogsOptions {
  readonly client: CollectorLspClient;
  readonly sink: EventSink;
  readonly scenario: string;
  readonly probe: string;
  readonly anchor: string;
  readonly provider: string;
  readonly version: number;
  readonly editStepIndex: number;
  /** Semantic failures observed by other collectors this window, for correlation. */
  readonly semanticFailures: readonly SemanticFailureKey[];
}

/**
 * Read the child server's buffered stderr and classify it against the semantic
 * failures observed this window. The collector drives the raw-LSP stderr surface; it
 * never fails on uncorrelated mapping text.
 */
export function collectLogs(options: CollectLogsOptions): void {
  const lines = splitStderr(options.client.stderr.text());
  const key: CollectorEventKey = {
    scenario: options.scenario,
    editStepIndex: options.editStepIndex,
    driver: "rawLsp",
    provider: options.provider,
    probe: options.probe,
    version: options.version,
    anchor: options.anchor,
  };
  for (const event of classifyLogs({ key, lines, semanticFailures: options.semanticFailures })) {
    options.sink.emit(event);
  }
}
