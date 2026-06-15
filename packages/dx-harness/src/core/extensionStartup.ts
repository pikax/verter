/**
 * Extension-host startup-readiness parser.
 *
 * When Verter runs inside VS Code there is no raw notification stream to observe —
 * the extension consumes `$/verter/ready` and `$/verter/typeProviderSyncComplete`
 * itself and re-emits them to its output channel as human-readable log lines
 * (packages/vue-vscode/src/extension.ts:864,869):
 *
 *   log.info(`Verter ready (init generation ${params.gen})`)
 *   log.info(`TypeProviderSyncComplete (init generation ${params.gen})`)
 *
 * This module recovers the init generations from those log lines and reuses the
 * shared {@link ./generationGate} matching core — so the extension-host gate has
 * exactly the same newest-generation-wins, both-channels-required semantics as the
 * raw-LSP gate, with no second engine.
 *
 * It is a pure function over log lines, testable WITHOUT launching VS Code.
 */
import {
  GenerationGate,
  evaluateGenerationGate,
  type GenerationEvent,
  type GenerationGateDecision,
} from "./generationGate.js";

/** Matches the extension's `Verter ready (init generation N)` log line. */
export const VERTER_READY_LOG_PATTERN = /Verter ready \(init generation (\d+)\)/;
/** Matches the extension's `TypeProviderSyncComplete (init generation N)` log line. */
export const TYPE_PROVIDER_SYNC_COMPLETE_LOG_PATTERN =
  /TypeProviderSyncComplete \(init generation (\d+)\)/;

/**
 * Parse a single extension log line into a {@link GenerationEvent}, or `null` if
 * it is not a readiness line. The two patterns are mutually exclusive substrings,
 * so a prefixed line (timestamp / `[info]`) still matches.
 */
export function parseStartupLogLine(line: string): GenerationEvent | null {
  const sync = TYPE_PROVIDER_SYNC_COMPLETE_LOG_PATTERN.exec(line);
  if (sync) return { channel: "sync", generation: Number.parseInt(sync[1], 10) };
  const ready = VERTER_READY_LOG_PATTERN.exec(line);
  if (ready) return { channel: "ready", generation: Number.parseInt(ready[1], 10) };
  return null;
}

/**
 * Evaluate the extension-host startup gate over a finite batch of log lines.
 * Non-readiness lines are ignored.
 */
export function parseExtensionStartupLog(lines: Iterable<string>): GenerationGateDecision {
  const events: GenerationEvent[] = [];
  for (const line of lines) {
    const event = parseStartupLogLine(line);
    if (event) events.push(event);
  }
  return evaluateGenerationGate(events);
}

/**
 * Streaming variant of {@link parseExtensionStartupLog} for a live log feed: push
 * lines one at a time and read {@link satisfied} as they arrive.
 */
export class ExtensionStartupGate {
  private readonly gate = new GenerationGate();

  /** Parse and incorporate one log line; returns the parsed event (or `null`). */
  observeLine(line: string): GenerationEvent | null {
    const event = parseStartupLogLine(line);
    if (event) this.gate.observe(event);
    return event;
  }

  get decision(): GenerationGateDecision {
    return this.gate.decision;
  }

  get satisfied(): boolean {
    return this.gate.satisfied;
  }

  get matchedGeneration(): number | null {
    return this.gate.matchedGeneration;
  }
}
