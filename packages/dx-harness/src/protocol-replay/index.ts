/**
 * WSP1 protocol replay harness: slow-reader, cancellation, duplicate
 * notification, dependency-storm and long-churn controls over
 * `@verter/lsp-test-client`. Client apply/paint are WSP1L.
 */

import {
  LspClient,
  REQUEST_CANCELLED_CODE,
  combineInteractionTrace,
  discriminateThroughputVsQuery,
  emptyReplay,
  firstBlockedServerStage,
  rejectReplay,
  type InteractionTrace,
  type ProtocolStage,
  type ProtocolWireEvent,
  type RecordedLspReplay,
  type ReplayExpectation,
  type ReplayRejectReason,
  type StageStamp,
  type ThroughputKind,
  type TraceStatus,
} from "@verter/lsp-test-client";

export {
  combineInteractionTrace,
  discriminateThroughputVsQuery,
  firstBlockedServerStage,
  rejectReplay,
  type InteractionTrace,
  type ProtocolStage,
  type ProtocolWireEvent,
  type RecordedLspReplay,
  type ReplayExpectation,
  type ReplayRejectReason,
  type ThroughputKind,
  type TraceStatus,
};

export interface ProtocolReplayControls {
  readonly slowReader?: boolean;
  readonly cancellation?: boolean;
  readonly duplicateNotification?: boolean;
  readonly dependencyStorm?: boolean;
  readonly longChurn?: boolean;
  readonly queryDelayMs?: number;
  readonly payloadBytes?: number;
  readonly burstCount?: number;
  readonly dropDiagnostics?: boolean;
  readonly missingProvider?: boolean;
}

export function controlsToFakeServerEnv(controls: ProtocolReplayControls): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = { FAKE_STAY_ALIVE: "1" };
  if (controls.slowReader) env.FAKE_SERVER_COMPLETE_STDERR = "1";
  // Cancellation needs a request still in flight when $/cancelRequest arrives;
  // default to a short delay unless the caller pinned one.
  if (controls.cancellation && !controls.queryDelayMs) env.FAKE_QUERY_DELAY_MS = "200";
  if (controls.queryDelayMs) env.FAKE_QUERY_DELAY_MS = String(controls.queryDelayMs);
  if (controls.payloadBytes) env.FAKE_PAYLOAD_BYTES = String(controls.payloadBytes);
  if (controls.dependencyStorm) {
    env.FAKE_BURST_ON = "$/test/storm";
    env.FAKE_BURST_COUNT = String(controls.burstCount ?? 32);
  }
  if (controls.duplicateNotification) {
    env.FAKE_DIAGNOSTICS_ON = "$/test/diagnostics";
    env.FAKE_DUPLICATE_DIAGNOSTICS = "1";
  } else if (!controls.dropDiagnostics) {
    env.FAKE_DIAGNOSTICS_ON = "$/test/diagnostics";
  }
  if (controls.dropDiagnostics) {
    env.FAKE_DIAGNOSTICS_ON = "$/test/diagnostics";
    env.FAKE_DROP_DIAGNOSTICS = "1";
  }
  if (!controls.missingProvider) env.FAKE_PROVIDER_STARTED = "1";
  if (controls.longChurn) {
    env.FAKE_BURST_ON = env.FAKE_BURST_ON ?? "$/test/churn";
    env.FAKE_BURST_COUNT = String(controls.burstCount ?? 64);
  }
  return env;
}

export interface RunProtocolReplayOptions {
  readonly command: string;
  readonly args?: readonly string[];
  readonly cwd?: string;
  readonly env?: NodeJS.ProcessEnv;
  readonly controls?: ProtocolReplayControls;
  readonly expectation?: ReplayExpectation;
  readonly timeoutMs?: number;
}

export interface ProtocolReplayVerdict {
  readonly replay: RecordedLspReplay;
  readonly rejected: ReplayRejectReason;
  readonly firstBlockedServerStage: ProtocolStage | null;
}

