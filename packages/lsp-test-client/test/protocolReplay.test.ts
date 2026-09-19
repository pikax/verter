import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { afterEach, describe, expect, it } from "vitest";

import {
  LspClient,
  combineInteractionTrace,
  discriminateThroughputVsQuery,
  firstBlockedServerStage,
  rejectReplay,
  emptyReplay,
  type LspClientOptions,
  type ProtocolWireEvent,
  type StageStamp,
} from "../src/index.js";

const HERE = dirname(fileURLToPath(import.meta.url));
const FIXTURE = join(HERE, "fixtures", "fakeLspServer.mjs");

const live: LspClient[] = [];

function makeClient(env: NodeJS.ProcessEnv = {}, options: LspClientOptions = {}): LspClient {
  const client = new LspClient("fake", process.execPath, [FIXTURE], undefined, { env, ...options });
  live.push(client);
  return client;
}

afterEach(async () => {
  while (live.length) {
    const client = live.pop()!;
    await client.kill().catch(() => {});
  }
});

describe("WSP1-AC1 stalled client reader", () => {
  it("detects unread bytes after the server has already written", async () => {
    const client = makeClient(
      { FAKE_STAY_ALIVE: "1", FAKE_SERVER_COMPLETE_STDERR: "1" },
      { stallReads: true },
    );
    const pending = client.sendRequest("echo/method", { n: 1 }, 8_000);
    await client.stderr.waitForLine((line) => line.includes("server-complete"), 8_000);
    expect(client.unreadByteLength).toBeGreaterThan(0);
    // A stalled reader stalls the transport: the frame is parked in the paused
    // pipe, not drained into the application buffer and merely left undecoded.
    expect(client.pausedTransportByteLength).toBeGreaterThan(0);
    client.resumeReads();
    const result = await pending;
    expect(result.echo).toEqual({ n: 1 });
  });

  it("emits server-complete only after the response bytes are written, even for a large frame", async () => {
    const payloadBytes = 4_000_000;
    const client = makeClient(
      {
        FAKE_STAY_ALIVE: "1",
        FAKE_SERVER_COMPLETE_STDERR: "1",
        FAKE_PAYLOAD_BYTES: String(payloadBytes),
      },
      { stallReads: true },
    );
    const pending = client.sendRequest("echo/method", { n: 2 }, 20_000);
    // A frame larger than the pipe cannot finish writing while the reader is
    // paused, so the marker must stay absent until the client drains it.
    let markerSeen = false;
    const marker = client.stderr
      .waitForLine((line) => line.includes("server-complete"), 20_000)
      .then(() => {
        markerSeen = true;
      });
    await new Promise((resolve) => setTimeout(resolve, 400));
    expect(client.unreadByteLength).toBeGreaterThan(0);
    expect(markerSeen).toBe(false);
    client.resumeReads();
    const result = await pending;
    await marker;
    expect(result.echo).toEqual({ n: 2 });
    expect(String(result.pad)).toHaveLength(payloadBytes);
  });

  it("clears a cancelled id once its terminal response is written", async () => {
    // Drive the fixture over raw frames so a request id can be reused: the
    // client itself never reuses ids, but a long-lived fixture must not keep
    // reporting an id cancelled after its terminal response went out.
    const child = spawn(process.execPath, [FIXTURE], {
      stdio: ["pipe", "pipe", "pipe"],
      env: { ...process.env, FAKE_STAY_ALIVE: "1", FAKE_QUERY_DELAY_MS: "200" },
    });
    const responses: Array<{ id: number; error?: { code: number }; result?: unknown }> = [];
    let buffered = Buffer.alloc(0);
    child.stdout.on("data", (chunk: Buffer) => {
      buffered = Buffer.concat([buffered, chunk]);
      for (;;) {
        const headerEnd = buffered.indexOf("\r\n\r\n");
        if (headerEnd < 0) break;
        const header = buffered.subarray(0, headerEnd).toString();
        const length = Number(/Content-Length: (\d+)/.exec(header)![1]);
        const bodyStart = headerEnd + 4;
        if (buffered.length < bodyStart + length) break;
        const message = JSON.parse(buffered.subarray(bodyStart, bodyStart + length).toString());
        buffered = buffered.subarray(bodyStart + length);
        if (message.id !== undefined && message.method === undefined) responses.push(message);
      }
    });
    const send = (payload: unknown) => {
      const body = Buffer.from(JSON.stringify(payload));
      const header = Buffer.from(`Content-Length: ${body.length}\r\n\r\n`);
      child.stdin.write(Buffer.concat([header, body]));
    };
    const responseCount = (count: number) =>
      new Promise<void>((resolve, reject) => {
        const deadline = Date.now() + 8_000;
        const tick = () => {
          if (responses.length >= count) resolve();
          else if (Date.now() > deadline) reject(new Error("timed out waiting for responses"));
          else setTimeout(tick, 10);
        };
        tick();
      });
    try {
      send({ jsonrpc: "2.0", id: 1, method: "echo/method", params: { n: 1 } });
      send({ jsonrpc: "2.0", method: "$/cancelRequest", params: { id: 1 } });
      await responseCount(1);
      expect(responses[0]!.error?.code).toBe(-32800);
      send({ jsonrpc: "2.0", id: 1, method: "echo/method", params: { n: 2 } });
      await responseCount(2);
      expect(responses[1]!.error).toBeUndefined();
      expect((responses[1]!.result as { echo: unknown }).echo).toEqual({ n: 2 });
    } finally {
      child.kill();
    }
  });
});

