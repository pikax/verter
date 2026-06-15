/**
 * Compile/parse churn collector (raw-LSP).
 *
 * The day-1 mechanism is `$/verter/getStatistics`: the host's `host:compile` counter
 * delta across a quiesced steady-state edit. HONEST SCOPE — stated here and recorded
 * on every event: `compile_requests` is a SINGLE GLOBAL atomic, bumped by every
 * `get_virtual_file` recompile including the background scanner, the drain, and the
 * sync coordinator, so per-keystroke attribution DURING A BURST is not obtainable from
 * it. The attributable signal is the per-quiesced-edit delta in steady state; a burst
 * reports the aggregate pre-burst → post-quiescence delta, never per-character
 * attribution. When an isolation precondition is unmet (an import introduced
 * mid-measurement legitimately triggers a provider sync, so it is an import-sync
 * scenario, not steady-state churn), the event records the uncertainty rather than
 * asserting a false churn failure. Escalating to gated host-side counters is a
 * separate product change; this measurement stays honest.
 */

import { extractQuiescenceCounters, type QuiescenceCounters } from "../core/quiescence.js";
import { GET_STATISTICS_METHOD } from "../core/startupGate.js";
import type { EditStep } from "../scenario/index.js";
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

/** The note recorded on coarse measurements explaining the global-counter limitation. */
const GLOBAL_COUNTER_NOTE =
  "host:compile is a single global atomic bumped by background scanner / drain / sync paths too; " +
  "per-keystroke-during-burst attribution is not obtainable from it";

/** The isolation preconditions a steady-state churn measurement requires. */
export interface ChurnPreconditions {
  /** The matching-generation typeProviderSyncComplete was reached. */
  readonly syncGenerationMatched: boolean;
  /** Host/log quiescence held before the measured edit. */
  readonly quiescedBefore: boolean;
  /** Host/log quiescence was re-reached after the measured edit. */
  readonly quiescedAfter: boolean;
  /** Exactly one document was open for the measurement. */
  readonly singleDocumentOpen: boolean;
  /** No new import was introduced mid-measurement (which would trigger a provider sync). */
  readonly noNewImportsMidMeasurement: boolean;
}

/** The measurement scope: a single quiesced edit (attributable) vs a burst aggregate. */
export type ChurnMode = "steadyStateQuiescedEdit" | "burstAggregate";

/** The pure inputs to one churn classification. */
export interface ChurnInput {
  readonly key: CollectorEventKey;
  /** The host counters before the measured edit (at quiescence). */
  readonly pre: QuiescenceCounters;
  /** The host counters after the measured edit (at quiescence). */
  readonly post: QuiescenceCounters;
  readonly preconditions: ChurnPreconditions;
  readonly mode: ChurnMode;
  /** The max compile delta tolerated; over it is flagged. Omitted = report-only. */
  readonly threshold?: number;
}

/** The `host:compile` counter delta across the measurement. */
export function steadyStateCompileDelta(pre: QuiescenceCounters, post: QuiescenceCounters): number {
  return post.compile - pre.compile;
}

/** The names of the unmet preconditions, in declaration order. */
function unmetPreconditions(p: ChurnPreconditions): string[] {
  const unmet: string[] = [];
  if (!p.syncGenerationMatched) unmet.push("syncGenerationMatched");
  if (!p.quiescedBefore) unmet.push("quiescedBefore");
  if (!p.quiescedAfter) unmet.push("quiescedAfter");
  if (!p.singleDocumentOpen) unmet.push("singleDocumentOpen");
  if (!p.noNewImportsMidMeasurement) unmet.push("noNewImportsMidMeasurement");
  return unmet;
}

/**
 * Classify one churn measurement. When an isolation precondition is unmet the
 * measurement is not attributable, so it is recorded as `churn_attribution_uncertain`
 * (a `candidate`, `ok` — an honest "cannot attribute", not a failure). When the
 * preconditions hold, a steady-state delta over threshold is a user-visible recompute
 * storm; a burst reports the aggregate delta as a `candidate` and explicitly does NOT
 * claim per-character attribution.
 */