function countDuplicates(methods: readonly string[]): number {
  const seen = new Map<string, number>();
  let dupes = 0;
  for (const method of methods) {
    const next = (seen.get(method) ?? 0) + 1;
    seen.set(method, next);
    if (next > 1 && method === "textDocument/publishDiagnostics") dupes += 1;
  }
  return dupes;
}

function countObservedCancellations(wire: readonly ProtocolWireEvent[]): number {
  return wire.filter(
    (event) => event.kind === "response" && event.errorCode === REQUEST_CANCELLED_CODE,
  ).length;
}

/**
 * The run's terminal state, derived from the observed wire: a cancelled or
 * failed request keeps the whole replay distinct from complete (the run-level
 * mirror of the per-trace TraceStatus terminals).
 */
/**
 * The replay's terminal state is derived from request terminal evidence: an
 * empty wire is `complete-empty`, request epochs without a response are
 * `pending` (all of them) or `partial` (some of them), and an observed
 * cancellation or failure response wins over both. A wire that merely lacks
 * error responses is never read as complete.
 */
function observedCompletenessState(
  wire: readonly ProtocolWireEvent[],
): RecordedLspReplay["completenessState"] {
  if (wire.length === 0) return "complete-empty";
  let cancelled = false;
  let failed = false;
  const requestEpochs = new Set<number>();
  const answeredEpochs = new Set<number>();
  for (const event of wire) {
    if (event.kind === "request") requestEpochs.add(event.requestEpoch);
    if (event.kind !== "response") continue;
    answeredEpochs.add(event.requestEpoch);
    if (event.errorCode === undefined) continue;
    if (event.errorCode === REQUEST_CANCELLED_CODE) cancelled = true;
    else failed = true;
  }
  if (cancelled) return "cancelled";
  if (failed) return "failed";
  const unanswered = [...requestEpochs].filter((epoch) => !answeredEpochs.has(epoch)).length;
  if (unanswered > 0) return unanswered === requestEpochs.size ? "pending" : "partial";
  return "complete";
}

export function buildReplayFromWire(
  wire: readonly ProtocolWireEvent[],
  extra: Partial<RecordedLspReplay> = {},
): RecordedLspReplay {
  const inbound = wire.filter((event) => event.direction === "server_to_client");
  const methods = inbound.map((event) => event.method);
  const traces: InteractionTrace[] = [];
  const byEpoch = new Map<number, ProtocolWireEvent[]>();
  for (const event of wire) {
    const list = byEpoch.get(event.requestEpoch) ?? [];
    list.push(event);
    byEpoch.set(event.requestEpoch, list);
  }
  for (const [epoch, events] of byEpoch) {
    const request = events.find((event) => event.kind === "request");
    const response = events.find((event) => event.kind === "response");
    if (!request && !response) continue;
    const method = request?.method ?? response?.method ?? "(unknown)";
    const stamps: StageStamp[] = [];
    if (request) {
      stamps.push({ stage: "request_received", atMs: request.encodeStartedMs });
      stamps.push({ stage: "admitted", atMs: request.encodeCompletedMs });
      stamps.push({ stage: "provider_work", atMs: request.encodeCompletedMs });
    }
    if (response) {
      stamps.push({
        stage: "serialize",
        atMs: response.encodeStartedMs,
        byteLength: response.byteLength,
      });
      stamps.push({
        stage: "outbound_enqueued",
        atMs: response.encodeStartedMs,
        byteLength: response.byteLength,
      });
      if (response.decodedMs != null) {
        stamps.push({
          stage: "outbound_written",
          atMs: response.decodedMs,
          byteLength: response.byteLength,
        });
      }
      // An error response closed the request without completing it: cancelled
      // and failed traces never carry a complete stamp.
      if (response.completedMs != null && response.errorCode === undefined) {
        stamps.push({ stage: "complete", atMs: response.completedMs });
      }
    }
    traces.push(combineInteractionTrace(method, epoch, null, stamps, events));
  }
  return emptyReplay({
    completenessState: extra.completenessState ?? observedCompletenessState(wire),
    wire,
    traces,
    diagnosticsPublished: methods.filter((m) => m === "textDocument/publishDiagnostics").length,
    duplicateNotifications: countDuplicates(methods),
    providerPresent: methods.includes("$/verter/typeProviderStarted"),
    cancelledRequests: countObservedCancellations(wire),
    ...extra,
  });
}

