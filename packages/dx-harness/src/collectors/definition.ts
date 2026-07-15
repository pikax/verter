/**
 * Definition collector (raw-LSP).
 *
 * Fails by SYMBOL IDENTITY — the expected source file and source range — and by the
 * generated-only-unmapped target, never by `line === 0`. The comparison itself is the
 * shared `compareDefinition` comparator (which matches by file+range, projects
 * generated targets back through the source map, and surfaces the generated-only-with-
 * no-way-back case as its own `unmappedGenerated` divergence); the collector drives it
 * and classifies the findings' severities. A wrong or unmapped definition target is
 * user-visible (go-to-definition lands in the wrong place, or inside a generated
 * artifact the user cannot navigate back from).
 */

import {
  compareDefinition,
  type BaselineLocations,
  type ParsedSourceMap,
} from "../differential/index.js";
import { normalizeDefinition, type CanonicalDefinitionTarget } from "../normalize/index.js";
import type { ExpectedDefinition } from "../normalize/index.js";
import type { DefinitionResponse } from "../normalize/lspTypes.js";
import type { EditStep } from "../scenario/index.js";
import {
  offsetToPosition,
  openDocument,
  sendTickChange,
  type CollectorLspClient,
} from "./client.js";
import { EditBuffer, runEditScript } from "./editLoop.js";
import {
  collectorEvent,
  type CollectorEvent,
  type CollectorEventKey,
  type EventSink,
  type SignalProvenance,
} from "./event.js";

const RAW_LSP: SignalProvenance = { detectedBy: "rawLsp" };

/** A baseline provider's resolved locations for one sample (for parity). */
export interface DefinitionBaseline {
  readonly provider: string;
  readonly locations: BaselineLocations;
}

/** The pure inputs to one definition-sample classification. */
export interface DefinitionSampleInput {
  readonly key: CollectorEventKey;
  /** Verter's normalized definition targets. */
  readonly verter: readonly CanonicalDefinitionTarget[];
  /** The expected authored-Vue symbol identity; when present it governs the comparison. */
  readonly expected?: ExpectedDefinition;
  /** Verter's emitted source map, for projecting generated targets back to authored Vue space. */
  readonly map?: ParsedSourceMap;
  /** An optional baseline provider's locations for verter-vs-baseline parity. */
  readonly baseline?: DefinitionBaseline;
}

/**
 * Classify one definition sample into events. Delegates the comparison to the shared
 * `compareDefinition` (identity by file+range, generated projection, the unmapped-
 * generated case) and folds the divergences into user-visible findings — never failing
 * on a line-0 target.
 */
export function classifyDefinitionSample(input: DefinitionSampleInput): CollectorEvent[] {
  const { key, verter, expected, map, baseline } = input;
  const events: CollectorEvent[] = [];
  const findings = compareDefinition(verter, {
    ...(expected !== undefined ? { expected } : {}),
    ...(map !== undefined ? { map } : {}),
    ...(baseline !== undefined ? { baseline: baseline.locations } : {}),
  });

  const note = baseline !== undefined ? `baseline=${baseline.provider}` : undefined;
  const provenance: SignalProvenance =
    note !== undefined ? { detectedBy: "rawLsp", note } : RAW_LSP;

  if (findings.length === 0) {
    events.push(
      collectorEvent({
        collector: "definition",
        signal: "definition_parity",
        ok: true,
        severity: "userVisible",
        provenance,
        key,
        detail:
          expected !== undefined
            ? "definition resolved to the expected authored identity"
            : "definition agrees with the baseline location set",
      }),
    );
    return events;
  }

  for (const finding of findings) {
    events.push(
      collectorEvent({
        collector: "definition",
        signal: "definition_parity",
        ok: false,
        severity: "userVisible",
        provenance,
        key,
        detail: finding.detail,
        data: {
          class: finding.class,
          verterValue: finding.verterValue,
          baselineValue: finding.baselineValue,
        },
      }),
    );
  }
  return events;
}

// ── live raw-LSP driver ──────────────────────────────────────────────────────

/** Options for the live {@link collectDefinition} run. */
export interface CollectDefinitionOptions {
  readonly client: CollectorLspClient;
  readonly sink: EventSink;
  readonly uri: string;
  readonly languageId?: string;
  readonly buffer: EditBuffer;
  readonly script?: readonly EditStep[];
  readonly scenario: string;
  readonly probe: string;
  readonly anchor: string;
  readonly provider: string;
  readonly expected?: ExpectedDefinition;
  readonly map?: ParsedSourceMap;
  readonly requestTimeoutMs?: number;
  readonly baselineAt?: (
    anchorOffset: number,
    version: number,
  ) => Promise<DefinitionBaseline | undefined>;
}

/**
 * Drive verter through the (optional) edit script, then sample definition at the
 * settled anchor and classify it against the expected identity / baseline.
 */
export async function collectDefinition(options: CollectDefinitionOptions): Promise<void> {
  const { client, sink, uri, buffer } = options;
  const script = options.script ?? [];
  openDocument(client, uri, options.languageId ?? "vue", buffer.text, buffer.version);
  await runEditScript(buffer, script, (tick) => {
    sendTickChange(client, uri, tick);
  });

  const anchorOffset = buffer.anchorOffset(options.anchor);
  const position = offsetToPosition(buffer.text, anchorOffset, client.positionEncoding);
  const raw = await client.sendRequest<DefinitionResponse>(
    "textDocument/definition",
    { textDocument: { uri }, position },
    options.requestTimeoutMs,
  );
  const verter = normalizeDefinition(raw);
  const baseline = await options.baselineAt?.(anchorOffset, buffer.version);
  const key: CollectorEventKey = {
    scenario: options.scenario,
    editStepIndex: script.length - 1,
    driver: "rawLsp",
    provider: options.provider,
    probe: options.probe,
    version: buffer.version,
    anchor: options.anchor,
  };
  for (const event of classifyDefinitionSample({
    key,
    verter,
    ...(options.expected !== undefined ? { expected: options.expected } : {}),
    ...(options.map !== undefined ? { map: options.map } : {}),
    ...(baseline !== undefined ? { baseline } : {}),
  })) {
    sink.emit(event);
  }
}
