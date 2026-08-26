// TCM0 probe 9 — the content-mapper transform RESPONSE contract, derived by live experiment.
//
// Probe 7 established the REQUEST half of the wire and recorded the successful RESPONSE body as
// unknown: several plausible encodings were rejected by the decoder, so probe 7's stub answers `{}`
// and asserts only that the compile fails at output decoding. This probe closes that residual.
//
// It also records the trap that made the residual persist. An early attempt read `{extension,
// content}` as the answer because that payload exits 0 — but `content` is an IGNORED unknown field,
// and the compile succeeded only because the virtual file was EMPTY. An empty program type-checks.
// The field carrying the virtual TypeScript source is `text`, and the discriminating evidence is not
// a green exit at all: it is a payload whose `text` contains a deliberate type ERROR, which the
// compiler must then report against the mapped file. A check that cannot go red proves nothing, and
// a vacuous pass is the commonest way for one to be unable to.
//
// Every assertion below is a behaviour of the pinned candidate observed directly, never inferred.
import { mkdtempSync, writeFileSync, mkdirSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { resolveCandidate, section, finish } from "./harness.mjs";

const candidate = resolveCandidate();
// The native package's `exports` map does not expose `./lib/tsc`, so resolve its package.json (which
// IS exported) and join — the same path the shipped `getExePath.js` builds.
const exe = join(
  candidate.require.resolve(
    `@typescript/typescript-${process.platform}-${process.arch}/package.json`,
  ),
  "..",
  "lib",
  process.platform === "win32" ? "tsc.exe" : "tsc",
);
if (!existsSync(exe)) {
  console.error("native binary not found at", exe);
  process.exit(2);
}
section(`probe9 content-mapper transform RESPONSE contract — typescript@${candidate.version}`);

const INIT = `{ name: "stub-mapper", version: "1.0.0", diagnosticSource: "stub-mapper", positionEncoding: "utf-8", capabilities: {} }`;

function run(src, resultExpr, files = {}, extraArgs = []) {
  const root = mkdtempSync(join(tmpdir(), "tcm0-c-"));
  try {
    mkdirSync(join(root, "node_modules", "stub-mapper"), { recursive: true });
    writeFileSync(
      join(root, "node_modules", "stub-mapper", "package.json"),
      JSON.stringify({
        name: "stub-mapper",
        version: "1.0.0",
        type: "module",
        typescript: { contentMapper: { exec: ["node", "mapper.mjs"] } },
      }),
    );
    writeFileSync(
      join(root, "node_modules", "stub-mapper", "mapper.mjs"),
      `
const send = (o) => { const b = Buffer.from(JSON.stringify(o), "utf8");
  process.stdout.write("Content-Length: " + b.length + "\\r\\n\\r\\n"); process.stdout.write(b); };
const TRANSFORM = (params) => (${resultExpr});
let buf = Buffer.alloc(0);
process.stdin.on("data", (c) => { buf = Buffer.concat([buf, c]);
  for (;;) { const sep = buf.indexOf("\\r\\n\\r\\n"); if (sep === -1) return;
    const m = /Content-Length: (\\d+)/i.exec(buf.subarray(0, sep).toString("utf8")); if (!m) return;
    const len = Number(m[1]); if (buf.length < sep + 4 + len) return;
    const msg = JSON.parse(buf.subarray(sep + 4, sep + 4 + len).toString("utf8"));
    buf = buf.subarray(sep + 4 + len);
    if (msg.method === "initialize") send({ jsonrpc: "2.0", id: msg.id, result: ${INIT} });
    else if (msg.method === "transform") send({ jsonrpc: "2.0", id: msg.id, result: TRANSFORM(msg.params) });
    else if (msg.id !== undefined) send({ jsonrpc: "2.0", id: msg.id, result: {} }); } });
`,
    );
    writeFileSync(
      join(root, "tsconfig.json"),
      JSON.stringify({
        compilerOptions: { noEmit: true, strict: true },
        contentMappers: [{ package: "stub-mapper", extensions: [".stub"] }],
        include: ["*.stub", "*.ts"],
      }),
    );
    writeFileSync(join(root, "thing.stub"), src);
    for (const [n, c] of Object.entries(files)) writeFileSync(join(root, n), c);
    const r = spawnSync(exe, ["--project", ".", "--runExternalCode", ...extraArgs], {
      cwd: root,
      encoding: "utf8",
      timeout: 60000,
    });
    const raw = `${r.stdout || ""}${r.stderr || ""}`;
    return {
      status: r.status,
      raw,
      out: raw
        .replace(/\s+/g, " ")
        .trim()
        .replace(/\/private\S*?\//g, ""),
    };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

let fails = 0;
const check = (label, cond, detail) => {
  if (cond) console.log(`ok   ${label}`);
  else {
    fails++;
    console.log(`FAIL ${label}\n       ${detail}`);
  }
};
const clean = (label, src, rx, files) => {
  const r = run(src, rx, files);
  check(label, r.status === 0, `status=${r.status} out=${r.out}`);
  return r;
};
const decodes = (label, src, rx, files) => {
  const r = run(src, rx, files);
  check(label, !r.out.includes("TS100025"), `wanted a decoded response, got: ${r.out}`);
  return r;
};
const errs = (label, src, rx, needle, files) => {
  const r = run(src, rx, files);
  check(label, r.out.includes(needle), `wanted ${JSON.stringify(needle)} got: ${r.out}`);
  return r;
};

const SRC = "let z=1\n"; // 8 bytes
const TXT = "let z=1\\n";
const BAD = "let z:string=1\\n"; // 15 bytes, has a type error
const ALL = 1048575; // SpanMapFeature.All

// ---- 1. Response shape ---------------------------------------------------
clean("extension+text alone compiles", SRC, `{ extension: ".ts", text: "${TXT}" }`);
clean("extension alone (text optional)", SRC, `{ extension: ".ts" }`);
errs(
  "missing extension is rejected",
  SRC,
  `{ text: "${TXT}" }`,
  "unsupported virtual extension ''",
);
errs("'text' IS the virtual source", SRC, `{ extension: ".ts", text: "${BAD}" }`, "TS2322");
clean("'content' is an ignored unknown field", SRC, `{ extension: ".ts", content: "${BAD}" }`);
clean("unknown fields ignored", SRC, `{ extension: ".ts", text: "${TXT}", zzzUnknown: 5 }`);

// ---- 2. extension domain -------------------------------------------------
for (const e of [".ts", ".tsx", ".js", ".jsx", ".mts", ".cts", ".mjs", ".cjs"])
  clean(`extension ${e} accepted`, SRC, `{ extension: "${e}", text: "${TXT}" }`);
errs(
  "extension .d.ts rejected",
  SRC,
  `{ extension: ".d.ts", text: "${TXT}" }`,
  "unsupported virtual extension '.d.ts'",
);
errs(
  "extension 'ts' (no dot) rejected",
  SRC,
  `{ extension: "ts", text: "${TXT}" }`,
  "unsupported virtual extension 'ts'",
);

// ---- 3. mappings: arity --------------------------------------------------
clean(
  "6-value entry decodes",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[0,8,0,8,0,${ALL}]] }`,
);
clean(
  "5-value entry decodes (features omitted)",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[0,8,0,8,0]] }`,
);
errs(
  "4-value entry rejected",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[0,8,0,8]] }`,
  "invalid transform response",
);
errs(
  "7-value entry rejected",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[0,8,0,8,0,${ALL},0]] }`,
  "invalid transform response",
);
errs(
  "object entry rejected",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [{virtualStart:0,virtualEnd:8,originalStart:0,originalEnd:8,kind:0}] }`,
  "invalid transform response",
);

// ---- 4. mappings: slots are (vStart, vLen, oStart, oLen, kind, features) --
// Asymmetric case: text = "XX" + src, so a length reading and an end reading differ.
const XT = "XXlet z=1\\n"; // 10
decodes(
  "(start,LENGTH): [2,8,0,8,0] decodes",
  SRC,
  `{ extension: ".ts", text: "${XT}", mappings: [[0,2,0,0,1,${ALL}],[2,8,0,8,0,${ALL}]] }`,
);
errs(
  "(start,END) reading refuted: [2,10,0,8,0] rejected",
  SRC,
  `{ extension: ".ts", text: "${XT}", mappings: [[0,2,0,0,1,${ALL}],[2,10,0,8,0,${ALL}]] }`,
  "invalid transform response",
);
errs(
  "vStart+vLen > len(text) rejected",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[0,9,0,8,0,${ALL}]] }`,
  "invalid transform response",
);
errs(
  "oStart+oLen > len(original) rejected",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[0,8,0,9,0,${ALL}]] }`,
  "invalid transform response",
);
decodes(
  "independent lengths: [0,2,0,0,1] (vLen 2, oLen 0)",
  SRC,
  `{ extension: ".ts", text: "${XT}", mappings: [[0,2,0,0,1,${ALL}],[2,8,0,8,0,${ALL}]] }`,
);
clean(
  "nonzero starts legal: [4,4,4,4,0]",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[0,4,0,4,0,${ALL}],[4,4,4,4,0,${ALL}]] }`,
);

