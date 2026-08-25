// TCM0 probe 8 — the `API.fromLSPConnection` session-attach path.
//
// §4a recorded that the attach path — the scenario closest to TCM3's "attach to the editor-owned API
// session" topology candidate — was NOT probed, and delegated the known session-attach hang question to
// TCM3. This probe closes it: it spawns the pinned native `tsc --lsp`, drives a real LSP handshake,
// issues `custom/initializeAPISession` to obtain the API pipe, attaches a second client over that pipe
// with `API.fromLSPConnection`, and exercises it.
//
// One methodological note that matters, because it produced a false result on the first attempt: the LSP
// server issues its OWN requests to the client (`client/registerCapability`). A harness that never answers
// them blocks the server, and `custom/initializeAPISession` then times out — which looks exactly like the
// hang this probe exists to test for. This probe answers every server-initiated request, so a timeout here
// is the server's, not the harness's.
import { spawn } from "node:child_process";
import { mkdtempSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  resolveCandidate,
  loadSyncApi,
  loadAsyncApi,
  record,
  check,
  assert,
  section,
  finish,
} from "./harness.mjs";

const ATTACH_TIMEOUT_MS = 20000;

/** Async sibling of harness `check()` — same contract, awaits the body. */
let asyncFailures = 0;
async function check2(label, fn) {
  try {
    console.log(`  PASS  ${label} — ${await fn()}`);
  } catch (err) {
    asyncFailures++;
    console.log(`  FAIL  ${label} — ${err && err.message ? err.message : String(err)}`);
  }
}

const candidate = resolveCandidate();
const { API: SyncAPI } = await loadSyncApi(candidate);
const { API: AsyncAPI } = await loadAsyncApi(candidate);
const nativePkg = candidate.require.resolve(
  `@typescript/typescript-${process.platform}-${process.arch}/package.json`,
);
const exe = join(nativePkg, "..", "lib", process.platform === "win32" ? "tsc.exe" : "tsc");
assert(existsSync(exe), `native binary not found at ${exe}`);

const root = mkdtempSync(join(tmpdir(), "tcm0-lsp-"));
writeFileSync(
  join(root, "tsconfig.json"),
  JSON.stringify(
    {
      compilerOptions: { target: "es2022", strict: true, noEmit: true },
      include: ["*.ts"],
    },
    null,
    2,
  ),
);
writeFileSync(
  join(root, "main.ts"),
  'export interface W { id: string; size: number }\nexport const w: W = { id: "a", size: 1 };\n',
);

section(`probe8 LSP API-session attach — typescript@${candidate.version}`);

const server = spawn(exe, ["--lsp", "-stdio"], { cwd: root, stdio: ["pipe", "pipe", "pipe"] });
let stderr = "";
server.stderr.on("data", (d) => {
  stderr += d;
});

const send = (o) => {
  const b = Buffer.from(JSON.stringify(o), "utf8");
  server.stdin.write(`Content-Length: ${b.length}\r\n\r\n`);
  server.stdin.write(b);
};
let buf = Buffer.alloc(0);
const waiters = new Map();
let serverRequestsAnswered = 0;
server.stdout.on("data", (c) => {
  buf = Buffer.concat([buf, c]);
  for (;;) {
    const s = buf.indexOf("\r\n\r\n");
    if (s === -1) return;
    const m = /Content-Length: (\d+)/i.exec(buf.subarray(0, s).toString("utf8"));
    if (!m) return;
    const n = Number(m[1]);
    if (buf.length < s + 4 + n) return;
    const msg = JSON.parse(buf.subarray(s + 4, s + 4 + n).toString("utf8"));
    buf = buf.subarray(s + 4 + n);
    if (msg.id !== undefined && msg.method) {
      send({ jsonrpc: "2.0", id: msg.id, result: null });
      serverRequestsAnswered++;
      continue;
    }
    if (msg.id !== undefined && waiters.has(msg.id)) {
      waiters.get(msg.id)(msg);
      waiters.delete(msg.id);
    }
  }
});
const rpc = (id, method, params) =>
  new Promise((res, rej) => {
    waiters.set(id, res);
    send({ jsonrpc: "2.0", id, method, params });
    setTimeout(
      () => rej(new Error(`timeout after ${ATTACH_TIMEOUT_MS}ms on ${method}`)),
      ATTACH_TIMEOUT_MS,
    );
  });

