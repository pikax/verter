/**
 * The report layer's JSONL event stream: a pure serializer/deserializer over the
 * landed {@link CollectorEvent} type plus a file-backed {@link EventSink}.
 *
 * This is the persistence half of the collector substrate. A collection run folds
 * its observations into {@link CollectorEvent}s; this module writes them to
 * `artifacts/dx-events.jsonl` — one event per line, stable field order, no embedded
 * record-breaking newline — and reads them back losslessly. It introduces NO new
 * event shape: serialization is delegated to the substrate's own
 * {@link serializeCollectorEvent}, so the on-disk form and the in-memory form never
 * drift.
 */

import { readFileSync, writeFileSync } from "node:fs";

import {
  isCollectorName,
  isCollectorSignal,
  isSeverity,
  serializeCollectorEvent,
  type CollectorEvent,
  type CollectorEventKey,
  type EventSink,
} from "../collectors/index.js";

/** The canonical on-disk name of the event stream. */
export const DX_EVENTS_FILENAME = "dx-events.jsonl";

/** Options governing {@link serializeEvents}. */
export interface SerializeEventsOptions {
  /** Append a trailing newline (the default for a file; off for an exact in-memory blob). */
  readonly trailingNewline?: boolean;
}

/**
 * Serialize events to newline-delimited JSON via the substrate's
 * {@link serializeCollectorEvent} (one record per line, stable field order). A
 * trailing newline is emitted by default so the file is POSIX-clean and append-safe.
 */
export function serializeEvents(
  events: readonly CollectorEvent[],
  options: SerializeEventsOptions = {},
): string {
  const body = events.map(serializeCollectorEvent).join("\n");
  const trailing = options.trailingNewline ?? true;
  if (!trailing || body.length === 0) return body;
  return `${body}\n`;
}

/** Raised when a JSONL line is not a well-formed {@link CollectorEvent}. */
export class ReportEventsError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ReportEventsError";
  }
}

/** Whether a parsed value carries the seven required {@link CollectorEventKey} fields. */
function isEventKeyShape(value: unknown): value is CollectorEventKey {
  if (value === null || typeof value !== "object") return false;
  const k = value as Record<string, unknown>;
  return (
    typeof k.scenario === "string" &&
    typeof k.editStepIndex === "number" &&
    typeof k.driver === "string" &&
    typeof k.provider === "string" &&
    typeof k.probe === "string" &&
    typeof k.version === "number" &&
    typeof k.anchor === "string"
  );
}

/**
 * Validate that a parsed JSON value is a structurally-complete {@link CollectorEvent}.
 * The deserializer is a trust boundary (it reads a file an earlier run wrote), so a
 * truncated or hand-edited record is rejected loudly rather than silently coerced into
 * a half-built event downstream stages would misread.
 */
function isCollectorEventShape(value: unknown): value is CollectorEvent {
  if (value === null || typeof value !== "object") return false;
  const e = value as Record<string, unknown>;
  // `collector` and `signal` must belong to the closed collector taxonomies, not merely
  // be strings: an unregistered signal must never enter a downstream finding reducer.
  if (!isCollectorName(e.collector) || !isCollectorSignal(e.signal)) return false;
  if (typeof e.ok !== "boolean" || !isSeverity(e.severity)) return false;
  if (typeof e.detail !== "string") return false;
  if (e.provenance === null || typeof e.provenance !== "object") return false;
  if (typeof (e.provenance as Record<string, unknown>).detectedBy !== "string") return false;
  return isEventKeyShape(e.key);
}

/**
 * Parse newline-delimited JSON back into {@link CollectorEvent}s. Blank lines (and a
 * present-or-absent trailing newline) are tolerated; a malformed or structurally
 * incomplete line raises a {@link ReportEventsError} naming its 1-based line number.
 * An event written WITHOUT `data` round-trips without gaining a `data` key.
 */
export function parseEvents(text: string): CollectorEvent[] {
  const events: CollectorEvent[] = [];
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (line.length === 0) continue;
    let parsed: unknown;
    try {
      parsed = JSON.parse(line);
    } catch {
      throw new ReportEventsError(`dx-events JSONL line ${i + 1} is not valid JSON`);
    }
    if (!isCollectorEventShape(parsed)) {
      throw new ReportEventsError(
        `dx-events JSONL line ${i + 1} is not a structurally-complete CollectorEvent`,
      );
    }
    events.push(parsed);
  }
  return events;
}

/** Serialize and write the event stream to `filePath` (with a trailing newline). */
export function writeEventsJsonl(filePath: string, events: readonly CollectorEvent[]): void {
  writeFileSync(filePath, serializeEvents(events, { trailingNewline: true }), "utf8");
}

/** Read and parse an event stream from `filePath`. */
export function readEventsJsonl(filePath: string): CollectorEvent[] {
  return parseEvents(readFileSync(filePath, "utf8"));
}

/**
 * A buffered, file-backed {@link EventSink}: the report-layer sibling of the
 * in-memory `CollectingSink`. A live collection run emits into it; the run then
 * {@link JsonlEventSink.writeTo}s the buffered events to `dx-events.jsonl`. Buffering
 * (rather than appending per emit) keeps the sink pure and the written file order
 * identical to {@link serializeEvents} over the same events.
 */
export class JsonlEventSink implements EventSink {
  readonly events: CollectorEvent[] = [];

  emit(event: CollectorEvent): void {
    this.events.push(event);
  }

  /** Serialize the buffered events (identical to {@link serializeEvents}). */
  serialize(options: SerializeEventsOptions = {}): string {
    return serializeEvents(this.events, options);
  }

  /** Flush the buffered events to a `dx-events.jsonl` file. */
  writeTo(filePath: string): void {
    writeEventsJsonl(filePath, this.events);
  }
}