// ---- 5. mappings: semantics ---------------------------------------------
errs(
  "verbatim mismatch -> TS100029",
  SRC,
  `{ extension: ".ts", text: "let q=1\\n", mappings: [[0,8,0,8,0,${ALL}]] }`,
  "TS100029",
);
errs(
  "kind 3 -> TS100040",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[0,8,0,8,3,${ALL}]] }`,
  "TS100040",
);
errs(
  "features > All -> TS100039",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[0,8,0,8,0,${ALL + 1}]] }`,
  "TS100039",
);
errs(
  "overlapping virtual spans -> TS100037",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[0,4,0,4,0,${ALL}],[2,4,2,4,0,${ALL}]] }`,
  "TS100037",
);
clean(
  "entries may be given out of virtual order",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[4,4,4,4,0,${ALL}],[0,4,0,4,0,${ALL}]] }`,
);
for (const k of [0, 1, 2])
  clean(
    `kind ${k} accepted`,
    SRC,
    `{ extension: ".ts", text: "${TXT}", mappings: [[0,8,0,8,${k},${ALL}]] }`,
  );

// ---- 6. mapping fidelity: verbatim is exact, atom collapses to span start -
{
  const src15 = "let z:string=1\n";
  const exact = run(
    src15,
    `{ extension: ".ts", text: "${BAD}", mappings: [[0,15,0,15,0,${ALL}]] }`,
  );
  check(
    "verbatim maps the error to its exact column (1,5)",
    exact.out.includes("thing.stub(1,5): error TS2322"),
    exact.out,
  );
  const atom = run(src15, `{ extension: ".ts", text: "${BAD}", mappings: [[0,15,0,15,1,${ALL}]] }`);
  check(
    "atom collapses the error to the span start (1,1)",
    atom.out.includes("thing.stub(1,1): error TS2322"),
    atom.out,
  );
  const none = run(src15, `{ extension: ".ts", text: "${BAD}" }`);
  check(
    "no mappings -> 'no corresponding location' note",
    none.out.includes("has no corresponding location in the original file"),
    none.out,
  );
}

