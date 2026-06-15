// Hermetic fake `verter-dx-baseline` bridge used by @verter/dx-harness
// bridge-client tests. It speaks the same newline-delimited JSON request/response
// protocol as the real bridge (crates/verter_dx_baseline/src/protocol.rs) but is
// driven by canned logic so a single fixture stands in for the real Rust binary.
//
//   FAKE_BRIDGE_STDERR=<line>     written to stderr at startup (stderr-capture test).
//   FAKE_BRIDGE_DELAY_MS=<ms>     delay applied to a query whose triggerCharacter
//                                 is "__delay__" (defaults to 400ms) so the client
//                                 times the request out and the response arrives
//                                 late — exercises the timeout/correlation path.
//
// Behaviour: hello echoes the tool root + utf-8 capabilities; query at version
// 999 is refused stale; a hover query echoes its byte offset in the contents;
// shutdown reports the count of probes actually run.
import process from "node:process";

const env = process.env;
if (env.FAKE_BRIDGE_STDERR) process.stderr.write(env.FAKE_BRIDGE_STDERR + "\n");

let baselineRan = 0;
let partial = "";

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

function capabilities(provider) {
  return {
    provider: provider ?? "tsgo",
    positionEncoding: "utf-8",
    diagnosticsPush: true,
    completionResolve: true,
  };
}

function handle(req) {
  switch (req.type) {
    case "hello":
      return send({
        type: "hello",
        ok: true,
        provider: req.provider,
        skipped: false,
        baselineToolRootUsed: req.toolRoot?.expectedTsserverJs ?? req.toolRoot?.tsgoBin ?? null,
        capabilities: capabilities(req.provider),
      });
    case "open":
      return send({
        type: "open",
        ok: true,
        opened: (req.files ?? []).map((f) => f.path),
        version: req.version,
      });
    case "syncArtifacts":
      return send({
        type: "syncArtifacts",
        ok: true,
        uri: req.uri,
        version: req.version,
        applied: (req.files ?? []).map((f) => ({ path: f.path, action: "updated" })),
      });
    case "query": {
      if (req.version === 999) {
        return send({
          type: "error",
          kind: "baseline_artifact_stale",
          message: `stale: requested v${req.version}`,
          uri: req.uri,
          requestedVersion: req.version,
          haveVersion: 1,
        });
      }
      if (req.requiresSourceMap === true && req.path.endsWith(".nomap.tsx")) {
        return send({
          type: "error",
          kind: "compiled_code_map_absent",
          message: "no map",
          uri: req.uri,
          requestedVersion: req.version,
        });
      }
      baselineRan += 1;
      let result;
      if (req.method === "hover") {
        result = { kind: "hover", hover: { contents: `offset=${req.offset}` } };
      } else if (req.method === "completion") {
        result = { kind: "completion", items: [{ label: "x" }], isIncomplete: false };
      } else {
        result = { kind: "definition", locations: [] };
      }
      const frame = {
        type: "query",
        method: req.method,
        uri: req.uri,
        version: req.version,
        result,
        capabilities: capabilities(),
      };
      // A "__delay__" trigger holds the response back so the client times out and
      // the reply arrives late — without the fix that late reply would be
      // misattributed to the next request.
      if (req.triggerCharacter === "__delay__") {
        const ms = Number(env.FAKE_BRIDGE_DELAY_MS ?? "400");
        setTimeout(() => send(frame), ms);
        return;
      }
      return send(frame);
    }
    case "diagnostics":
      baselineRan += 1;
      return send({
        type: "diagnostics",
        uri: req.uri,
        version: req.version,
        diagnostics: [],
        capabilities: capabilities(),
      });
    case "shutdown":
      send({ type: "shutdown", ok: true, baselineRan });
      process.exit(0);
      return;
    default:
      return send({ type: "error", kind: "invalid_request", message: `unknown type ${req.type}` });
  }
}

process.stdin.setEncoding("utf-8");
process.stdin.on("data", (chunk) => {
  partial += chunk;
  const parts = partial.split("\n");
  partial = parts.pop() ?? "";
  for (const line of parts) {
    const trimmed = line.trim();
    if (trimmed.length === 0) continue;
    let req;
    try {
      req = JSON.parse(trimmed);
    } catch {
      send({ type: "error", kind: "invalid_request", message: "bad json" });
      continue;
    }
    handle(req);
  }
});