export async function runProtocolReplay(
  options: RunProtocolReplayOptions,
): Promise<ProtocolReplayVerdict> {
  const controls = options.controls ?? {};
  const wire: ProtocolWireEvent[] = [];
  const env = {
    ...controlsToFakeServerEnv(controls),
    ...options.env,
  };
  const client = new LspClient(
    "wsp1-replay",
    options.command,
    [...(options.args ?? [])],
    options.cwd,
    {
      env,
      stallReads: controls.slowReader === true,
      onWireEvent: (event) => {
        wire.push(event);
      },
    },
  );
  const timeout = options.timeoutMs ?? 8_000;
  try {
    if (controls.slowReader) {
      const waiting = client.stderr.waitForLine(
        (line) => line.includes("server-complete"),
        timeout,
      );
      const pending = client.sendRequest("echo/method", { n: 1 }, timeout);
      await waiting;
      const unread = client.unreadByteLength;
      if (unread <= 0) {
        throw new Error("stalled reader observed no unread bytes after server-complete");
      }
      client.resumeReads();
      await pending;
      const replay = buildReplayFromWire(wire, {
        unreadByteLength: unread,
        stalledReader: true,
      });
      // Server finished; the unread buffer is the blocked outbound/transport stage.
      return {
        replay,
        rejected: rejectReplay(replay, options.expectation ?? {}),
        firstBlockedServerStage: "outbound_enqueued",
      };
    }

    await client.initialize(
      {
        processId: null,
        rootUri: null,
        capabilities: {},
        initializationOptions: { interactionTrace: { enabled: true } },
      },
      timeout,
    );

    if (controls.cancellation) {
      // Live control: put a request in flight, cancel it by its real JSON-RPC
      // id, and let the server answer. The recorded count comes from the
      // observed wire below, never from this flag.
      const requestId = client.peekNextRequestId();
      const outcome = client.sendRequest("echo/method", { n: 1 }, timeout).then(
        () => undefined,
        (err: Error) => err,
      );
      client.sendNotification("$/cancelRequest", { id: requestId });
      await outcome;
    }
    if (controls.dependencyStorm || controls.longChurn) {
      client.sendNotification(controls.longChurn ? "$/test/churn" : "$/test/storm", {});
      await client.waitForNotification("$/test/burst", timeout);
    }
    if (controls.duplicateNotification || !controls.dropDiagnostics) {
      if (controls.dropDiagnostics) {
        client.sendNotification("$/test/diagnostics", {});
      } else {
        const waiting = client.waitForNotification("textDocument/publishDiagnostics", timeout);
        client.sendNotification("$/test/diagnostics", {});
        await waiting;
      }
    } else if (controls.dropDiagnostics) {
      client.sendNotification("$/test/diagnostics", {});
    }

    await client.sendRequest("echo/method", { n: 1 }, timeout);

    const replay = buildReplayFromWire(wire, {
      unreadByteLength: client.unreadByteLength,
      stalledReader: false,
    });
    const blocked =
      replay.traces.map((trace) => trace.firstBlockedStage).find((stage) => stage != null) ??
      firstBlockedServerStage(replay.traces[0]?.stamps ?? []);
    return {
      replay,
      rejected: rejectReplay(replay, options.expectation ?? {}),
      firstBlockedServerStage: blocked,
    };
  } finally {
    await client.kill().catch(() => {});
  }
}