// ---- 7. supplemental outputs --------------------------------------------
clean(
  "supplemental output accepted",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[0,8,0,8,0,${ALL}]], supplemental: [{ extension: ".ts", text: "export const q = 1;\\n" }] }`,
);
errs(
  "supplemental #0 is named <file>.0.<ext>",
  SRC,
  `{ extension: ".ts", text: "${TXT}", supplemental: [{ extension: ".ts", text: "export {};\\n" }] }`,
  "thing.stub.0.ts' conflicts with an existing file",
  { "thing.stub.0.ts": "export {};\n" },
);

// ---- 8. mapper diagnostics ----------------------------------------------
errs(
  "diagnostics entry surfaces as <source><code>: <messageText>",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[0,8,0,8,0,${ALL}]], diagnostics: [{ code: 1234, category: 1, start: 0, length: 3, messageText: "hello world" }] }`,
  "error stub-mapper1234: hello world",
);
errs(
  "diagnostic 'start' is a VIRTUAL offset, mapped back",
  SRC,
  `{ extension: ".ts", text: "${TXT}", mappings: [[0,8,0,8,0,${ALL}]], diagnostics: [{ code: 7, category: 1, start: 4, length: 1, messageText: "at z" }] }`,
  "thing.stub(1,5): error stub-mapper7: at z",
);
errs(
  "diagnostic span outside text rejected",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnostics: [{ code: 7, category: 1, start: 100, length: 1, messageText: "x" }] }`,
  "invalid transform response",
);

// ---- 9. diagnostic directives ------------------------------------------
clean(
  "diagnosticDirectives is an object with two array members",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [], directives: [] } }`,
);
errs(
  "diagnosticDirectives array form rejected",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: [] }`,
  "invalid transform response",
);
clean(
  "directives member is independently optional",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [] } }`,
);
clean(
  "unusedExpectDirectiveDiagnostics member is independently optional",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { directives: [] } }`,
);
errs(
  "directives member must be an array",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [], directives: {} } }`,
  "invalid transform response",
);
errs(
  "unusedExpectDirectiveDiagnostics member must be an array",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: {}, directives: [] } }`,
  "invalid transform response",
);

clean(
  "diagnostic directive 5-value tuple decodes",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { directives: [[0,1,0,1,0]] } }`,
);
errs(
  "diagnostic directive 6-value tuple decodes",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{code:9000,messageText:"unused"},{code:9001,messageText:"selected"}], directives: [[0,1,0,1,1,1]] } }`,
  "error stub-mapper9001: selected",
);
errs(
  "diagnostic directive 4-value tuple rejected",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { directives: [[0,1,0,1]] } }`,
  "invalid transform response",
);
errs(
  "diagnostic directive 7-value tuple rejected",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { directives: [[0,1,0,1,0,0,0]] } }`,
  "invalid transform response",
);
errs(
  "diagnostic directive object entry rejected",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { directives: [{originalStart:0,originalLength:1,virtualStart:0,virtualEnd:1,policy:0}] } }`,
  "invalid transform response",
);