let api;
try {
  const t0 = performance.now();
  const init = await rpc(1, "initialize", {
    processId: process.pid,
    rootUri: `file://${root}`,
    capabilities: {},
  });
  send({ jsonrpc: "2.0", method: "initialized", params: {} });
  record("LSP initialize (ms)", (performance.now() - t0).toFixed(0));

  check("the LSP server advertises real capabilities", () => {
    assert(init.result && init.result.capabilities, "no capabilities in initialize result");
    assert(init.result.capabilities.completionProvider, "no completionProvider");
    return `positionEncoding=${init.result.capabilities.positionEncoding}`;
  });

  const t1 = performance.now();
  const session = await rpc(2, "custom/initializeAPISession", {});
  const attachRequestMs = performance.now() - t1;

  check("custom/initializeAPISession returns a session id and a pipe, without hanging", () => {
    assert(session.result, `error: ${JSON.stringify(session.error)}`);
    assert(
      typeof session.result.pipe === "string" && session.result.pipe.length > 0,
      `no pipe: ${JSON.stringify(session.result)}`,
    );
    assert(
      typeof session.result.sessionId === "string",
      `no sessionId: ${JSON.stringify(session.result)}`,
    );
    return `sessionId=${session.result.sessionId} in ${attachRequestMs.toFixed(0)}ms`;
  });

  // The SYNC client cannot attach at all. This is a hard capability limit, not a timing result.
  check("the SYNC client CANNOT attach over a pipe — it refuses by design", () => {
    let msg = "";
    try {
      SyncAPI.fromLSPConnection({ pipe: session.result.pipe });
      throw new Error("the sync client attached — this capability limit no longer holds");
    } catch (err) {
      msg = err.message;
    }
    assert(
      /Socket connections are not yet supported in the sync client/.test(msg),
      `threw: ${msg}`,
    );
    return `threw: ${msg} (dist/api/sync/client.js:11)`;
  });

  // `fromLSPConnection` is ASYNC on this client and returns a Promise<API<true>> — awaiting it is
  // required; the sync sibling's signature is not a Promise, which is an easy porting trap.
  const t2 = performance.now();
  let lastErr;
  for (let attempt = 0; attempt < 20 && !api; attempt++) {
    try {
      api = await AsyncAPI.fromLSPConnection({ pipe: session.result.pipe });
    } catch (e) {
      lastErr = e;
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  assert(api, `could not attach after 20 attempts over 2s: ${lastErr && lastErr.message}`);
  const snapshot = await api.updateSnapshot({ openProjects: [join(root, "tsconfig.json")] });
  const attachMs = performance.now() - t2;
  record("async fromLSPConnection + first updateSnapshot (ms)", attachMs.toFixed(0));

  await check2(
    "an API client ATTACHED to the LSP session resolves the project and its files",
    async () => {
      const project = await snapshot.getProject(join(root, "tsconfig.json"));
      assert(project, "attached client could not resolve the opened project");
      const names = await project.program.getSourceFileNames();
      assert(
        names.some((n) => n.endsWith("main.ts")),
        "main.ts absent from the attached client's program",
      );
      return `${names.length} files visible over the attached pipe`;
    },
  );

  await check2("the attached client answers a real semantic query, not just metadata", async () => {
    const project = await snapshot.getProject(join(root, "tsconfig.json"));
    const text =
      'export interface W { id: string; size: number }\nexport const w: W = { id: "a", size: 1 };\n';
    const sym = await project.checker.getSymbolAtPosition(
      join(root, "main.ts"),
      text.indexOf("W {"),
    );
    assert(sym, "no symbol resolved over the attached session");
    const t = await project.checker.getDeclaredTypeOfSymbol(sym);
    const props = (await project.checker.getPropertiesOfType(t)).map((x) => x.name).sort();
    assert(props.join(",") === "id,size", `properties=[${props.join(",")}]`);
    return `resolved interface W with members [${props.join(",")}] over the attached pipe`;
  });

  check("NO session-attach hang was observed on this path", () => {
    assert(
      attachRequestMs < ATTACH_TIMEOUT_MS && attachMs < ATTACH_TIMEOUT_MS,
      `attach request ${attachRequestMs.toFixed(0)}ms / attach+snapshot ${attachMs.toFixed(0)}ms`,
    );
    return `session request ${attachRequestMs.toFixed(0)}ms, attach+first snapshot ${attachMs.toFixed(0)}ms, both well inside the ${ATTACH_TIMEOUT_MS}ms budget`;
  });

  // Checked LAST, not first: the server issues `client/registerCapability` asynchronously, so sampling
  // this counter early is a race. By now all traffic has flowed. If this is ever 0, every "no hang"
  // result above is meaningless — a harness that ignores the server's own requests blocks it and produces
  // a FALSE hang, which is exactly what the first version of this probe reported before it was fixed.
  check(
    "this harness answered the server's OWN requests, so a hang here would be the server's",
    () => {
      assert(
        serverRequestsAnswered > 0,
        "the server issued no request to answer — a timeout could not be distinguished from a harness bug",
      );
      return `${serverRequestsAnswered} server-initiated request(s) answered`;
    },
  );

  await snapshot.dispose();
  // Close the attached client while the server is still alive; closing after `server.kill()` writes to a
  // dead pipe and throws EPIPE from inside the vendored jsonrpc writer.
  try {
    await api.close();
  } catch {
    /* teardown only */
  }
  api = undefined;
} catch (err) {
  console.log(`  FAIL  attach path — ${err.message}`);
  if (stderr) console.log(`  server stderr: ${stderr.slice(0, 500)}`);
  process.exitCode = 1;
} finally {
  if (api) {
    try {
      await api.close();
    } catch {
      /* teardown only */
    }
  }
  server.kill();
  rmSync(root, { recursive: true, force: true });
}
if (asyncFailures > 0) {
  console.log(`\nASYNC FAILURES: ${asyncFailures}`);
  process.exitCode = 1;
}
finish();
