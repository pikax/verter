// Minimal hermetic fake LSP server for the benchmark initialize-handshake test.
//
// It speaks the same Content-Length-framed JSON-RPC stdio protocol as a real
// language server but only implements `initialize` (plus `shutdown`/`exit`). The
// initialize result echoes back the client's advertised
// `general.positionEncodings` and the `initializationOptions` it received, and
// reports a chosen `positionEncoding`, so the test can assert the benchmark
// negotiates encoding through LspClient.initialize rather than a raw request.
//
//   FAKE_INIT_ENCODING  positionEncoding returned from `initialize` (default "utf-8").
import process from "node:process";

function write(payload) {
  const body = Buffer.from(JSON.stringify(payload), "utf-8");
  const header = Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, "utf-8");
  process.stdout.write(Buffer.concat([header, body]));
}

let buf = Buffer.alloc(0);
process.stdin.on("data", (chunk) => {
  buf = buf.length === 0 ? chunk : Buffer.concat([buf, chunk]);
  for (;;) {
    const headerEnd = buf.indexOf("\r\n\r\n");
    if (headerEnd === -1) break;
    const header = buf.subarray(0, headerEnd).toString("utf-8");
    const m = header.match(/Content-Length:\s*(\d+)/i);
    if (!m) {
      buf = buf.subarray(headerEnd + 4);
      continue;
    }
    const len = Number.parseInt(m[1], 10);
    const start = headerEnd + 4;
    const end = start + len;
    if (buf.length < end) break;
    const body = buf.subarray(start, end).toString("utf-8");
    buf = buf.subarray(end);
    let msg;
    try {
      msg = JSON.parse(body);
    } catch {
      continue;
    }
    handle(msg);
  }
});

function handle(msg) {
  const { id, method, params } = msg;

  // Notifications (no id) — quit on `exit`, ignore the rest (e.g. "initialized").
  if (id === undefined) {
    if (method === "exit") process.exit(0);
    return;
  }

  if (method === "initialize") {
    const encoding = process.env.FAKE_INIT_ENCODING ?? "utf-8";
    write({
      jsonrpc: "2.0",
      id,
      result: {
        capabilities: { positionEncoding: encoding },
        serverInfo: { name: "bench-fake-lsp", version: "0.0.0" },
        receivedPositionEncodings: params?.capabilities?.general?.positionEncodings ?? null,
        receivedInitializationOptions: params?.initializationOptions ?? null,
      },
    });
    return;
  }

  if (method === "shutdown") {
    write({ jsonrpc: "2.0", id, result: null });
    return;
  }

  // Any other request still carries an id; answer so nothing hangs.
  write({ jsonrpc: "2.0", id, result: null });
}
