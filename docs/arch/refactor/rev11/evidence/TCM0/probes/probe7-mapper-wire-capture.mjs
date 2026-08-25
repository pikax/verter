// TCM0 probe 7 — LIVE capture of the content-mapper wire protocol.
//
// `package-lock-and-semantic-api.md` §3 recorded that static `strings` extraction on the stripped Go
// binary could NOT isolate the literal lowercase wire method names, and delegated the exact spelling to
// TCM2. That delegation was correct for static extraction and wrong as a limit: the spelling is
// obtainable today by running the real thing. This probe configures a real `contentMappers` entry in a
// real tsconfig, points it at a stub mapper process, runs the pinned native `tsc --runExternalCode`, and
// records every JSON-RPC frame TypeScript sends.
//
// It asserts the four-step lifecycle by NAME and ORDER, so it goes red if upstream renames a method or
// reorders the handshake.
import { mkdtempSync, writeFileSync, mkdirSync, readFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { resolveCandidate, record, check, assert, section, finish } from "./harness.mjs";

const candidate = resolveCandidate();
// The native package's `exports` map does not expose `./lib/tsc`, so resolve its package.json (which IS
// exported) and join — the same path the shipped `getExePath.js` builds.
const nativePkg = candidate.require.resolve(
  `@typescript/typescript-${process.platform}-${process.arch}/package.json`,
);
const exe = join(nativePkg, "..", "lib", process.platform === "win32" ? "tsc.exe" : "tsc");
assert(existsSync(exe), `native binary not found at ${exe}`);

const root = mkdtempSync(join(tmpdir(), "tcm0-wire-"));
const log = join(root, "frames.log");
section(`probe7 content-mapper wire capture — typescript@${candidate.version}`);

try {
  mkdirSync(join(root, "node_modules", "stub-mapper"), { recursive: true });
  // The manifest key is `typescript.contentMapper`, and its spawn key is `exec` (a non-empty string
  // array) — both discovered by iterating against the binary's own typed config errors (TS100034/TS100035).
  writeFileSync(
    join(root, "node_modules", "stub-mapper", "package.json"),
    JSON.stringify(
      {
        name: "stub-mapper",
        version: "1.0.0",
        type: "module",
        typescript: { contentMapper: { exec: ["node", "mapper.mjs"] } },
      },
      null,
      2,
    ),
  );
  writeFileSync(
    join(root, "node_modules", "stub-mapper", "mapper.mjs"),
    `
import { appendFileSync } from "node:fs";
const LOG = process.env.WIRE_LOG;
const log = (d, o) => appendFileSync(LOG, d + " " + JSON.stringify(o) + "\\n");
const send = (o) => { const b = Buffer.from(JSON.stringify(o), "utf8");
  process.stdout.write(\`Content-Length: \${b.length}\\r\\n\\r\\n\`); process.stdout.write(b); log("<--", o); };
let buf = Buffer.alloc(0);
process.stdin.on("data", (c) => { buf = Buffer.concat([buf, c]);
  for (;;) { const sep = buf.indexOf("\\r\\n\\r\\n"); if (sep === -1) return;
    const m = /Content-Length: (\\d+)/i.exec(buf.subarray(0, sep).toString("utf8")); if (!m) return;
    const len = Number(m[1]); if (buf.length < sep + 4 + len) return;
    const msg = JSON.parse(buf.subarray(sep + 4, sep + 4 + len).toString("utf8"));
    buf = buf.subarray(sep + 4 + len); log("-->", msg);
    if (msg.method === "initialize") {
      send({ jsonrpc: "2.0", id: msg.id, result: { name: "stub-mapper", version: "1.0.0",
        diagnosticSource: "stub-mapper", positionEncoding: "utf-8", capabilities: {} } });
    } else if (msg.id !== undefined) { send({ jsonrpc: "2.0", id: msg.id, result: {} }); }
  } });
`,
  );
  writeFileSync(
    join(root, "tsconfig.json"),
    JSON.stringify(
      {
        compilerOptions: { noEmit: true, strict: true },
        contentMappers: [{ package: "stub-mapper", extensions: [".stub"] }],
        include: ["*.ts", "*.stub"],
      },
      null,
      2,
    ),
  );
  writeFileSync(join(root, "main.ts"), "export const a: number = 1;\n");
  writeFileSync(join(root, "thing.stub"), "stub content\n");
  writeFileSync(log, "");

  const res = spawnSync(exe, ["--project", ".", "--runExternalCode"], {
    cwd: root,
    encoding: "utf8",
    timeout: 60000,
    env: { ...process.env, WIRE_LOG: log },
  });
  record("tsc exit", res.status);
  record(
    "why tsc exits non-zero",
    "expected — the stub answers `transform` with `{}`, which is not a " +
      "usable mapped output, so the compile fails AFTER the lifecycle completes",
  );

  check(
    "the compile fails for the EXPECTED reason — an unusable transform result, not a protocol error",
    () => {
      // This probe establishes the REQUEST half of the wire contract. It deliberately does NOT establish the
      // transform RESPONSE contract: several plausible encodings were rejected by the decoder, so the stub
      // answers `{}` and the compile fails at output decoding. Asserting the failure MODE is what stops a
      // reader treating `tsc exit: 2` as either success or as evidence the response shape is known.
      const out = `${res.stdout || ""}${res.stderr || ""}`;
      assert(
        res.status !== 0,
        "tsc succeeded — the stub's empty transform result should not type-check",
      );
      assert(
        /unsupported virtual extension/.test(out),
        `expected an output-decoding failure, got: ${out.slice(0, 300)}`,
      );
      assert(
        !/could not be initialized|did not respond/.test(out),
        `the mapper failed to initialise — the captured lifecycle below would be incomplete: ${out.slice(0, 300)}`,
      );
      return "TS100025 unsupported virtual extension — the four-step lifecycle completed, the OUTPUT was rejected";
    },
  );

  const frames = readFileSync(log, "utf8")
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((l) => ({ dir: l.slice(0, 3), msg: JSON.parse(l.slice(4)) }));
  const inbound = frames.filter((f) => f.dir === "-->").map((f) => f.msg);
  const methods = inbound.map((m) => m.method);
  record("methods captured, in order", methods.join(" -> "));

  check(
    "the wire lifecycle is exactly initialize -> openProject -> transform -> closeProject",
    () => {
      assert(
        methods.join(",") === "initialize,openProject,transform,closeProject",
        `captured [${methods.join(",")}]`,
      );
      return methods.join(" -> ");
    },
  );
  check("method names are lowercase-initial camelCase, not the capitalised Go type names", () => {
    for (const m of methods)
      assert(/^[a-z][A-Za-z]*$/.test(m), `"${m}" is not lowercase-initial camelCase`);
    return methods.join(", ");
  });
  check("initialize params offer both position encodings", () => {
    const p = inbound.find((m) => m.method === "initialize").params;
    assert(Array.isArray(p.positionEncodings), `params=${JSON.stringify(p)}`);
    assert(
      p.positionEncodings.includes("utf-8") && p.positionEncodings.includes("utf-16"),
      `positionEncodings=${JSON.stringify(p.positionEncodings)}`,
    );
    return JSON.stringify(p);
  });
  check("openProject params carry configFileName, projectHandle and compilerOptions", () => {
    const p = inbound.find((m) => m.method === "openProject").params;
    for (const k of ["configFileName", "projectHandle", "compilerOptions"]) {
      assert(k in p, `missing ${k}; got [${Object.keys(p).join(",")}]`);
    }
    assert(
      p.compilerOptions.runExternalCode === true,
      "runExternalCode not echoed into compilerOptions",
    );
    return `keys=[${Object.keys(p).join(",")}] handle=${p.projectHandle}`;
  });
  check("transform params carry fileName, content and the SAME projectHandle", () => {
    const open = inbound.find((m) => m.method === "openProject").params;
    const p = inbound.find((m) => m.method === "transform").params;
    for (const k of ["fileName", "content", "projectHandle"]) {
      assert(k in p, `missing ${k}; got [${Object.keys(p).join(",")}]`);
    }
    assert(
      p.projectHandle === open.projectHandle,
      "transform handle differs from openProject handle",
    );
    assert(
      p.content === "stub content\n",
      `content=${JSON.stringify(p.content)} — not the authored bytes`,
    );
    assert(p.fileName.endsWith("thing.stub"), `fileName=${p.fileName}`);
    return `keys=[${Object.keys(p).join(",")}]`;
  });
  check("transform is sent ONLY for the mapper's declared extension", () => {
    const transformed = inbound
      .filter((m) => m.method === "transform")
      .map((m) => m.params.fileName);
    assert(
      transformed.length === 1,
      `${transformed.length} transform calls: ${transformed.join(",")}`,
    );
    assert(!transformed.some((f) => f.endsWith(".ts")), "a .ts file was sent to the mapper");
    return "1 call, for thing.stub only — main.ts never reached the mapper";
  });
  check("closeProject params carry the projectHandle and nothing else", () => {
    const p = inbound.find((m) => m.method === "closeProject").params;
    assert(Object.keys(p).join(",") === "projectHandle", `keys=[${Object.keys(p).join(",")}]`);
    return `keys=[${Object.keys(p).join(",")}]`;
  });
  check("projectHandle is {package}@{version}:{n}", () => {
    const h = inbound.find((m) => m.method === "openProject").params.projectHandle;
    assert(/^stub-mapper@1\.0\.0:\d+$/.test(h), `handle=${h}`);
    return h;
  });
  check(
    "the mapper connection carries NO inbound request from TypeScript beyond these four",
    () => {
      // Corroborates the rejectHandler finding from the opposite side: across a whole compile, TypeScript
      // issued exactly the four lifecycle requests and nothing else.
      assert(inbound.length === 4, `${inbound.length} inbound frames: ${methods.join(",")}`);
      return "4 frames, all lifecycle";
    },
  );

  record("raw frames", "");
  for (const f of frames) console.log(`    ${f.dir} ${JSON.stringify(f.msg)}`);
} finally {
  rmSync(root, { recursive: true, force: true });
}
finish();
