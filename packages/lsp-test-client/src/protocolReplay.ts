/**
 * Protocol replay types and combined-timeline analysis (WSP1).
 *
 * Captures method, byte length, queue residence, encode/decode and
 * completion timestamps. Client apply/paint are WSP1L and are never
 * inferred from these records.
 */

export type ProtocolStage =
  | "request_received"
  | "admitted"
  | "provider_work"
  | "serialize"
  | "outbound_enqueued"
  | "outbound_written"
  | "complete";

export const SERVER_STAGE_ORDER: readonly ProtocolStage[] = [
  "request_received",
  "admitted",
  "provider_work",
  "serialize",
  "outbound_enqueued",
  "outbound_written",
  "complete",
] as const;

export type ProtocolDirection = "client_to_server" | "server_to_client";

/** JSON-RPC RequestCancelled, as observed on the wire. */
export const REQUEST_CANCELLED_CODE = -32800;

/**
 * Terminal state of one request's timeline. Failed and cancelled requests
 * never carry a `complete` stamp: partial, pending, failed and cancelled stay
 * distinct from complete (the client-side mirror of the server's TraceStatus).
 */
export type TraceStatus = "pending" | "complete" | "failed" | "cancelled";

/** One framed JSON-RPC message. No source text. */
export interface ProtocolWireEvent {
  readonly requestEpoch: number;
  readonly method: string;
  readonly direction: ProtocolDirection;
  readonly byteLength: number;
  readonly encodeStartedMs: number;
  readonly encodeCompletedMs: number;
  readonly queuedMs: number;
  readonly decodedMs: number | null;
  readonly completedMs: number | null;
  readonly kind: "request" | "response" | "notification";
  /**
   * JSON-RPC error code when this is an error response (e.g. -32800
   * RequestCancelled). Absent on successful responses and non-responses.
   */
  readonly errorCode?: number;
}

export interface StageStamp {
  readonly stage: ProtocolStage;
  readonly atMs: number;
  readonly byteLength?: number;
}

export interface InteractionTrace {
  readonly requestEpoch: number;
  readonly sourceEpoch: number | null;
  readonly method: string;
  readonly status: TraceStatus;
  readonly stamps: readonly StageStamp[];
  readonly firstBlockedStage: ProtocolStage | null;
}

export interface RecordedLspReplay {
  readonly schema: "recorded-lsp-replay.v1";
  readonly host: "lsp-test-client";
  readonly engineVersion: string | null;
  readonly completenessState:
    | "complete"
    | "complete-empty"
    | "partial"
    | "pending"
    | "unsupported"
    | "ambiguous"
    | "failed"
    | "cancelled"
    | "stale";
  readonly wire: readonly ProtocolWireEvent[];
  readonly traces: readonly InteractionTrace[];
  readonly unreadByteLength: number;
  readonly stalledReader: boolean;
  readonly diagnosticsPublished: number;
  readonly duplicateNotifications: number;
  readonly providerPresent: boolean;
  readonly cancelledRequests: number;
  readonly unavailableMetrics: readonly string[];
}

export const WSP1_UNAVAILABLE_METRICS = [
  "client_input_to_paint",
  "decode_apply_time",
  "process_tree_rss",
  "language_service_incremental_rss",
] as const;

const BLOCK_MS = 1;

export function firstBlockedServerStage(stamps: readonly StageStamp[]): ProtocolStage | null {
  if (stamps.length === 0) return null;
  const latest = new Map<ProtocolStage, number>();
  for (const stamp of stamps) latest.set(stamp.stage, stamp.atMs);
  const ordered = SERVER_STAGE_ORDER.filter((stage) => latest.has(stage)).map((stage) => ({
    stage,
    atMs: latest.get(stage)!,
  }));
  const hasEnqueued = latest.has("outbound_enqueued");
  const hasWritten = latest.has("outbound_written");
  if (hasEnqueued && !hasWritten) return "outbound_enqueued";

  let worst: { stage: ProtocolStage; gap: number } | null = null;
  for (let i = 0; i < ordered.length - 1; i++) {
    const gap = ordered[i + 1].atMs - ordered[i].atMs;
    if (gap < BLOCK_MS) continue;
    if (!worst || gap > worst.gap) worst = { stage: ordered[i].stage, gap };
  }
  return worst?.stage ?? null;
}

