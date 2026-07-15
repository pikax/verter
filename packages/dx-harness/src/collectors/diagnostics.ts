/**
 * Diagnostics collector (raw-LSP).
 *
 * Collects push-delivered `textDocument/publishDiagnostics` and classifies them. The
 * component diagnostic fallback is specifically the DEFAULT `(0,0)` range on offset-
 * mapping failure — NOT any line-0 range — so the impossible/default flag fires only
 * when the known source span does not start at the origin (a mapping failure) or the
 * range is the zero-width default sentinel (an impossible extent). The shared
 * `isImpossibleDefaultDiagnostic` predicate makes that decision; `compareDiagnostics`
 * drives verter-vs-baseline parity (verter-only / baseline-only / mapped-vs-default-
 * range / severity).
 */

import {
  classifyOracleDiagnostics,
  compareDiagnostics,
  type DifferentialOutcome,
  type GeneratedDocument,
  type ProviderInputs,
} from "../differential/index.js";
import type { NormalizedDiagnostic, ProviderName } from "../baseline/bridgeClient.js";
import {
  isImpossibleDefaultDiagnostic,
  normalizeDiagnostics,
  type CanonicalDiagnostic,
  type Range,
} from "../normalize/index.js";
import type { DiagnosticsResponse } from "../normalize/lspTypes.js";
import type { EditStep, Probe } from "../scenario/index.js";
import { openDocument, sendTickChange, type CollectorLspClient } from "./client.js";
import { EditBuffer, runEditScript } from "./editLoop.js";
import {
  collectorEvent,
  type CollectorEvent,
  type CollectorEventKey,
  type EventSink,
  type SignalProvenance,
} from "./event.js";

const RAW_LSP: SignalProvenance = { detectedBy: "rawLsp" };

/** A baseline provider's diagnostics for one sample, plus the emitted-TSX converter. */
export interface DiagnosticsBaseline {
  readonly provider: string;
  readonly diagnostics: readonly NormalizedDiagnostic[];
  /** The emitted-TSX byte→position converter (the baseline's offsets index into it). */
  readonly document: GeneratedDocument;
}

/**
 * A curated-oracle (vue semantic validity) comparison: verter's diagnostics for the
 * `.vue` document classified against a hand-authored `.ts` gold standard's diagnostics
 * via the SHARED {@link classifyOracleDiagnostics}. Distinct from artifact parity
 * ({@link DiagnosticsBaseline}), which projects baseline byte offsets through a shared
 * emitted-TSX {@link GeneratedDocument}; the oracle is a DIFFERENT file from the `.vue`,
 * so it pairs by code/severity identity and never compares the raw `.ts` byte range.
 */
export interface DiagnosticsOracle {
  /** The oracle probe identity (mapping policy `none`, `vueSemanticValidity` dimension). */
  readonly probe: Probe;
  /** The oracle providers' diagnostics for the mirrored `.ts` document. */
  readonly providers: ProviderInputs<readonly NormalizedDiagnostic[]>;
  /** Per-code authored `.vue` spans (the default-range collapse / expected-range oracle). */
  readonly knownSourceSpans?: Readonly<Record<string, Range>>;
  /** When set, verter is compared only against this oracle provider. */
  readonly authoritativeProvider?: ProviderName;
}

/** The pure inputs to one diagnostics-sample classification. */
export interface DiagnosticsSampleInput {
  readonly key: CollectorEventKey;
  /** Verter's normalized diagnostics. */
  readonly verter: readonly CanonicalDiagnostic[];
  /** An optional baseline provider's diagnostics for verter-vs-baseline parity. */
  readonly baseline?: DiagnosticsBaseline;
  /** An optional curated-oracle (vue semantic validity) comparison against a `.ts` gold standard. */
  readonly oracle?: DiagnosticsOracle;
  /** Per-code known true source spans, overriding the default-range decision's oracle. */
  readonly knownSourceSpans?: Readonly<Record<string, Range>>;
}

