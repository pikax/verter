import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { afterEach, describe, expect, it } from "vitest";

import {
  LspClient,
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
    client.resumeReads();
    const result = await pending;
    expect(result.echo).toEqual({ n: 1 });
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