export function classifyChurn(input: ChurnInput): CollectorEvent {
  const { key, pre, post, preconditions, mode, threshold } = input;
  const delta = steadyStateCompileDelta(pre, post);
  const unmet = unmetPreconditions(preconditions);

  if (unmet.length > 0) {
    return collectorEvent({
      collector: "churn",
      signal: "churn_attribution_uncertain",
      ok: true,
      severity: "candidate",
      provenance: { detectedBy: "rawLsp", note: GLOBAL_COUNTER_NOTE },
      key,
      detail: `compile delta ${delta} is not attributable to steady-state churn (unmet: ${unmet.join(", ")})`,
      data: { scope: mode, delta, attributable: false, unmet, note: GLOBAL_COUNTER_NOTE },
    });
  }

  if (mode === "burstAggregate") {
    const over = threshold !== undefined && delta > threshold;
    return collectorEvent({
      collector: "churn",
      // A burst aggregate is a coarse candidate signal — never asserted user-visible.
      signal: "churn_burst_aggregate",
      ok: !over,
      severity: "candidate",
      provenance: { detectedBy: "rawLsp", note: GLOBAL_COUNTER_NOTE },
      key,
      detail: `burst aggregate compile delta ${delta}${threshold !== undefined ? ` (threshold ${threshold})` : ""}`,
      data: {
        scope: mode,
        delta,
        attributable: true,
        perCharacterAttribution: false,
        note: GLOBAL_COUNTER_NOTE,
      },
    });
  }

  const over = threshold !== undefined && delta > threshold;
  return collectorEvent({
    collector: "churn",
    signal: "churn_steady_state_delta",
    ok: !over,
    severity: "userVisible",
    provenance: RAW_LSP,
    key,
    detail: `steady-state compile delta ${delta}${threshold !== undefined ? ` (threshold ${threshold})` : ""}`,
    data: { scope: mode, delta, attributable: true },
  });
}

// ── live raw-LSP driver ──────────────────────────────────────────────────────

/** The static (scenario-known) preconditions the caller asserts; quiescence is measured. */
export interface StaticChurnPreconditions {
  readonly syncGenerationMatched: boolean;
  readonly singleDocumentOpen: boolean;
  readonly noNewImportsMidMeasurement: boolean;
}

/** Options for the live {@link collectChurn} run. */
export interface CollectChurnOptions {
  readonly client: CollectorLspClient;
  readonly sink: EventSink;
  readonly uri: string;
  readonly languageId?: string;
  readonly buffer: EditBuffer;
  /** The measured edit script (a single step for steady-state, a burst for aggregate). */
  readonly script: readonly EditStep[];
  readonly scenario: string;
  readonly probe: string;
  readonly anchor: string;
  readonly provider: string;
  readonly mode: ChurnMode;
  readonly threshold?: number;
  readonly preconditions: StaticChurnPreconditions;
  /**
   * Reach host/log quiescence and report whether it was attained — wires the shared
   * quiescence gate (this collector reuses it, never re-implements it).
   */
  readonly awaitQuiescence: () => Promise<boolean>;
  readonly statisticsTimeoutMs?: number;
  /** Whether the document is already open (skip the open notification). Default false. */
  readonly alreadyOpen?: boolean;
}

/** Read the host quiescence counters via `$/verter/getStatistics`. */
async function readCounters(
  client: CollectorLspClient,
  timeout?: number,
): Promise<QuiescenceCounters> {
  const snapshot = await client.sendRequest(GET_STATISTICS_METHOD, {}, timeout);
  return extractQuiescenceCounters(snapshot);
}

/**
 * Drive a measured churn observation: quiesce, read the pre counters, apply the edit
 * script, re-quiesce, read the post counters, and classify the delta. The
 * before/after quiescence outcomes feed the precondition set so a measurement taken
 * without quiescence is recorded as unattributable rather than as false churn.
 */
export async function collectChurn(options: CollectChurnOptions): Promise<void> {
  const { client, sink, uri, buffer } = options;
  if (options.alreadyOpen !== true) {
    openDocument(client, uri, options.languageId ?? "vue", buffer.text, buffer.version);
  }

  const quiescedBefore = await options.awaitQuiescence();
  const pre = await readCounters(client, options.statisticsTimeoutMs);

  await runEditScript(buffer, options.script, (tick) => {
    sendTickChange(client, uri, tick);
  });

  const quiescedAfter = await options.awaitQuiescence();
  const post = await readCounters(client, options.statisticsTimeoutMs);

  const key: CollectorEventKey = {
    scenario: options.scenario,
    editStepIndex: options.script.length - 1,
    driver: "rawLsp",
    provider: options.provider,
    probe: options.probe,
    version: buffer.version,
    anchor: options.anchor,
  };
  sink.emit(
    classifyChurn({
      key,
      pre,
      post,
      preconditions: {
        syncGenerationMatched: options.preconditions.syncGenerationMatched,
        quiescedBefore,
        quiescedAfter,
        singleDocumentOpen: options.preconditions.singleDocumentOpen,
        noNewImportsMidMeasurement: options.preconditions.noNewImportsMidMeasurement,
      },
      mode: options.mode,
      ...(options.threshold !== undefined ? { threshold: options.threshold } : {}),
    }),
  );
}