export function combineInteractionTrace(
  method: string,
  requestEpoch: number,
  sourceEpoch: number | null,
  serverStamps: readonly StageStamp[],
  wire: readonly ProtocolWireEvent[],
): InteractionTrace {
  const stamps = [...serverStamps];
  const matching = wire.filter((event) => event.requestEpoch === requestEpoch);
  const response = matching.find((event) => event.kind === "response");
  const inbound = matching.find((event) => event.direction === "server_to_client");
  if (inbound && inbound.decodedMs != null && !stamps.some((s) => s.stage === "outbound_written")) {
    stamps.push({
      stage: "outbound_written",
      atMs: inbound.decodedMs,
      byteLength: inbound.byteLength,
    });
  }
  return {
    requestEpoch,
    sourceEpoch,
    method,
    status: traceTerminalStatus(response),
    stamps,
    firstBlockedStage: firstBlockedServerStage(stamps),
  };
}

/** A request's terminal state, read off its observed response (if any yet). */
function traceTerminalStatus(response: ProtocolWireEvent | undefined): TraceStatus {
  if (!response) return "pending";
  if (response.errorCode === REQUEST_CANCELLED_CODE) return "cancelled";
  if (response.errorCode !== undefined) return "failed";
  return "complete";
}

export type ThroughputKind = "throughput" | "query" | "indeterminate";

/**
 * Discriminate a payload/rate-only change from expensive query work.
 * Query work lengthens `provider_work`; payload/rate lengthens serialize or outbound.
 */
export function discriminateThroughputVsQuery(
  baseline: InteractionTrace,
  candidate: InteractionTrace,
): ThroughputKind {
  const dwell = (trace: InteractionTrace, stage: ProtocolStage): number => {
    const stamps = SERVER_STAGE_ORDER.map((name) =>
      trace.stamps.find((stamp) => stamp.stage === name),
    );
    const index = SERVER_STAGE_ORDER.indexOf(stage);
    const start = stamps[index];
    const next = stamps.slice(index + 1).find(Boolean);
    if (!start || !next) return 0;
    return next.atMs - start.atMs;
  };
  const providerDelta = dwell(candidate, "provider_work") - dwell(baseline, "provider_work");
  const serializeDelta = dwell(candidate, "serialize") - dwell(baseline, "serialize");
  const outboundDelta =
    dwell(candidate, "outbound_enqueued") - dwell(baseline, "outbound_enqueued");
  const throughputDelta = serializeDelta + outboundDelta;
  if (providerDelta > throughputDelta + 2 && providerDelta > 2) return "query";
  if (throughputDelta > providerDelta + 2 && throughputDelta > 2) return "throughput";
  return "indeterminate";
}

export interface ReplayExpectation {
  readonly requireDiagnostics?: boolean;
  readonly requireProvider?: boolean;
}

export type ReplayRejectReason =
  | "dropped-diagnostics"
  | "missing-provider"
  | "stalled-reader"
  | null;

export function rejectReplay(
  replay: Pick<RecordedLspReplay, "diagnosticsPublished" | "providerPresent" | "completenessState">,
  expectation: ReplayExpectation,
): ReplayRejectReason {
  if (expectation.requireDiagnostics && replay.diagnosticsPublished <= 0) {
    return "dropped-diagnostics";
  }
  if (expectation.requireProvider && !replay.providerPresent) {
    return "missing-provider";
  }
  return null;
}

export function emptyReplay(partial: Partial<RecordedLspReplay> = {}): RecordedLspReplay {
  return {
    schema: "recorded-lsp-replay.v1",
    host: "lsp-test-client",
    engineVersion: null,
    completenessState: "pending",
    wire: [],
    traces: [],
    unreadByteLength: 0,
    stalledReader: false,
    diagnosticsPublished: 0,
    duplicateNotifications: 0,
    providerPresent: false,
    cancelledRequests: 0,
    unavailableMetrics: [...WSP1_UNAVAILABLE_METRICS],
    ...partial,
  };
}
