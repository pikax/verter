import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { afterEach, describe, expect, it } from "vitest";

import { LspClient, type LspClientOptions } from "../src/index.js";

const HERE = dirname(fileURLToPath(import.meta.url));
const FIXTURE = join(HERE, "fixtures", "fakeLspServer.mjs");
const SOH = String.fromCharCode(1);

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

describe("LspClient stderr capture", () => {
  it("buffers and exposes child stderr instead of swallowing it", async () => {
    const client = makeClient({
      FAKE_STDERR_LINES: ["boot-line-1", "ready-marker"].join(SOH),
      FAKE_STAY_ALIVE: "1",
    });

    const matched = await client.stderr.waitForLine((l) => l.includes("ready-marker"), 5000);
    expect(matched).toContain("ready-marker");

    const text = client.stderr.text();
    expect(text).toContain("boot-line-1");
    expect(text).toContain("ready-marker");
    expect(client.stderr.lines()).toContain("boot-line-1");
    expect(client.stderr.byteLength).toBeGreaterThan(0);

    // Negative: stderr is captured exactly — content never written must be absent.
    expect(client.stderr.lines()).not.toContain("never-emitted-line");
    expect(text).not.toContain("never-emitted-line");
  });
});

describe("LspClient request transport", () => {
  it("rejects a request that receives no response after the timeout", async () => {
    const client = makeClient({ FAKE_NO_RESPONSE: "1", FAKE_STAY_ALIVE: "1" });
    await expect(client.sendRequest("slow/method", { x: 1 }, 150)).rejects.toThrow(
      /timed out after 150ms/,
    );
  });

  it("round-trips a request whose params contain multi-byte UTF-8 (byte-accurate framing)", async () => {
    const client = makeClient();
    // A naive char-indexed framer (Content-Length is BYTES) would mis-slice this
    // body and corrupt the stream; byte-accurate framing round-trips it intact.
    const payload = { text: "日本語 café 😀 — multibyte", n: 3, nested: { a: ["é", "🚀"] } };
    const result = await client.sendRequest<{ echo: unknown }>("echo/method", payload, 5000);
    expect(result).toEqual({ echo: payload });
  });
});

describe("LspClient notification waiting", () => {
  it("resolves on a matching notification and ignores non-matching ones", async () => {
    const client = makeClient({ FAKE_EMIT_ON: "$/test/go" });
    const waiting = client.waitForNotification(
      "$/test/note",
      5000,
      (p: any) => p?.kind === "match",
    );
    client.sendNotification("$/test/go", {});
    // The server emits {kind:"nomatch",value:1} first, then {kind:"match",value:42};
    // resolving with value 42 proves the predicate filtered the earlier one out.
    const params = await waiting;
    expect(params).toEqual({ kind: "match", value: 42 });
  });

  it("does not resolve when no notification matches the predicate", async () => {
    const client = makeClient({ FAKE_EMIT_ON: "$/test/go" });
    const waiting = client.waitForNotification(
      "$/test/note",
      250,
      (p: any) => p?.kind === "never-sent",
    );
    client.sendNotification("$/test/go", {});
    await expect(waiting).rejects.toThrow(/timed out/);
  });
});

describe("LspClient notification observation", () => {
  it("invokes onAnyNotification for every inbound notification, unfiltered", async () => {
    const seen: Array<{ method: string; params: any }> = [];
    const client = makeClient(
      { FAKE_EMIT_ON: "$/test/go" },
      { onAnyNotification: (method, params) => seen.push({ method, params }) },
    );
    // The observer must catch BOTH emissions (the nomatch one AND the match one),
    // proving it is a true wildcard and not filtered like waitForNotification.
    const done = client.waitForNotification("$/test/note", 5000, (p: any) => p?.kind === "match");
    client.sendNotification("$/test/go", {});
    await done;

    expect(seen).toEqual([
      { method: "$/test/note", params: { kind: "nomatch", value: 1 } },
      { method: "$/test/note", params: { kind: "match", value: 42 } },
    ]);
  });

  it("does not invoke onAnyNotification for plain request responses", async () => {
    const seen: string[] = [];
    const client = makeClient({}, { onAnyNotification: (method) => seen.push(method) });
    // A request/response round-trip carries an id and is not a notification.
    await client.sendRequest("echo/method", { a: 1 }, 5000);
    expect(seen).not.toContain("echo/method");
    expect(seen).toEqual([]);
  });
});

