// Hermetic fake LSP server used by @verter/lsp-test-client transport tests.
//
// It speaks the same Content-Length-framed JSON-RPC stdio protocol as a real
// language server, but its behaviour is driven entirely by environment
// variables so a single fixture can stand in for every transport scenario.
// It never requires the real verter-lsp binary.
//
//   FAKE_STDERR_LINES   SOH (0001) separated lines written to stderr at startup.
//   FAKE_INIT_ENCODING  positionEncoding returned from `initialize`
//                       ("utf-8" | "utf-16" | "utf-32" | "none"). Default "utf-16".
//   FAKE_NO_RESPONSE=1  never answer requests (other than initialize/shutdown).
//   FAKE_EMIT_ON=<m>    on notification <m>, emit two `$/test/note` notifications
//                       ({kind:"nomatch"} then {kind:"match"}).
//   FAKE_STAY_ALIVE=1   hold an interval so the process does not exit on its own.
//   FAKE_IGNORE_SIGTERM=1  install a no-op SIGTERM handler (forces the hard-kill path).
//   FAKE_PROVOKE_ON=<m>  on notification <m>, send a server→client REQUEST (method
//                        FAKE_PROVOKE_METHOD, default "$/server/unknownRequest"). When
//                        the client's response arrives, re-emit it as a notification
//                        "$/test/clientReply" ({id, error, result}) so tests can assert
//                        how the client answered an unsolicited server request.
//   FAKE_PROVOKE_METHOD  method name for the provoked server→client request.
import process from "node:process";

const env = process.env;
const SEP = String.fromCharCode(1); // SOH delimiter between stderr lines
const PROVOKE_REQUEST_ID = 9001; // id of the server→client request FAKE_PROVOKE_ON sends

function write(payload) {
  const body = Buffer.from(JSON.stringify(payload), "utf-8");
  const header = Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, "utf-8");
  process.stdout.write(Buffer.concat([header, body]));
}

function notify(method, params) {
  write({ jsonrpc: "2.0", method, params });
}

// Install the SIGTERM handler BEFORE writing the stderr readiness lines: a test
// that waits for a stderr marker (e.g. "alive") before calling kill() relies on
// that marker meaning "the no-op SIGTERM handler is installed". Writing the
// marker first would let the parent observe it — and send SIGTERM — in the window
// before `process.on("SIGTERM", …)` runs, so the signal would hit the default
// terminate action during boot and the child would die immediately instead of
// swallowing it. Installing the handler first closes that race under load.
if (env.FAKE_IGNORE_SIGTERM === "1") {
  process.on("SIGTERM", () => {});
}

if (env.FAKE_STDERR_LINES) {
  for (const line of env.FAKE_STDERR_LINES.split(SEP)) {
    process.stderr.write(line + "\n");
  }
}

let keepAlive;
if (env.FAKE_STAY_ALIVE === "1") {
  keepAlive = setInterval(() => {}, 1000);
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

  // Client → server RESPONSE (carries an id but no method): the answer to a
  // server→client request we provoked. Forward it so tests can inspect it.
  if (id !== undefined && method === undefined) {
    if (id === PROVOKE_REQUEST_ID) {
      notify("$/test/clientReply", { id, error: msg.error ?? null, result: msg.result ?? null });
    }
    return;
  }

  // Notifications (no id).
  if (id === undefined) {
    if (method === "exit") {
      if (keepAlive) clearInterval(keepAlive);
      process.exit(0);
    }
    if (method === env.FAKE_EMIT_ON) {
      notify("$/test/note", { kind: "nomatch", value: 1 });
      notify("$/test/note", { kind: "match", value: 42 });
    }
    if (method === env.FAKE_PROVOKE_ON) {
      write({
        jsonrpc: "2.0",
        id: PROVOKE_REQUEST_ID,
        method: env.FAKE_PROVOKE_METHOD ?? "$/server/unknownRequest",
        params: { provoked: true },
      });
    }
    return;
  }

  // Requests (with id).
  if (method === "initialize") {
    const encoding = env.FAKE_INIT_ENCODING ?? "utf-16";
    const capabilities = {};
    if (encoding !== "none") capabilities.positionEncoding = encoding;
    write({
      jsonrpc: "2.0",
      id,
      result: {
        capabilities,
        serverInfo: { name: "fake-lsp", version: "0.0.0" },
        receivedPositionEncodings: params?.capabilities?.general?.positionEncodings ?? null,
        receivedCapabilities: params?.capabilities ?? null,
        receivedInitializationOptions: params?.initializationOptions ?? null,
      },
    });
    return;
  }

  if (method === "shutdown") {
    write({ jsonrpc: "2.0", id, result: null });
    return;
  }

  if (env.FAKE_NO_RESPONSE === "1") return;

  // Default: echo the params back so request round-trips can be asserted.
  write({ jsonrpc: "2.0", id, result: { echo: params ?? null } });
}