/** Provenance for the curated-oracle (vue semantic validity) diagnostics findings. */
const ORACLE_PROVENANCE: SignalProvenance = {
  detectedBy: "rawLsp",
  note: "curated .ts oracle — vue semantic validity",
};

/**
 * Map one shared-oracle outcome into a `diagnostics_vue_semantic_validity` event. A
 * `divergence` is the ONLY verter-fault outcome (the curated `.ts` gold standard
 * disagreed with verter's `.vue` diagnostics); agreement and every refusal/baseline-
 * disagreement outcome are recorded as `ok` observations, never failures.
 */
function oracleDiagnosticsEvent(
  outcome: DifferentialOutcome,
  key: CollectorEventKey,
): CollectorEvent {
  if (outcome.kind === "divergence") {
    return collectorEvent({
      collector: "diagnostics",
      signal: "diagnostics_vue_semantic_validity",
      ok: false,
      severity: "userVisible",
      provenance: ORACLE_PROVENANCE,
      key,
      detail: `verter diagnostics diverged from the curated oracle: ${outcome.detail}`,
      data: {
        kind: outcome.kind,
        class: outcome.class,
        provider: outcome.provider,
        verterValue: outcome.verterValue,
        baselineValue: outcome.baselineValue,
      },
    });
  }
  const detail =
    outcome.kind === "agreement"
      ? "verter diagnostics agree with the curated oracle"
      : `curated oracle not compared (${outcome.kind})`;
  return collectorEvent({
    collector: "diagnostics",
    signal: "diagnostics_vue_semantic_validity",
    ok: true,
    severity: "userVisible",
    provenance: ORACLE_PROVENANCE,
    key,
    detail,
    data: { kind: outcome.kind },
  });
}

/**
 * Classify one diagnostics sample. With a baseline, the shared `compareDiagnostics`
 * comparator drives the parity findings; without one, each verter diagnostic is checked
 * for the impossible/default `(0,0)` collapse. Both routes reuse the shared
 * `isImpossibleDefaultDiagnostic` predicate (compareDiagnostics internally, the
 * standalone path directly), so a precise positive-width line-0 diagnostic is never
 * flagged.
 */
