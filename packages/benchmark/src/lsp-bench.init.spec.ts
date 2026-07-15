import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { LspClient } from "@verter/lsp-test-client";
import { afterEach, describe, expect, it } from "vitest";

import { initializeBenchmarkClient, makeInitializeParams } from "./lsp-bench.init";

const HERE = dirname(fileURLToPath(import.meta.url));
const FIXTURE = join(HERE, "fixtures", "lsp-init-fake-server.mjs");

const live: LspClient[] = [];

function makeClient(env: NodeJS.ProcessEnv = {}): LspClient {
  const client = new LspClient("bench-fake", process.execPath, [FIXTURE], undefined, { env });
  live.push(client);
  return client;
}

afterEach(async () => {
  while (live.length) {
    const client = live.pop()!;
    await client.kill().catch(() => {});
  }
});

interface InitResult {
  receivedPositionEncodings: string[] | null;
  receivedInitializationOptions: unknown;
}

describe("initializeBenchmarkClient position-encoding negotiation", () => {
  it("advertises general.positionEncodings and adopts the server's chosen encoding", async () => {
    const client = makeClient({ FAKE_INIT_ENCODING: "utf-8" });
    // Before the handshake the client sits at the LSP default.
    expect(client.positionEncoding).toBe("utf-16");

    const result = await initializeBenchmarkClient<InitResult>(
      client,
      "file:///bench",
      "bench",
      5000,
    );

    // Advertised: the benchmark's init params reached the server WITH the
    // encoding list. A raw `initialize` request (the bug) arrives without it, so
    // the server would echo null here.
    expect(result.receivedPositionEncodings).toEqual(["utf-16", "utf-8"]);
    // Adopted: the client honoured the server's chosen utf-8...
    expect(client.positionEncoding).toBe("utf-8");
    // ...and was NOT silently left at the utf-16 default.
    expect(client.positionEncoding).not.toBe("utf-16");
  });

  it("keeps the encoding handshake when initializationOptions are passed (Volar path)", async () => {
    const client = makeClient({ FAKE_INIT_ENCODING: "utf-8" });
    const volarInitOptions = { typescript: { tsdk: "/tmp/tsdk" } };

    const result = await initializeBenchmarkClient<InitResult>(
      client,
      "file:///bench",
      "bench",
      5000,
      volarInitOptions,
    );

    // The Volar call site passes initializationOptions; they must survive...
    expect(result.receivedInitializationOptions).toEqual(volarInitOptions);
    // ...and the encoding negotiation still happens for that path too.
    expect(result.receivedPositionEncodings).toEqual(["utf-16", "utf-8"]);
    expect(client.positionEncoding).toBe("utf-8");
  });

  it("leaves the raw initialize params free of encoding metadata (the helper adds it)", () => {
    // The discriminating contract: makeInitializeParams must NOT itself carry
    // general.positionEncodings — the encoding list is injected only by routing
    // through LspClient.initialize. So a regression to
    // sendRequest("initialize", makeInitializeParams(...)) would advertise
    // nothing. This pins that the params alone are insufficient.
    const params = makeInitializeParams("file:///bench", "bench") as {
      capabilities: { general?: { positionEncodings?: string[] } };
    };
    expect(params.capabilities.general?.positionEncodings).toBeUndefined();
  });
});
