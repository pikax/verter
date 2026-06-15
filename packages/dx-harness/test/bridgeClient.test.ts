import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { afterEach, describe, expect, it } from "vitest";

import {
  BridgeClient,
  NewlineFramer,
  decodeResponse,
  diagnosticsFrame,
  encodeRequest,
  helloFrame,
  openFrame,
  queryFrame,
  syncArtifactsFrame,
} from "../src/baseline/bridgeClient.js";

const HERE = dirname(fileURLToPath(import.meta.url));
const FAKE = join(HERE, "fixtures", "fakeBridge.mjs");

const live: BridgeClient[] = [];
afterEach(async () => {
  for (const c of live.splice(0)) await c.dispose();
});

function client(env: NodeJS.ProcessEnv = {}): BridgeClient {
  const c = new BridgeClient(process.execPath, {
    extraArgs: [FAKE],
    cwd: HERE,
    env: { ...process.env, ...env },
  });
  live.push(c);
  return c;
}

describe("request frames (exact wire shape)", () => {
  it("hello carries camelCase fields and a nested toolRoot", () => {
    const frame = helloFrame({
      workspaceRoot: "/ws",
      repoRoot: "/repo",
      provider: "tsserver",
      strictCi: true,
      toolRoot: { tsserverTsdk: "/tsdk", expectedTsserverJs: "/tsdk/tsserver.js" },
    });
    const line = encodeRequest(frame);
    expect(line.endsWith("\n")).toBe(true);
    const obj = JSON.parse(line);
    expect(obj.type).toBe("hello");
    expect(obj.strictCi).toBe(true);
    expect(obj.toolRoot.expectedTsserverJs).toBe("/tsdk/tsserver.js");
    // Negative: no snake_case leaks.
    expect(obj).not.toHaveProperty("strict_ci");
    expect(obj).not.toHaveProperty("workspace_root");
  });

  it("open files carry the role strings the bridge expects", () => {
    const frame = openFrame(
      [
        { path: "/A.vue.tsx", content: "x", role: "entry" },
        { path: "/A.vue.ts", content: "y", role: "api" },
        { path: "/vue.d.ts", content: "z", role: "support" },
      ],
      3,
    );
    expect(frame.files.map((f) => f.role)).toEqual(["entry", "api", "support"]);
    expect(frame.version).toBe(3);
  });

  it("query omits triggerCharacter and requiresSourceMap by default, includes them when set", () => {
    const bare = queryFrame({
      method: "hover",
      uri: "/A.vue",
      path: "/A.vue.tsx",
      offset: 10,
      version: 2,
    });
    expect(bare).not.toHaveProperty("triggerCharacter");
    expect(bare).not.toHaveProperty("requiresSourceMap");

    const full = queryFrame({
      method: "completion",
      uri: "/A.vue",
      path: "/A.vue.tsx",
      offset: 1,
      version: 2,
      triggerCharacter: ".",
      requiresSourceMap: true,
    });
    expect(full.triggerCharacter).toBe(".");
    expect(full.requiresSourceMap).toBe(true);
  });

  it("syncArtifacts omits absent map identity and empty twins, but keeps per-twin versions", () => {
    const minimal = syncArtifactsFrame({ uri: "/A.vue", version: 2, files: [] });
    expect(minimal).not.toHaveProperty("sourceMapIdentity");
    expect(minimal).not.toHaveProperty("changedPublicApiTwins");

    const full = syncArtifactsFrame({
      uri: "/Parent.vue",
      version: 5,
      files: [],
      sourceMapIdentity: "map-5",
      changedPublicApiTwins: [{ path: "/Child.vue.ts", version: 1 }],
    });
    expect(full.sourceMapIdentity).toBe("map-5");
    expect(full.changedPublicApiTwins).toEqual([{ path: "/Child.vue.ts", version: 1 }]);
    // The twin carries its OWN version, never the edited document's.
    expect(full.changedPublicApiTwins![0].version).not.toBe(full.version);
  });

  it("diagnostics omits requiresSourceMap by default", () => {
    const d = diagnosticsFrame({ uri: "/A.vue", path: "/A.vue.tsx", version: 2 });
    expect(d).not.toHaveProperty("requiresSourceMap");
    expect(d.type).toBe("diagnostics");
  });
});

describe("decodeResponse", () => {
  it("decodes a typed error frame with the snake_case kind", () => {
    const r = decodeResponse(
      '{"type":"error","kind":"baseline_artifact_stale","message":"stale","uri":"/A.vue","requestedVersion":4,"haveVersion":2}',
    );
    expect(r.type).toBe("error");
    if (r.type !== "error") throw new Error("expected error");
    expect(r.kind).toBe("baseline_artifact_stale");
    expect(r.haveVersion).toBe(2);
  });

  it("throws on a frame missing a type tag", () => {
    expect(() => decodeResponse('{"ok":true}')).toThrow();
    expect(() => decodeResponse("not json")).toThrow();
  });
});

describe("NewlineFramer", () => {
  it("yields complete lines and buffers a partial across chunks", () => {
    const f = new NewlineFramer();
    expect(f.push('{"a":1}\n{"b":2}\n')).toEqual(['{"a":1}', '{"b":2}']);
    // A partial line is retained until its newline arrives.
    expect(f.push('{"c":')).toEqual([]);
    expect(f.push("3}\n")).toEqual(['{"c":3}']);
  });

  it("skips blank lines and tolerates a trailing carriage return", () => {
    const f = new NewlineFramer();
    expect(f.push('\n{"a":1}\r\n\n')).toEqual(['{"a":1}']);
  });
});

