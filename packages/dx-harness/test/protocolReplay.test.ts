import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import {
  buildReplayFromWire,
  controlsToFakeServerEnv,
  rejectReplay,
  runProtocolReplay,
} from "../src/protocol-replay/index.js";
import type { ProtocolWireEvent } from "@verter/lsp-test-client";

const HERE = dirname(fileURLToPath(import.meta.url));
const FIXTURE = join(HERE, "..", "..", "lsp-test-client", "test", "fixtures", "fakeLspServer.mjs");

describe("protocol replay controls", () => {
  it("maps slow-reader, storm, duplicate and drop controls onto fake-server env", () => {
    const env = controlsToFakeServerEnv({
      slowReader: true,
      dependencyStorm: true,
      duplicateNotification: true,
      dropDiagnostics: true,
      missingProvider: true,
      burstCount: 12,
    });
    expect(env.FAKE_SERVER_COMPLETE_STDERR).toBe("1");
    expect(env.FAKE_BURST_COUNT).toBe("12");
    expect(env.FAKE_DUPLICATE_DIAGNOSTICS).toBe("1");
    expect(env.FAKE_DROP_DIAGNOSTICS).toBe("1");
    expect(env.FAKE_PROVIDER_STARTED).toBeUndefined();
  });
});

describe("WSP1-AC1 live stalled reader", () => {
  it("reports outbound_enqueued as the first blocked server stage", async () => {
    const verdict = await runProtocolReplay({
      command: process.execPath,
      args: [FIXTURE],
      controls: { slowReader: true },
      timeoutMs: 8_000,
    });
    expect(verdict.replay.stalledReader).toBe(true);
    expect(verdict.replay.unreadByteLength).toBeGreaterThan(0);
    expect(verdict.firstBlockedServerStage).toBe("outbound_enqueued");
    expect(verdict.replay.unavailableMetrics).toContain("client_input_to_paint");
  });
});

describe("WSP1-AC2 live payload vs query delay", () => {
  it("grows response bytes when only payload size increases, and dwell when only query delay increases", async () => {
    const baseline = await runProtocolReplay({
      command: process.execPath,
      args: [FIXTURE],
      timeoutMs: 8_000,
    });
    const fat = await runProtocolReplay({
      command: process.execPath,
      args: [FIXTURE],
      controls: { payloadBytes: 20_000 },
      timeoutMs: 8_000,
    });
    const slow = await runProtocolReplay({
      command: process.execPath,
      args: [FIXTURE],
      controls: { queryDelayMs: 40 },
      timeoutMs: 8_000,
    });
    const responseBytes = (verdict: typeof baseline) =>
      Math.max(
        0,
        ...verdict.replay.wire
          .filter((event) => event.kind === "response")
          .map((event) => event.byteLength),
      );
    expect(responseBytes(fat)).toBeGreaterThan(responseBytes(baseline));
    const dwell = (verdict: typeof baseline) => {
      const request = [...verdict.replay.wire]
        .reverse()
        .find((event) => event.kind === "request" && event.method === "echo/method");
      const response = [...verdict.replay.wire]
        .reverse()
        .find((event) => event.kind === "response" && event.decodedMs != null);
      if (!request || !response || response.decodedMs == null) return 0;
      return response.decodedMs - request.encodeCompletedMs;
    };
    expect(dwell(slow)).toBeGreaterThan(dwell(baseline) + 20);
  });
});

describe("WSP1-AC3 live reject", () => {
  it("rejects dropped diagnostics", async () => {
    const verdict = await runProtocolReplay({
      command: process.execPath,
      args: [FIXTURE],
      controls: { dropDiagnostics: true, missingProvider: false },
      expectation: { requireDiagnostics: true, requireProvider: true },
      timeoutMs: 8_000,
    });
    expect(verdict.rejected).toBe("dropped-diagnostics");
  });

  it("rejects a missing provider", async () => {
    const verdict = await runProtocolReplay({
      command: process.execPath,
      args: [FIXTURE],
      controls: { missingProvider: true, dropDiagnostics: false },
      expectation: { requireDiagnostics: true, requireProvider: true },
      timeoutMs: 8_000,
    });
    expect(verdict.rejected).toBe("missing-provider");
  });
});

describe("buildReplayFromWire", () => {
  it("counts duplicate diagnostic notifications", () => {
    const wire: ProtocolWireEvent[] = [
      {
        requestEpoch: 1,
        method: "textDocument/publishDiagnostics",
        direction: "server_to_client",
        byteLength: 40,
        encodeStartedMs: 1,
        encodeCompletedMs: 1,
        queuedMs: 1,
        decodedMs: 1,
        completedMs: 1,
        kind: "notification",
      },
      {
        requestEpoch: 2,
        method: "textDocument/publishDiagnostics",
        direction: "server_to_client",
        byteLength: 40,
        encodeStartedMs: 2,
        encodeCompletedMs: 2,
        queuedMs: 2,
        decodedMs: 2,
        completedMs: 2,
        kind: "notification",
      },
    ];
    const replay = buildReplayFromWire(wire);
    expect(replay.diagnosticsPublished).toBe(2);
    expect(replay.duplicateNotifications).toBe(1);
    expect(rejectReplay(replay, { requireDiagnostics: true })).toBeNull();
  });
});