describe("wire request epochs", () => {
  it("correlate each response with its request by JSON-RPC id", async () => {
    const wire: ProtocolWireEvent[] = [];
    const client = makeClient(
      { FAKE_STAY_ALIVE: "1" },
      { onWireEvent: (event) => wire.push(event) },
    );
    await client.initialize({ processId: null, rootUri: null, capabilities: {} }, 8_000);
    await client.sendRequest("echo/method", { n: 1 }, 8_000);
    await client.sendRequest("echo/method", { n: 2 }, 8_000);

    const requests = wire.filter(
      (event) => event.kind === "request" && event.method === "echo/method",
    );
    const responses = wire.filter((event) => event.kind === "response");
    expect(requests).toHaveLength(2);
    // initialize + the two echo round-trips all resolved.
    expect(responses).toHaveLength(3);
    // Every request shares its epoch with exactly one response, so a combined
    // timeline joins the pair instead of splitting into per-message traces.
    for (const request of requests) {
      expect(
        wire.filter(
          (event) => event.kind === "response" && event.requestEpoch === request.requestEpoch,
        ),
      ).toHaveLength(1);
    }
    // Distinct requests keep distinct epochs.
    expect(new Set(requests.map((event) => event.requestEpoch)).size).toBe(2);
  });
});

describe("firstBlockedServerStage", () => {
  it("treats a missing outbound_written as a stalled outbound queue", () => {
    const stamps: StageStamp[] = [
      { stage: "request_received", atMs: 1 },
      { stage: "provider_work", atMs: 1.1 },
      { stage: "serialize", atMs: 1.2, byteLength: 32 },
      { stage: "outbound_enqueued", atMs: 1.3, byteLength: 32 },
      { stage: "complete", atMs: 1.4 },
    ];
    expect(firstBlockedServerStage(stamps)).toBe("outbound_enqueued");
  });
});