export function classifyDiagnosticsSample(input: DiagnosticsSampleInput): CollectorEvent[] {
  const { key, verter, baseline, knownSourceSpans } = input;
  const events: CollectorEvent[] = [];

  // Curated oracle (vue semantic validity): verter's `.vue` diagnostics vs a `.ts` gold
  // standard, classified through the SHARED classifyOracleDiagnostics (code/severity
  // identity plus the authored-span default-range check, NEVER a cross-file raw range).
  // Exclusive of the emitted-TSX artifact-parity baseline below.
  if (input.oracle !== undefined) {
    const outcomes = classifyOracleDiagnostics({
      probe: input.oracle.probe,
      verter,
      providers: input.oracle.providers,
      ...(input.oracle.knownSourceSpans !== undefined
        ? { knownSourceSpans: input.oracle.knownSourceSpans }
        : {}),
      ...(input.oracle.authoritativeProvider !== undefined
        ? { authoritativeProvider: input.oracle.authoritativeProvider }
        : {}),
    });
    for (const outcome of outcomes) events.push(oracleDiagnosticsEvent(outcome, key));
    return events;
  }

  if (baseline !== undefined) {
    const findings = compareDiagnostics(verter, baseline.diagnostics, baseline.document, {
      ...(knownSourceSpans !== undefined ? { knownSourceSpans } : {}),
    });
    const provenance: SignalProvenance = {
      detectedBy: "rawLsp",
      note: `baseline=${baseline.provider}`,
    };
    if (findings.length === 0) {
      events.push(
        collectorEvent({
          collector: "diagnostics",
          signal: "diagnostics_parity",
          ok: true,
          severity: "userVisible",
          provenance,
          key,
          detail: "verter diagnostics agree with the baseline set",
        }),
      );
    }
    for (const finding of findings) {
      events.push(
        collectorEvent({
          collector: "diagnostics",
          // The default-range collapse keeps its own signal name even under the parity route.
          signal:
            finding.class === "defaultRange" ? "diagnostics_default_range" : "diagnostics_parity",
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

  // No baseline: flag the impossible/default (0,0) collapse per diagnostic. With a known
  // span the full predicate applies; without one, the diagnostic's OWN range is the oracle
  // so only the zero-width default sentinel (an impossible extent) trips — a precise
  // positive-width line-0 diagnostic does NOT.
  for (const d of verter) {
    const known = (d.code !== undefined ? knownSourceSpans?.[d.code] : undefined) ?? d.range;
    if (!isImpossibleDefaultDiagnostic(d, known)) continue;
    events.push(
      collectorEvent({
        collector: "diagnostics",
        signal: "diagnostics_default_range",
        ok: false,
        severity: "userVisible",
        provenance: RAW_LSP,
        key,
        detail: `diagnostic ${d.code ?? d.message} collapsed to the impossible/default (0,0) range`,
        data: { range: d.range, code: d.code, knownSourceSpan: known },
      }),
    );
  }
  return events;
}

// ── live raw-LSP driver ──────────────────────────────────────────────────────

/** Accumulates the most recent published diagnostics per document URI. */
export class DiagnosticsAccumulator {
  private readonly latest = new Map<string, readonly CanonicalDiagnostic[]>();

  /** Handle one `textDocument/publishDiagnostics` notification. */
  handle = (params: unknown): void => {
    const p = params as { uri?: unknown; diagnostics?: unknown } | null | undefined;
    if (typeof p?.uri !== "string") return;
    this.latest.set(p.uri, normalizeDiagnostics(p.diagnostics as DiagnosticsResponse));
  };

  /** The latest normalized diagnostics for `uri` (empty if none published). */
  forUri(uri: string): readonly CanonicalDiagnostic[] {
    return this.latest.get(uri) ?? [];
  }
}

/** Options for the live {@link collectDiagnostics} run. */
export interface CollectDiagnosticsOptions {
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
  readonly knownSourceSpans?: Readonly<Record<string, Range>>;
  /** Awaited after the edits to let push diagnostics settle (reuse a quiescence gate live). */
  readonly settle: () => Promise<void>;
  readonly baseline?: DiagnosticsBaseline;
  /** An optional curated-oracle (vue semantic validity) comparison against a `.ts` gold standard. */
  readonly oracle?: DiagnosticsOracle;
}

/**
 * Drive verter through the (optional) edit script while accumulating push
 * diagnostics, await the caller-supplied settle, then classify the latest published
 * set for the document.
 */
export async function collectDiagnostics(options: CollectDiagnosticsOptions): Promise<void> {
  const { client, sink, uri, buffer } = options;
  const script = options.script ?? [];
  const accumulator = new DiagnosticsAccumulator();
  client.onNotification("textDocument/publishDiagnostics", accumulator.handle);
  try {
    openDocument(client, uri, options.languageId ?? "vue", buffer.text, buffer.version);
    await runEditScript(buffer, script, (tick) => {
      sendTickChange(client, uri, tick);
    });
    await options.settle();

    const verter = accumulator.forUri(uri);
    const key: CollectorEventKey = {
      scenario: options.scenario,
      editStepIndex: script.length - 1,
      driver: "rawLsp",
      provider: options.provider,
      probe: options.probe,
      version: buffer.version,
      anchor: options.anchor,
    };
    for (const event of classifyDiagnosticsSample({
      key,
      verter,
      ...(options.baseline !== undefined ? { baseline: options.baseline } : {}),
      ...(options.oracle !== undefined ? { oracle: options.oracle } : {}),
      ...(options.knownSourceSpans !== undefined
        ? { knownSourceSpans: options.knownSourceSpans }
        : {}),
    })) {
      sink.emit(event);
    }
  } finally {
    client.offNotification("textDocument/publishDiagnostics", accumulator.handle);
  }
}
