// TCM0 probe 10 — the EXTERNAL SOURCE UNIT contract: transform input, project and configuration
// identity, for a file referenced from inside a carrier rather than written into it.
//
// `external-source-decision-table.md` row 7 records `<template src="...">` as content-mapped with the
// model NOT YET PROVEN: the steering permits an external unit to be independently content-mapped only
// under a proven project/context contract, and the transform-input, project and configuration
// identities had never been established. Naming a conditional model is not an architecture lock, so
// this probe establishes the three by experiment against the pinned candidate.
//
// Each assertion is paired with the RIVAL hypothesis it excludes, and each rival can be injected on
// the command line to drive the corresponding assertion red — a check nobody has watched fail is not
// evidence:
//
//   --ts <dir>                 the pinned candidate (required, as for every probe here)
//   --inject input             asserts the referencing file's bytes instead — drives 1.4 red
//   --inject project           asserts a different project handle — drives 2a.3 red
//   --inject config            asserts one shared configuration — drives 3.4 red
//   --inject mapper            makes the mapper report the wrong per-project option — real tsc red
//
// The fixtures are deliberately unmistakable: no substring of the carrier's own bytes occurs in the
// external unit's, so "the mapper received the right file" cannot pass by coincidence. The type-level
// half is non-vacuous by construction — the external unit's emitted `text` feeds a type the carrier
// asserts, so an empty or wrong transform reddens the compile rather than passing quietly.
import { mkdtempSync, writeFileSync, mkdirSync, rmSync, existsSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { spawnSync } from "node:child_process";
import { resolveCandidate, section, finish } from "./harness.mjs";

const candidate = resolveCandidate();
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
section(`probe10 external-source-unit contract — typescript@${candidate.version}`);

const argi = process.argv.indexOf("--inject");
const INJECT = argi === -1 ? "" : process.argv[argi + 1] || "";

// ---- unmistakable payloads: no substring of one occurs in the other ----------
const CARRIER_BYTES = `<template src="./ext/thing.tplx"></template>\n<!--QQQ-CARRIER-OWN-BYTES-QQQ-->\n`;
const TPL_BYTES = `<div>ZZZ-EXTERNAL-UNIT-OWN-BYTES-ZZZ</div>\n`;

// ---- mapper source -----------------------------------------------------------
// Records openProject compilerOptions per handle; parses the carrier's src="..."
// attribute out of the carrier's OWN content and emits an import for it.
const MAPPER = `
import { appendFileSync } from "node:fs";
const log = (o) => appendFileSync(process.env.TCM_LOG, JSON.stringify(o) + "\\n");
const send = (o) => { const b = Buffer.from(JSON.stringify(o), "utf8");
  process.stdout.write("Content-Length: " + b.length + "\\r\\n\\r\\n"); process.stdout.write(b); };
const OPTS = {};
const TRANSFORM = (p) => {
  const target = (OPTS[p.projectHandle] || {}).target;
  if (p.fileName.endsWith(".tplx")) {
    if (process.env.TCM_TPL_STRING) return { extension: ".ts", text: 'export const T = "not-a-number";\\n' };
    if (process.env.TCM_MODE === "target")
      return { extension: ".ts", text: "export const T = " + (process.env.TCM_FORCE_WRONG ? 2 : target) + " as const;\\n" };
    return { extension: ".ts", text: "export const T: number = 111;\\n" };
  }
  const src = /src="([^"]+)"/.exec(p.content);       // read the CARRIER's own bytes
  if (process.env.TCM_MODE === "target") {
    const want = p.fileName.includes("/a/") ? 2 : 99;
    return { extension: ".ts", text: 'import { T } from "../shared/tpl.tplx";\\nexport const chk: ' + want + ' = T;\\n' };
  }
  if (!src) return { extension: ".ts", text: "export const C: number = 1;\\n" };
  const spec = process.env.TCM_SPEC || src[1];
  return { extension: ".ts",
    text: 'import { T } from "' + spec + '";\\nexport const C: number = T;\\nexport const SRC = ' + JSON.stringify(src[1]) + ';\\n' };
};
let buf = Buffer.alloc(0);
process.stdin.on("data", (c) => { buf = Buffer.concat([buf, c]);
  for (;;) { const sep = buf.indexOf("\\r\\n\\r\\n"); if (sep === -1) return;
    const m = /Content-Length: (\\d+)/i.exec(buf.subarray(0, sep).toString("utf8")); if (!m) return;
    const len = Number(m[1]); if (buf.length < sep + 4 + len) return;
    const msg = JSON.parse(buf.subarray(sep + 4, sep + 4 + len).toString("utf8"));
    buf = buf.subarray(sep + 4 + len);
    log({ method: msg.method, params: msg.params });
    if (msg.method === "openProject") OPTS[msg.params.projectHandle] = msg.params.compilerOptions;
    if (msg.method === "initialize") send({ jsonrpc: "2.0", id: msg.id, result: { name: "stub-mapper",
      version: "1.0.0", diagnosticSource: "stub-mapper", positionEncoding: "utf-8", capabilities: {} } });
    else if (msg.method === "transform") send({ jsonrpc: "2.0", id: msg.id, result: TRANSFORM(msg.params) });
    else if (msg.id !== undefined) send({ jsonrpc: "2.0", id: msg.id, result: {} }); } });
`;

function fixture(files) {
  const root = mkdtempSync(join(tmpdir(), "tcm0-eu-"));
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
  writeFileSync(join(root, "node_modules", "stub-mapper", "mapper.mjs"), MAPPER);
  for (const [n, c] of Object.entries(files)) {
    const p = join(root, n);
    mkdirSync(dirname(p), { recursive: true });
    writeFileSync(p, c);
  }
  return root;
}
function tsc(root, args, env = {}) {
  const log = join(root, "frames.log");
  writeFileSync(log, "");
  const r = spawnSync(exe, args, {
    cwd: root,
    encoding: "utf8",
    timeout: 120000,
    env: { ...process.env, TCM_LOG: log, ...env },
  });
  const frames = readFileSync(log, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((l) => JSON.parse(l));
  return {
    status: r.status,
    out: `${r.stdout || ""}${r.stderr || ""}`.replace(/\s+/g, " ").trim(),
    opens: frames.filter((f) => f.method === "openProject").map((f) => f.params),
    xforms: frames.filter((f) => f.method === "transform").map((f) => f.params),
    root,
  };
}
const MAPPERS = [{ package: "stub-mapper", extensions: [".vuex", ".tplx"] }];

let fails = 0;
const check = (label, cond, detail) => {
  if (cond) console.log(`ok   ${label}`);
  else {
    fails++;
    console.log(`FAIL ${label}\n       ${detail}`);
  }
};
const base = (p) => p.split("/").slice(-2).join("/");
const find = (xs, suf) => xs.filter((x) => x.fileName.endsWith(suf));

// =============================================================================
// 1. TRANSFORM INPUT IDENTITY
// =============================================================================
console.log("\n--- 1. transform input identity ---");
{
  const root = fixture({
    "tsconfig.json": JSON.stringify({
      compilerOptions: { noEmit: true, strict: true },
      contentMappers: MAPPERS,
      include: ["*.vuex"],
    }), // ext/ deliberately NOT included
    "comp.vuex": CARRIER_BYTES,
    "ext/thing.tplx": TPL_BYTES,
  });
  const r = tsc(root, ["--project", ".", "--runExternalCode"]);
  check("1.0 program type-checks", r.status === 0, `status=${r.status} out=${r.out}`);
  check(
    "1.1 exactly two transforms (carrier + external unit)",
    r.xforms.length === 2,
    JSON.stringify(r.xforms.map((x) => base(x.fileName))),
  );
  const car = find(r.xforms, "comp.vuex")[0],
    tpl = find(r.xforms, "thing.tplx")[0];
  check("1.2 external unit received a transform of its own", !!tpl, "no .tplx transform frame");
  // rival hypotheses encoded as the injection:
  const expectedTplContent = INJECT === "input" ? CARRIER_BYTES : TPL_BYTES;
  check(
    "1.3 carrier's `content` is the CARRIER's own bytes, byte-exact",
    car && car.content === CARRIER_BYTES,
    JSON.stringify(car && car.content),
  );
  check(
    "1.4 external unit's `content` is the EXTERNAL FILE's own bytes, byte-exact",
    tpl && tpl.content === expectedTplContent,
    JSON.stringify(tpl && tpl.content),
  );
  check(
    "1.5 external `content` carries NO carrier bytes (not the referencing file)",
    tpl && !tpl.content.includes("QQQ-CARRIER-OWN-BYTES-QQQ"),
    JSON.stringify(tpl && tpl.content),
  );
  check(
    "1.6 external `content` is not a concatenation (length == own file length)",
    tpl && tpl.content.length === TPL_BYTES.length,
    `got ${tpl && tpl.content.length}, own=${TPL_BYTES.length}, concat=${TPL_BYTES.length + CARRIER_BYTES.length}`,
  );
  check(
    "1.7 carrier `content` carries NO external bytes",
    car && !car.content.includes("ZZZ-EXTERNAL-UNIT-OWN-BYTES-ZZZ"),
    JSON.stringify(car && car.content),
  );
  rmSync(root, { recursive: true, force: true });
}
{
  // non-vacuity: the external unit's emitted TEXT really feeds the carrier's type check
  const root = fixture({
    "tsconfig.json": JSON.stringify({
      compilerOptions: { noEmit: true, strict: true },
      contentMappers: MAPPERS,
      include: ["*.vuex"],
    }),
    "comp.vuex": CARRIER_BYTES,
    "ext/thing.tplx": TPL_BYTES,
  });
  const r = tsc(root, ["--project", ".", "--runExternalCode"], { TCM_TPL_STRING: "1" });
  check(
    "1.8 NON-VACUITY: external unit emitting a string breaks the carrier (TS2322)",
    r.status !== 0 && r.out.includes("TS2322") && r.out.includes("comp.vuex"),
    `status=${r.status} out=${r.out}`,
  );
  rmSync(root, { recursive: true, force: true });
}

// =============================================================================
// 2. PROJECT IDENTITY  +  the reachability mechanism
// =============================================================================
console.log("\n--- 2. project identity / reachability ---");
{
  // 2a. outside `include`, reached ONLY by the mapper-emitted import
  const root = fixture({
    "tsconfig.json": JSON.stringify({
      compilerOptions: { noEmit: true, strict: true },
      contentMappers: MAPPERS,
      include: ["*.vuex"],
    }),
    "comp.vuex": CARRIER_BYTES,
    "ext/thing.tplx": TPL_BYTES,
  });
  const r = tsc(root, ["--project", ".", "--runExternalCode"]);
  const car = find(r.xforms, "comp.vuex")[0],
    tpl = find(r.xforms, "thing.tplx")[0];
  check(
    "2a.1 exactly one project opened",
    r.opens.length === 1,
    JSON.stringify(r.opens.map((o) => o.projectHandle)),
  );
  check("2a.2 external unit outside `include` IS transformed", !!tpl, "not transformed");
  const sameHandle = car && tpl && car.projectHandle === tpl.projectHandle;
  check(
    "2a.3 external unit runs under the SAME projectHandle as the carrier",
    INJECT === "project" ? !sameHandle : sameHandle,
    `carrier=${car && car.projectHandle} external=${tpl && tpl.projectHandle}`,
  );
  rmSync(root, { recursive: true, force: true });
}
{
  // 2b. the specifier must carry the mapped extension -> the mapper's own output decides reachability
  for (const [spec, want] of [
    ["./ext/thing.tplx", 0],
    ["./ext/thing", 2],
    ["./ext/thing.js", 2],
  ]) {
    const root = fixture({
      "tsconfig.json": JSON.stringify({
        compilerOptions: { noEmit: true, strict: true },
        contentMappers: MAPPERS,
        include: ["*.vuex"],
      }),
      "comp.vuex": CARRIER_BYTES,
      "ext/thing.tplx": TPL_BYTES,
    });
    const r = tsc(root, ["--project", ".", "--runExternalCode"], { TCM_SPEC: spec });
    const reached = find(r.xforms, "thing.tplx").length > 0;
    check(
      `2b specifier ${spec} -> ${want === 0 ? "resolves + transforms" : "TS2307, no transform"}`,
      want === 0
        ? r.status === 0 && reached
        : r.status !== 0 && r.out.includes("TS2307") && !reached,
      `status=${r.status} reached=${reached} out=${r.out.slice(0, 160)}`,
    );
    rmSync(root, { recursive: true, force: true });
  }
}
{
  // 2c. NEITHER included NOR imported -> never transformed (demand-driven)
  const root = fixture({
    "tsconfig.json": JSON.stringify({
      compilerOptions: { noEmit: true, strict: true },
      contentMappers: MAPPERS,
      include: ["*.vuex"],
    }),
    "comp.vuex": `<!--QQQ-CARRIER-OWN-BYTES-QQQ-->\n`, // no src= attribute -> no import emitted
    "ext/thing.tplx": TPL_BYTES,
  });
  const r = tsc(root, ["--project", ".", "--runExternalCode"]);
  check(
    "2c.1 unreferenced, unincluded external unit is NEVER transformed",
    r.status === 0 && find(r.xforms, "thing.tplx").length === 0,
    `status=${r.status} xforms=${JSON.stringify(r.xforms.map((x) => base(x.fileName)))}`,
  );
  rmSync(root, { recursive: true, force: true });
}
{
  // 2d. `include` membership alone also makes it reachable (second, independent mechanism)
  const root = fixture({
    "tsconfig.json": JSON.stringify({
      compilerOptions: { noEmit: true, strict: true },
      contentMappers: MAPPERS,
      include: ["*.vuex", "ext/*.tplx"],
    }),
    "comp.vuex": `<!--QQQ-CARRIER-OWN-BYTES-QQQ-->\n`,
    "ext/thing.tplx": TPL_BYTES,
  });
  const r = tsc(root, ["--project", ".", "--runExternalCode"]);
  check(
    "2d.1 `include` membership alone transforms it, with no reference at all",
    r.status === 0 && find(r.xforms, "thing.tplx").length === 1,
    `status=${r.status}`,
  );
  rmSync(root, { recursive: true, force: true });
}

// =============================================================================
// 3. CONFIGURATION IDENTITY  (+ two configured projects both covering the file)
// =============================================================================
console.log("\n--- 3. configuration identity ---");
{
  const proj = (name, target) =>
    JSON.stringify({
      compilerOptions: {
        composite: true,
        strict: true,
        target,
        rootDir: "..",
        outDir: `../out/${name}`,
      },
      contentMappers: MAPPERS,
      include: ["*.vuex", "../shared/*.tplx"],
    });
  const root = fixture({
    "tsconfig.json": JSON.stringify({ files: [], references: [{ path: "./a" }, { path: "./b" }] }),
    "a/tsconfig.json": proj("a", "es2015"),
    "b/tsconfig.json": proj("b", "esnext"),
    "a/comp.vuex": `<!--A-->\n`,
    "b/comp.vuex": `<!--B-->\n`,
    "shared/tpl.tplx": TPL_BYTES,
  });
  const env = { TCM_MODE: "target", ...(INJECT === "mapper" ? { TCM_FORCE_WRONG: "1" } : {}) };
  const r = tsc(root, ["--build", ".", "--runExternalCode"], env);
  check(
    "3.0 both projects type-check",
    r.status === 0,
    `status=${r.status} out=${r.out.slice(0, 300)}`,
  );
  const byCfg = Object.fromEntries(r.opens.map((o) => [base(o.configFileName), o]));
  check(
    "3.1 two projects opened, distinct handles",
    r.opens.length === 2 && r.opens[0].projectHandle !== r.opens[1].projectHandle,
    JSON.stringify(r.opens.map((o) => [base(o.configFileName), o.projectHandle])),
  );
  check(
    "3.2 each project reports its OWN compilerOptions.target",
    byCfg["a/tsconfig.json"].compilerOptions.target === 2 &&
      byCfg["b/tsconfig.json"].compilerOptions.target === 99,
    JSON.stringify(r.opens.map((o) => [base(o.configFileName), o.compilerOptions.target])),
  );
  const tpls = find(r.xforms, "tpl.tplx");
  check(
    "3.3 the ONE shared external unit is transformed ONCE PER OWNING PROJECT",
    tpls.length === 2,
    `${tpls.length} transforms: ${JSON.stringify(tpls.map((t) => t.projectHandle))}`,
  );
  const hs = new Set(tpls.map((t) => t.projectHandle));
  const distinct =
    hs.size === 2 &&
    hs.has(byCfg["a/tsconfig.json"].projectHandle) &&
    hs.has(byCfg["b/tsconfig.json"].projectHandle);
  check(
    "3.4 its two transforms carry the two DISTINCT owning-project handles",
    INJECT === "config" ? !distinct : distinct,
    `handles=${JSON.stringify([...hs])} a=${byCfg["a/tsconfig.json"].projectHandle} b=${byCfg["b/tsconfig.json"].projectHandle}`,
  );
  // compile-visible: each carrier asserts the target literal of ITS OWN project against
  // the value the external unit emitted under that transform's handle. Both green => the
  // external unit's config is resolved per referencing project, not once globally.
  check(
    "3.5 compile-visible: per-project option reaches the external unit's transform",
    r.status === 0 && !r.out.includes("TS2322"),
    r.out.slice(0, 300),
  );
  rmSync(root, { recursive: true, force: true });
}

if (fails > 0) {
  process.exitCode = 1;
}
finish();