describe("BridgeClient (hermetic fake bridge)", () => {
  it("handshakes and reports utf-8 byte-offset capabilities (never utf-16)", async () => {
    const c = client();
    const hello = await c.hello({
      workspaceRoot: "/ws",
      repoRoot: "/repo",
      provider: "tsgo",
      strictCi: false,
      toolRoot: { tsgoBin: "/bin/tsgo" },
    });
    expect(hello.type).toBe("hello");
    if (hello.type !== "hello") throw new Error("expected hello");
    expect(hello.capabilities?.positionEncoding).toBe("utf-8");
    expect(hello.capabilities?.positionEncoding).not.toBe("utf-16");
    expect(hello.baselineToolRootUsed).toBe("/bin/tsgo");
  });

  it("passes the byte offset through to a hover probe and counts the run", async () => {
    const c = client();
    await c.hello({
      workspaceRoot: "/ws",
      repoRoot: "/repo",
      provider: "tsgo",
      strictCi: false,
      toolRoot: { tsgoBin: "/bin/tsgo" },
    });
    const res = await c.query({
      method: "hover",
      uri: "/A.vue",
      path: "/A.vue.tsx",
      offset: 42,
      version: 1,
    });
    expect(res.type).toBe("query");
    if (res.type !== "query") throw new Error("expected query");
    if (res.result.kind !== "hover") throw new Error("expected hover");
    expect(res.result.hover?.contents).toBe("offset=42");

    const bye = await c.shutdown();
    expect(bye.type).toBe("shutdown");
    if (bye.type !== "shutdown") throw new Error("expected shutdown");
    expect(bye.baselineRan).toBe(1);
  });

  it("surfaces a stale refusal as a typed error frame, not a throw", async () => {
    const c = client();
    await c.hello({
      workspaceRoot: "/ws",
      repoRoot: "/repo",
      provider: "tsgo",
      strictCi: false,
      toolRoot: { tsgoBin: "/bin/tsgo" },
    });
    const res = await c.query({
      method: "hover",
      uri: "/A.vue",
      path: "/A.vue.tsx",
      offset: 0,
      version: 999,
    });
    expect(res.type).toBe("error");
    if (res.type !== "error") throw new Error("expected error");
    expect(res.kind).toBe("baseline_artifact_stale");
    // A refused probe is NOT counted as a run.
    const bye = await c.shutdown();
    if (bye.type !== "shutdown") throw new Error("expected shutdown");
    expect(bye.baselineRan).toBe(0);
  });

  it("correlates concurrent in-flight requests in FIFO order", async () => {
    const c = client();
    await c.hello({
      workspaceRoot: "/ws",
      repoRoot: "/repo",
      provider: "tsgo",
      strictCi: false,
      toolRoot: { tsgoBin: "/bin/tsgo" },
    });
    const [a, b] = await Promise.all([
      c.query({ method: "hover", uri: "/A.vue", path: "/A.vue.tsx", offset: 1, version: 1 }),
      c.query({ method: "hover", uri: "/A.vue", path: "/A.vue.tsx", offset: 2, version: 1 }),
    ]);
    if (a.type !== "query" || a.result.kind !== "hover") throw new Error("a");
    if (b.type !== "query" || b.result.kind !== "hover") throw new Error("b");
    expect(a.result.hover?.contents).toBe("offset=1");
    expect(b.result.hover?.contents).toBe("offset=2");
  });

  it("fails the session on a request timeout so a late reply cannot corrupt a later request", async () => {
    const c = new BridgeClient(process.execPath, {
      extraArgs: [FAKE],
      cwd: HERE,
      // The fake holds the "__delay__" reply for 400ms — well past the 150ms
      // per-request timeout — so the reply lands after the request has timed out.
      env: { ...process.env, FAKE_BRIDGE_DELAY_MS: "400" },
      requestTimeoutMs: 150,
    });
    live.push(c);

    const slow = c.query({
      method: "hover",
      uri: "/A.vue",
      path: "/A.vue.tsx",
      offset: 1,
      version: 1,
      triggerCharacter: "__delay__",
    });
    await expect(slow).rejects.toThrow(/timed out/);

    // C's bridge is strictly sequential with no response ids, so a timeout poisons
    // the whole session: a subsequent request rejects IMMEDIATELY rather than
    // being parked as a waiter that the slow reply would later be misattributed to.
    // The regression (splice-the-waiter-and-keep-going) leaves the next request
    // pending, so this race would resolve to "pending" and fail the assertion.
    const next = c.query({
      method: "hover",
      uri: "/A.vue",
      path: "/A.vue.tsx",
      offset: 2,
      version: 1,
    });
    let nextValue: unknown;
    const settled = await Promise.race([
      next.then(
        (v) => {
          nextValue = v;
          return "resolved" as const;
        },
        () => "rejected" as const,
      ),
      new Promise<"pending">((r) => {
        const t = setTimeout(() => r("pending"), 50);
        t.unref();
      }),
    ]);
    expect(settled).toBe("rejected");
    // Discrimination: the next request must NEVER carry the slow request's reply.
    expect(JSON.stringify(nextValue ?? {})).not.toContain("offset=1");
  });
});