for (const [slot, tuple] of [
  [0, '["x",1,0,1,0]'],
  [1, '[0,"x",0,1,0]'],
  [2, '[0,1,"x",1,0]'],
  [3, '[0,1,0,"x",0]'],
  [4, '[0,1,0,1,"x"]'],
  [5, '[0,1,0,1,1,"x"]'],
])
  errs(
    `diagnostic directive slot ${slot} must be numeric`,
    SRC,
    `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{code:9000,messageText:"unused"}], directives: [${tuple}] } }`,
    "invalid transform response",
  );

{
  const r = run(
    SRC,
    `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{code:9000,messageText:"unused"}], directives: [[1,1,0,1,1]] } }`,
    {},
    ["--pretty", "true"],
  );
  const plain = r.raw.replace(/\x1b\[[0-9;]*m/g, "");
  check("directive slot 0 is originalStart", plain.includes("thing.stub:1:2 - error"), plain);
}
{
  const r = run(
    SRC,
    `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{code:9000,messageText:"unused"}], directives: [[0,3,0,1,1]] } }`,
    {},
    ["--pretty", "true"],
  );
  const plain = r.raw.replace(/\x1b\[[0-9;]*m/g, "");
  check("directive slot 1 is originalLength", plain.includes("\n  ~~~\n"), plain);
}
clean(
  "directive slot 2 virtualStart includes diagnostic at offset 4",
  "let z:string=1\n",
  `{ extension: ".ts", text: "${BAD}", mappings: [[0,15,0,15,0,${ALL}]], diagnosticDirectives: { directives: [[0,1,4,15,0]] } }`,
);
errs(
  "directive slot 2 virtualStart excludes diagnostic when moved to 5",
  "let z:string=1\n",
  `{ extension: ".ts", text: "${BAD}", mappings: [[0,15,0,15,0,${ALL}]], diagnosticDirectives: { directives: [[0,1,5,15,0]] } }`,
  "TS2322",
);
clean(
  "directive slot 3 virtualEnd includes diagnostic through offset 5",
  "let z:string=1\n",
  `{ extension: ".ts", text: "${BAD}", mappings: [[0,15,0,15,0,${ALL}]], diagnosticDirectives: { directives: [[0,1,0,5,0]] } }`,
);
errs(
  "directive slot 3 virtualEnd excludes diagnostic when moved to 4",
  "let z:string=1\n",
  `{ extension: ".ts", text: "${BAD}", mappings: [[0,15,0,15,0,${ALL}]], diagnosticDirectives: { directives: [[0,1,0,4,0]] } }`,
  "TS2322",
);
errs(
  "directive slot 4 policy 1 reports an unused-expect diagnostic",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{code:9000,messageText:"unused"}], directives: [[0,1,0,1,1]] } }`,
  "error stub-mapper9000: unused",
);
errs(
  "directive slot 4 rejects policy 2",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { directives: [[0,1,0,1,2]] } }`,
  "invalid policy '2'",
);
errs(
  "directive slot 5 selects one shared unused-expect diagnostic",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{code:9000,messageText:"first"},{code:9001,messageText:"second"}], directives: [[0,1,0,1,1,1]] } }`,
  "error stub-mapper9001: second",
);
errs(
  "omitted slot 5 rejected when shared unused-expect count is not one",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{code:9000,messageText:"first"},{code:9001,messageText:"second"}], directives: [[0,1,0,1,1]] } }`,
  "must specify 'unusedExpectDirectiveIndex'",
);
errs(
  "slot 5 out-of-range unused-expect index rejected",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{code:9000,messageText:"unused"}], directives: [[0,1,0,1,1,1]] } }`,
  "invalid 'unusedExpectDirectiveIndex'",
);

errs(
  "unused-expect diagnostic code is numeric and prefixes the mapper code",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{code:9001,messageText:"unused"}], directives: [[0,1,0,1,1]] } }`,
  "error stub-mapper9001: unused",
);
errs(
  "unused-expect diagnostic messageText is the surfaced message",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{code:9000,messageText:"distinct unused message"}], directives: [[0,1,0,1,1]] } }`,
  "distinct unused message",
);
errs(
  "unused-expect diagnostic string code rejected",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{code:"x",messageText:"unused"}], directives: [[0,1,0,1,1]] } }`,
  "invalid transform response",
);
errs(
  "unused-expect diagnostic numeric messageText rejected",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{code:9000,messageText:7}], directives: [[0,1,0,1,1]] } }`,
  "invalid transform response",
);
errs(
  "missing unused-expect code decodes as zero",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{messageText:"unused"}], directives: [[0,1,0,1,1]] } }`,
  "error stub-mapper0: unused",
);
errs(
  "missing unused-expect messageText decodes but panics when rendered",
  SRC,
  `{ extension: ".ts", text: "${TXT}", diagnosticDirectives: { unusedExpectDirectiveDiagnostics: [{code:9000}], directives: [[0,1,0,1,1]] } }`,
  "panic: Unknown diagnostic message",
);

if (fails > 0) {
  process.exitCode = 1;
}
finish();
