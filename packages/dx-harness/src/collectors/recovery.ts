/**
 * Recovery-after-burst collector (raw-LSP).
 *
 * Applies a burst WITHOUT waiting between characters, then waits for quiescence
 * (diagnostics stability + stats stability + provider-query success — the shared
 * quiescence gate, reused, not re-implemented). The pass condition:
 * completions / hover / diagnostics return to a baseline-EQUIVALENT state within the
 * recovery threshold AND no correlated critical/user-visible/candidate signal appears
 * during the window. Any of those — a drifted completion set, a hover/diagnostics
 * mismatch, an over-threshold settle, a failure to quiesce, or a correlated log/probe
 * signal — fails recovery.
 */

import { diagnosticIdentityKey, stripUnstableDocs } from "../differential/index.js";
import { normalizeCompletion, normalizeHover } from "../normalize/index.js";
import type { CompletionResponse, HoverResponse } from "../normalize/lspTypes.js";
import type { EditStep } from "../scenario/index.js";
import {
  offsetToPosition,
  openDocument,
  sendTickChange,
  type CollectorLspClient,
} from "./client.js";
import { DiagnosticsAccumulator } from "./diagnostics.js";
import { EditBuffer, runEditScript } from "./editLoop.js";
import {
  atLeastAsSevere,
  collectorEvent,
  type CollectorEvent,
  type CollectorEventKey,
  type EventSink,
  type Severity,
  type SignalProvenance,
} from "./event.js";
import { monotonicNow } from "./latency.js";

const RAW_LSP: SignalProvenance = { detectedBy: "rawLsp" };

/** A settled probe snapshot used to judge baseline-equivalence after a burst. */
export interface ProbeSnapshot {
  /** The completion label set at the anchor. */
  readonly completionLabels: readonly string[];
  /** The stripped hover type label at the anchor (`null` = no hover). */
  readonly hoverLabel: string | null;
  /** The diagnostic identity keys for the document. */
  readonly diagnosticKeys: readonly string[];
}

/** A correlated signal observed during the recovery window (from logs or the probes). */
export interface CorrelatedSignal {
  readonly severity: Severity;
  readonly signal: string;
}

/** The pure inputs to one recovery decision. */
export interface RecoveryInput {
  readonly key: CollectorEventKey;
  /** The pre-burst settled snapshot. */
  readonly baseline: ProbeSnapshot;
  /** The post-burst settled snapshot. */
  readonly afterBurst: ProbeSnapshot;
  /** The measured time to re-quiesce after the burst (milliseconds). */
  readonly recoveredMs?: number;
  /** The max recovery time tolerated; over it fails. */
  readonly maxRecoveryMs?: number;
  /** Whether quiescence was reached after the burst (default `true`). */
  readonly quiesced?: boolean;
  /** Correlated signals observed during the window; any one fails recovery. */
  readonly correlatedSignals?: readonly CorrelatedSignal[];
}

/**
 * Whether two string lists are equal as SETS (order- and duplicate-insensitive).
 * Compares distinct membership both directions: `["count","name"]` and
 * `["count","count"]` are NOT equal (a length+one-way-membership check would wrongly
 * accept them, since the duplicate inflates the length to match).
 */
function setsEqual(a: readonly string[], b: readonly string[]): boolean {
  const setA = new Set(a);
  const setB = new Set(b);
  if (setA.size !== setB.size) return false;
  for (const value of setA) if (!setB.has(value)) return false;
  return true;
}

/** Whether two settled snapshots are baseline-equivalent (completions, hover, diagnostics). */
function snapshotsEquivalent(a: ProbeSnapshot, b: ProbeSnapshot): boolean {
  return (
    setsEqual(a.completionLabels, b.completionLabels) &&
    a.hoverLabel === b.hoverLabel &&
    setsEqual(a.diagnosticKeys, b.diagnosticKeys)
  );
}

/** The most severe correlated signal's severity, or `null` if there are none. */
function worstSeverity(signals: readonly CorrelatedSignal[]): Severity | null {
  let worst: Severity | null = null;
  for (const signal of signals) {
    if (worst === null || atLeastAsSevere(signal.severity, worst)) worst = signal.severity;
  }
  return worst;
}

/**
 * Decide whether recovery succeeded. Passes only when the post-burst snapshot is
 * baseline-equivalent, quiescence was reached, the settle was within threshold, and no
 * correlated signal appeared. On failure the severity is the worst correlated signal's
 * (escalating a correlated crash to `critical`), defaulting to `userVisible`.
 */
export function decideRecovery(input: RecoveryInput): CollectorEvent {
  const { key, baseline, afterBurst } = input;
  const quiesced = input.quiesced ?? true;
  const correlatedSignals = input.correlatedSignals ?? [];
  const equivalent = snapshotsEquivalent(baseline, afterBurst);
  const withinThreshold =
    input.maxRecoveryMs === undefined ||
    (input.recoveredMs !== undefined && input.recoveredMs <= input.maxRecoveryMs);
  const worst = worstSeverity(correlatedSignals);

  const recovered = quiesced && equivalent && withinThreshold && worst === null;
  const data = {
    equivalent,
    quiesced,
    withinThreshold,
    recoveredMs: input.recoveredMs,
    maxRecoveryMs: input.maxRecoveryMs,
    correlatedSignals,
    // The driven snapshots are carried on the event so the report layer (and the
    // env-gated live suite) can confirm the probes actually ran — a constant-empty
    // snapshot would surface here as empty completion labels and a null hover label.
    baseline,
    afterBurst,
  };

  if (recovered) {
    return collectorEvent({
      collector: "recovery",
      signal: "recovery_baseline_restored",
      ok: true,
      severity: "userVisible",
      provenance: RAW_LSP,
      key,
      detail:
        "completions / hover / diagnostics returned to a baseline-equivalent state after the burst",
      data,
    });
  }

  const reasons: string[] = [];
  if (!quiesced) reasons.push("did not quiesce");
  if (!equivalent) reasons.push("state did not return to baseline");
  if (!withinThreshold) reasons.push("recovery exceeded the time threshold");
  if (worst !== null) reasons.push(`a correlated ${worst} signal appeared`);

  return collectorEvent({
    collector: "recovery",
    signal: "recovery_not_restored",
    ok: false,
    severity: worst ?? "userVisible",
    provenance: RAW_LSP,
    key,
    detail: `recovery failed: ${reasons.join("; ")}`,
    data,
  });
}