describe("LspClient server→client request handling", () => {
  it("replies method-not-found (-32601) to an unhandled server-initiated request", async () => {
    const client = makeClient({
      FAKE_STAY_ALIVE: "1",
      FAKE_PROVOKE_ON: "$/test/provoke",
      FAKE_PROVOKE_METHOD: "$/server/unknownRequest",
    });
    // The server sends an unsolicited request; the client has no handler for it.
    // It MUST still answer (with -32601) so a real server is not left deadlocked
    // awaiting a reply that never comes.
    const replyWaiting = client.waitForNotification("$/test/clientReply", 5000);
    client.sendNotification("$/test/provoke", {});
    const reply = await replyWaiting;

    expect(reply.error).toBeTruthy();
    expect(reply.error.code).toBe(-32601);
    // Negative: the client did not silently drop the request (which would never
    // produce a reply) and did not answer with a bogus success result.
    expect(reply.result ?? null).toBeNull();
  });

  it("dispatches a registered server→client request handler instead of -32601", async () => {
    const client = makeClient({
      FAKE_STAY_ALIVE: "1",
      FAKE_PROVOKE_ON: "$/test/provoke",
      FAKE_PROVOKE_METHOD: "$/server/customReq",
    });
    client.onRequest("$/server/customReq", (params: any) => ({ handled: true, echo: params }));
    const replyWaiting = client.waitForNotification("$/test/clientReply", 5000);
    client.sendNotification("$/test/provoke", {});
    const reply = await replyWaiting;

    // The registered handler answers; the method-not-found fallback must NOT fire.
    expect(reply.result).toEqual({ handled: true, echo: { provoked: true } });
    expect(reply.error ?? null).toBeNull();
  });
});

describe("LspClient in-flight rejection on child exit", () => {
  it("rejects pending requests, notification waiters, and stderr waiters when the child exits", async () => {
    // FAKE_NO_RESPONSE makes the server ignore the request issued below, so it
    // stays outstanding (never answered) right up to the kill. That pins the
    // pendingRequests rejection arm specifically — without it the request would
    // be echoed back and resolve, leaving that arm of rejectInFlight untested.
    const client = makeClient({
      FAKE_STAY_ALIVE: "1",
      FAKE_STDERR_LINES: "boot",
      FAKE_NO_RESPONSE: "1",
    });
    await client.stderr.waitForLine((l) => l.includes("boot"), 5000);

    // All three waiters use a timeout far larger than this test's own timeout: if
    // a regression leaves any of them un-rejected on exit, the test fails as a
    // timeout (they never resolve and never reach their own deadline first).
    const reqWaiting = client.sendRequest("$/never/answered", { x: 1 }, 60_000);
    const noteWaiting = client.waitForNotification("$/never/arrives", 60_000);
    const lineWaiting = client.stderr.waitForLine((l) => l.includes("never-emitted"), 60_000);

    // Attach expectations before killing so the rejections are never unhandled.
    // Each must reject with the EXIT cause (/exited/), never the timeout message.
    const reqAssertion = expect(reqWaiting).rejects.toThrow(/exited/);
    const noteAssertion = expect(noteWaiting).rejects.toThrow(/exited/);
    const lineAssertion = expect(lineWaiting).rejects.toThrow(/exited/);
    // Negative: none rejects with the timeout message (that would mean it waited
    // out the deadline instead of failing fast on exit).
    const reqNotTimeout = expect(reqWaiting).rejects.not.toThrow(/timed out/);
    const noteNotTimeout = expect(noteWaiting).rejects.not.toThrow(/timed out/);
    const lineNotTimeout = expect(lineWaiting).rejects.not.toThrow(/timed out/);

    await client.kill();

    await reqAssertion;
    await noteAssertion;
    await lineAssertion;
    await reqNotTimeout;
    await noteNotTimeout;
    await lineNotTimeout;
  }, 10_000);
});