describe("WSP1-AC2 throughput vs query", () => {
  it("labels payload-only growth as throughput, not query work", () => {
    const baseline = {
      requestEpoch: 1,
      sourceEpoch: null,
      method: "echo/method",
      status: "complete" as const,
      stamps: [
        { stage: "request_received" as const, atMs: 0 },
        { stage: "provider_work" as const, atMs: 0.1 },
        { stage: "serialize" as const, atMs: 0.2, byteLength: 16 },
        { stage: "outbound_enqueued" as const, atMs: 0.3, byteLength: 16 },
        { stage: "outbound_written" as const, atMs: 0.4, byteLength: 16 },
        { stage: "complete" as const, atMs: 0.5 },
      ],
      firstBlockedStage: null,
    };
    const payload = {
      ...baseline,
      stamps: [
        { stage: "request_received" as const, atMs: 0 },
        { stage: "provider_work" as const, atMs: 0.1 },
        { stage: "serialize" as const, atMs: 0.2, byteLength: 200_000 },
        { stage: "outbound_enqueued" as const, atMs: 20, byteLength: 200_000 },
        { stage: "outbound_written" as const, atMs: 21, byteLength: 200_000 },
        { stage: "complete" as const, atMs: 21.2 },
      ],
    };
    const query = {
      ...baseline,
      stamps: [
        { stage: "request_received" as const, atMs: 0 },
        { stage: "provider_work" as const, atMs: 0.1 },
        { stage: "serialize" as const, atMs: 25, byteLength: 16 },
        { stage: "outbound_enqueued" as const, atMs: 25.1, byteLength: 16 },
        { stage: "outbound_written" as const, atMs: 25.2, byteLength: 16 },
        { stage: "complete" as const, atMs: 25.3 },
      ],
    };
    expect(discriminateThroughputVsQuery(baseline, payload)).toBe("throughput");
    expect(discriminateThroughputVsQuery(baseline, query)).toBe("query");
  });
});

describe("combineInteractionTrace terminal status", () => {
  const requestEvent = (epoch: number): ProtocolWireEvent => ({
    requestEpoch: epoch,
    method: "echo/method",
    direction: "client_to_server",
    byteLength: 40,
    encodeStartedMs: 1,
    encodeCompletedMs: 1,
    queuedMs: 1,
    decodedMs: 1,
    completedMs: null,
    kind: "request",
  });
  const responseEvent = (epoch: number, errorCode?: number): ProtocolWireEvent => ({
    requestEpoch: epoch,
    method: "(response)",
    direction: "server_to_client",
    byteLength: 60,
    encodeStartedMs: 5,
    encodeCompletedMs: 5,
    queuedMs: 5,
    decodedMs: 5,
    completedMs: errorCode === undefined ? 5 : null,
    kind: "response",
    ...(errorCode !== undefined ? { errorCode } : {}),
  });
  const stamps: StageStamp[] = [
    { stage: "request_received", atMs: 1 },
    { stage: "admitted", atMs: 1 },
    { stage: "provider_work", atMs: 1 },
    { stage: "serialize", atMs: 5, byteLength: 60 },
    { stage: "outbound_enqueued", atMs: 5, byteLength: 60 },
  ];

  it("marks a successful round trip complete", () => {
    const trace = combineInteractionTrace("echo/method", 1, null, stamps, [
      requestEvent(1),
      responseEvent(1),
    ]);
    expect(trace.status).toBe("complete");
  });

  it("marks a RequestCancelled response cancelled, not complete", () => {
    const trace = combineInteractionTrace("echo/method", 1, null, stamps, [
      requestEvent(1),
      responseEvent(1, -32800),
    ]);
    expect(trace.status).toBe("cancelled");
  });

  it("marks any other error response failed", () => {
    const trace = combineInteractionTrace("echo/method", 1, null, stamps, [
      requestEvent(1),
      responseEvent(1, -32603),
    ]);
    expect(trace.status).toBe("failed");
  });

  it("leaves a request without any response pending", () => {
    const trace = combineInteractionTrace("echo/method", 1, null, stamps, [requestEvent(1)]);
    expect(trace.status).toBe("pending");
  });
});

describe("WSP1-AC3 reject dropped diagnostics or missing provider", () => {
  it("rejects a run that published no diagnostics when they were required", () => {
    const replay = emptyReplay({ diagnosticsPublished: 0, providerPresent: true });
    expect(rejectReplay(replay, { requireDiagnostics: true, requireProvider: true })).toBe(
      "dropped-diagnostics",
    );
  });

  it("rejects a run with no provider when a provider was required", () => {
    const replay = emptyReplay({ diagnosticsPublished: 2, providerPresent: false });
    expect(rejectReplay(replay, { requireDiagnostics: true, requireProvider: true })).toBe(
      "missing-provider",
    );
  });

  it("accepts a complete run with diagnostics and a provider", () => {
    const replay = emptyReplay({ diagnosticsPublished: 1, providerPresent: true });
    expect(rejectReplay(replay, { requireDiagnostics: true, requireProvider: true })).toBeNull();
  });
});