// ── live raw-LSP driver ──────────────────────────────────────────────────────

/** Options for the live {@link collectRecovery} run. */
export interface CollectRecoveryOptions {
  readonly client: CollectorLspClient;
  readonly sink: EventSink;
  readonly uri: string;
  readonly languageId?: string;
  readonly buffer: EditBuffer;
  /** The burst typed without waiting between characters. */
  readonly burst: readonly EditStep[];
  readonly scenario: string;
  readonly probe: string;
  /** The named snapshot anchor: completions + hover are sampled here, before and after the burst. */
  readonly anchor: string;
  readonly provider: string;
  readonly maxRecoveryMs?: number;
  /**
   * Reach quiescence (the shared quiescence gate — diagnostics stability + stats
   * stability + provider-query success) and report whether it settled. REUSED, not
   * re-implemented here; awaited once to settle the baseline and again after the burst.
   */
  readonly awaitQuiescence: () => Promise<boolean>;
  /** The correlated critical/user-visible/candidate signals observed during the window. */
  readonly correlatedSignals: () => readonly CorrelatedSignal[];
  /** Per-request timeout for the completion / hover probes (milliseconds). */
  readonly requestTimeoutMs?: number;
  readonly alreadyOpen?: boolean;
}

/**
 * Capture the settled probe snapshot at the snapshot anchor: verter's completion label
 * set + stripped hover type label (driven directly through the shared
 * `textDocument/completion` / `textDocument/hover` request path and the shared
 * normalizers) plus the document's published-diagnostic identity keys (read from the
 * accumulated `publishDiagnostics` stream). This is the recovery collector DRIVING the
 * real probes — never an opaque caller-supplied snapshot.
 */
async function captureProbeSnapshot(
  options: CollectRecoveryOptions,
  diagnostics: DiagnosticsAccumulator,
): Promise<ProbeSnapshot> {
  const { client, uri, buffer } = options;
  const anchorOffset = buffer.anchorOffset(options.anchor);
  const position = offsetToPosition(buffer.text, anchorOffset, client.positionEncoding);

  const completionRaw = await client.sendRequest<CompletionResponse>(
    "textDocument/completion",
    { textDocument: { uri }, position },
    options.requestTimeoutMs,
  );
  const completionLabels = normalizeCompletion(completionRaw).items.map((item) => item.label);

  const hoverRaw = await client.sendRequest<HoverResponse>(
    "textDocument/hover",
    { textDocument: { uri }, position },
    options.requestTimeoutMs,
  );
  const hover = normalizeHover(hoverRaw);
  const hoverLabel = hover === null ? null : stripUnstableDocs(hover.contents) || null;

  const diagnosticKeys = diagnostics.forUri(uri).map(diagnosticIdentityKey);
  return { completionLabels, hoverLabel, diagnosticKeys };
}

/**
 * Drive a recovery observation: settle the freshly-opened document, capture the
 * baseline snapshot by ISSUING the real completion/hover/diagnostics probes, fire the
 * burst without waiting between characters, time the re-quiescence, capture the
 * post-burst snapshot the same way, and decide. The burst is sent as raw `didChange`
 * notifications with no per-tick sampling — exactly the stress pattern recovery
 * measures resilience against.
 */
export async function collectRecovery(options: CollectRecoveryOptions): Promise<void> {
  const { client, sink, uri, buffer } = options;
  const diagnostics = new DiagnosticsAccumulator();
  client.onNotification("textDocument/publishDiagnostics", diagnostics.handle);
  try {
    if (options.alreadyOpen !== true) {
      openDocument(client, uri, options.languageId ?? "vue", buffer.text, buffer.version);
    }

    // Settle the freshly-opened document so the baseline snapshot is a stable reference.
    await options.awaitQuiescence();
    const baseline = await captureProbeSnapshot(options, diagnostics);

    // Fire the whole burst without waiting between characters.
    await runEditScript(buffer, options.burst, (tick) => {
      sendTickChange(client, uri, tick);
    });

    // Time ONLY the post-burst re-quiescence (the initial settle above is not counted).
    const started = monotonicNow();
    const quiesced = await options.awaitQuiescence();
    const recoveredMs = monotonicNow() - started;

    const afterBurst = await captureProbeSnapshot(options, diagnostics);
    const key: CollectorEventKey = {
      scenario: options.scenario,
      editStepIndex: options.burst.length - 1,
      driver: "rawLsp",
      provider: options.provider,
      probe: options.probe,
      version: buffer.version,
      anchor: options.anchor,
    };
    sink.emit(
      decideRecovery({
        key,
        baseline,
        afterBurst,
        recoveredMs,
        quiesced,
        correlatedSignals: options.correlatedSignals(),
        ...(options.maxRecoveryMs !== undefined ? { maxRecoveryMs: options.maxRecoveryMs } : {}),
      }),
    );
  } finally {
    client.offNotification("textDocument/publishDiagnostics", diagnostics.handle);
  }
}