describe("LspClient process lifecycle", () => {
  it("kill terminates the child process cleanly", async () => {
    const client = makeClient({ FAKE_STAY_ALIVE: "1", FAKE_STDERR_LINES: "alive" });
    await client.stderr.waitForLine((l) => l.includes("alive"), 5000);
    expect(client.isAlive()).toBe(true);

    await client.kill();
    expect(client.isAlive()).toBe(false);
  });

  it("force-kills a child that ignores SIGTERM (hard-kill fallback)", async () => {
    // The fake server installs a no-op SIGTERM handler. On POSIX that overrides
    // the default terminate action, so a graceful SIGTERM is swallowed and only
    // the force path (SIGKILL to the process group after the grace period) can
    // reap it. On Windows `kill("SIGTERM")` maps to TerminateProcess, which the
    // handler cannot intercept, so the child is reaped immediately. Either way,
    // teardown must leave no live child behind.
    const client = makeClient({
      FAKE_IGNORE_SIGTERM: "1",
      FAKE_STAY_ALIVE: "1",
      FAKE_STDERR_LINES: "alive",
    });
    await client.stderr.waitForLine((l) => l.includes("alive"), 5000);
    expect(client.isAlive()).toBe(true);

    const start = performance.now();
    await client.kill();
    const elapsed = performance.now() - start;

    expect(client.isAlive()).toBe(false);
    if (process.platform !== "win32") {
      // POSIX: SIGTERM was swallowed, so only the post-grace force path could
      // have reaped it — a honoured SIGTERM would resolve well under the grace.
      expect(elapsed).toBeGreaterThanOrEqual(1500);
    }
  });

  it("kill resolves even when the child never spawned (no exit event)", async () => {
    // A non-existent command fails to spawn: the child emits 'error' (ENOENT)
    // and never an 'exit', and has no pid. kill() must still resolve as a last
    // resort instead of hanging forever waiting for an exit that cannot come.
    const client = new LspClient("ghost", "verter-nonexistent-binary-xyz", [], undefined, {
      onError: () => {},
    });
    live.push(client);
    await client.kill();
    expect(client.isAlive()).toBe(false);
    expect(client.spawnError).toBeTruthy();
  }, 8000);
});

describe("LspClient position-encoding negotiation", () => {
  it("advertises general.positionEncodings and adopts the server's chosen encoding", async () => {
    const client = makeClient(
      { FAKE_INIT_ENCODING: "utf-8" },
      { positionEncodings: ["utf-16", "utf-8"] },
    );
    // Default before the handshake completes.
    expect(client.positionEncoding).toBe("utf-16");

    const result: any = await client.initialize({ processId: process.pid, rootUri: null });

    // Advertised: the server echoes back the client's general.positionEncodings.
    expect(result.receivedPositionEncodings).toEqual(["utf-16", "utf-8"]);
    // Adopted: the client honours the server's chosen encoding.
    expect(client.positionEncoding).toBe("utf-8");
  });

  it("falls back to utf-16 when the server returns no positionEncoding", async () => {
    const client = makeClient({ FAKE_INIT_ENCODING: "none" });
    await client.initialize({ processId: process.pid, rootUri: null });
    expect(client.positionEncoding).toBe("utf-16");
  });

  it("injects general.positionEncodings without clobbering caller-provided capabilities", async () => {
    const client = makeClient({ FAKE_INIT_ENCODING: "utf-8" });
    const result: any = await client.initialize({
      processId: process.pid,
      rootUri: null,
      capabilities: {
        textDocument: { hover: { contentFormat: ["markdown"] } },
        general: { markdown: { parser: "marked" } },
      },
    });

    const caps = result.receivedCapabilities;
    expect(caps.general.positionEncodings).toEqual(["utf-16", "utf-8"]);
    // Pre-existing capability fields survive the merge.
    expect(caps.general.markdown).toEqual({ parser: "marked" });
    expect(caps.textDocument.hover.contentFormat).toEqual(["markdown"]);
  });

  it("converts byte offsets to LSP positions using the negotiated encoding", async () => {
    const client = makeClient({ FAKE_INIT_ENCODING: "utf-8" });
    await client.initialize({ processId: process.pid, rootUri: null });
    expect(client.positionEncoding).toBe("utf-8");

    const text = "ab café 😀 cd";
    // Byte offset 13 is just after the emoji on line 0.
    expect(client.byteOffsetToPosition(text, 13)).toEqual({ line: 0, character: 13 });
    expect(client.positionToByteOffset(text, { line: 0, character: 13 })).toBe(13);
  });
});
