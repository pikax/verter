// GATE 5 — the PRODUCTION companion-identity proof (the load-bearing P0).
//
// The bare-import membership mechanism (GATE 4 / the 3-strategy probe) established
// that tsgo resolves `import "./Comp.vue"` by APPENDING `.tsx`/`.ts` to the FULL
// basename `Comp.vue` and probing `Comp.vue.tsx` / `Comp.vue.ts`. There is NO
// module-resolution-map endpoint in the shipped `--api`. Therefore whatever path
// Verter serves the carrier at IS the engine file identity, and it MUST be a path
// tsgo's appended-extension probe actually reaches.
//
// This script DECIDES the production component-carrier companion identity against
// the REAL user-installed tsgo, falsifying the wrong candidate and proving the
// right one. All findings are empirical (assert the actual tsgo behaviour):
//
//   A. `.verter.` infix is REJECTED for a bare-import component carrier:
//      serving the carrier at `Comp.vue.verter.tsx` does NOT satisfy
//      `import "./Comp.vue"` under default resolution — tsgo never probes a
//      `.verter.` infix -> TS2307. (The doc's prior `.verter.` component identity
//      is empirically refuted.)
//   B. `.vue.tsx` (the production component identity) DOES satisfy the bare `.vue`
//      import and the companion's exported types flow into the plain `.ts`.
//   C. The Svelte hard case (realistic layout): `import W from "./Widget.svelte"`
//      resolves to the `Widget.svelte.tsx` component carrier WHILE a REAL Svelte
//      rune module (`*.svelte.ts`) is present in the SAME directory and stays
//      independently resolvable — correct types BOTH ways, no clash. Proves
//      `.svelte.tsx` is BOTH probe-compatible AND collision-free for the component
//      carrier (it is `.tsx`; rune modules are `.svelte.ts`/`.svelte.js`).
//   D. tsgo's actual `.svelte` appended-extension PROBE ORDER, recorded
//      empirically: `.svelte.ts` is probed BEFORE `.svelte.tsx`. Consequence
//      (also asserted): a component carrier and a rune module that share the EXACT
//      SAME stem (`Widget.svelte.tsx` + `Widget.svelte.ts`) DO collide on the bare
//      `./Widget.svelte` import (the rune `.ts` shadows the carrier `.tsx`). This
//      is the documented same-stem edge AND the reason the REDIRECT-reached `.ts`
//      API carrier must NOT use a bare `.svelte.ts` identity. The `.verter.` infix
//      does not fix this edge either (a `.svelte.verter.tsx` is unreachable by the
//      bare `./Widget.svelte` probe regardless).
//
// Run: NM_BASE="$PWD" TSGO_PATH="<tsgo exe>" node tools/tsgo-api-gate/companion-identity.mjs
// (run-gate.mjs discovers TSGO_PATH and runs this with the other scripts).

import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.join(ROOT, "fixture");
const norm = (p) => p.replace(/\\/g, "/");
const require = createRequire(import.meta.url);
const opts = { paths: (process.env.NM_BASE || "").split(path.delimiter).filter(Boolean) };
const sourcePkgs = [process.env.TS7_SOURCE, "typescript", "@typescript/native-preview"].filter(
  Boolean,
);
// Import via the PUBLIC `<pkg>/unstable/sync` export (honouring the package
// `exports` map) — the same public surface a real consumer uses.
let syncApiPath;
for (const p of sourcePkgs) {
  try {
    syncApiPath = require.resolve(`${p}/unstable/sync`, opts);
    break;
  } catch {
    /* try next */
  }
}
if (!syncApiPath)
  throw new Error(
    `could not resolve a TS>=7 sync API (<pkg>/unstable/sync) from ${JSON.stringify(sourcePkgs)}`,
  );
const { API } = await import(pathToFileURL(syncApiPath).href);

const TSCONFIG = path.join(FIXTURE, "tsconfig.json");
const DIR = path.join(FIXTURE, "src", "components");

// ---- Helpers ---------------------------------------------------------------
function diagSummary(diags) {
  return diags.map((d) => ({ code: d.code, text: d.text }));
}
function hasCode(diags, code) {
  return diags.some((d) => d.code === code);
}
function offsetOf(content, needle, extra = 0) {
  const i = content.indexOf(needle);
  if (i === -1) throw new Error(`needle not found: ${needle}`);
  return i + extra;
}

const results = {};
function record(name, pass, detail) {
  results[name] = { pass, detail };
  console.log(`[${pass ? "PASS" : "FAIL"}] ${name}: ${detail}`);
}

// Build a sparse FS overlay over a fixed set of off-disk files (abs path -> content),
// injecting their basenames into the shared dir enumeration so `include` discovers them.
function makeOverlay(offDiskMap) {
  const map = new Map(Object.entries(offDiskMap).map(([p, c]) => [norm(p), c]));
  const dirNorm = norm(DIR);
  const bases = [...map.keys()].map((p) => path.basename(p));
  return {
    readFile(f) {
      const v = map.get(norm(f));
      return v === undefined ? undefined : v;
    },
    fileExists(f) {
      return map.has(norm(f)) ? true : undefined;
    },
    getAccessibleEntries(d) {
      if (norm(d) !== dirNorm) return undefined;
      let real;
      try {
        const e = fs.readdirSync(d, { withFileTypes: true });
        real = {
          files: e.filter((x) => x.isFile()).map((x) => x.name),
          directories: e.filter((x) => x.isDirectory()).map((x) => x.name),
        };
      } catch {
        real = { files: [], directories: [] };
      }
      for (const b of bases) if (!real.files.includes(b)) real.files.push(b);
      return real;
    },
  };
}

function typeAtStr(project, file, offset) {
  const t = project.checker.getTypeAtPosition(file, offset);
  if (!t) return "(no type)";
  try {
    return project.checker.typeToString(t);
  } catch (e) {
    return `(typeToString threw: ${e.message})`;
  }
}

console.log(
  "=== GATE 5: production companion-identity proof (.verter. vs .vue.tsx / .svelte.tsx) ===",
);
console.log("fixture root:", norm(FIXTURE));

// ============================================================================
// A. `.verter.` INFIX IS REJECTED for a bare-import component carrier.
// ============================================================================
{
  const consumer = path.join(DIR, "ConsumerA.ts");
  const verterCompanion = path.join(DIR, "CompA.vue.verter.tsx"); // the .verter. infix identity
  const CONSUMER_SRC = `import { widget } from "./CompA.vue";\nexport const a: string = widget.label;\n`;
  const COMPANION_SRC = `export const widget = { label: "hi" as string };\n`;
  const api = new API({
    tsserverPath: process.env.TSGO_PATH,
    cwd: FIXTURE,
    fs: makeOverlay({ [consumer]: CONSUMER_SRC, [verterCompanion]: COMPANION_SRC }),
  });
  try {
    const snap = api.updateSnapshot({
      openProject: TSCONFIG,
      fileChanges: { changed: [consumer, verterCompanion] },
    });
    const project = snap.getProject(TSCONFIG);
    const diags = project.program.getSemanticDiagnostics(consumer);
    console.log(
      "[A] ConsumerA diags (serving .verter.tsx only):",
      JSON.stringify(diagSummary(diags)),
    );
    record(
      "verter_infix_rejected_for_bare_import",
      hasCode(diags, 2307),
      hasCode(diags, 2307)
        ? "CompA.vue.verter.tsx does NOT satisfy `import ./CompA.vue` (tsgo never probes a .verter. infix) -> TS2307. The .verter. component identity is REJECTED."
        : "expected TS2307 (the .verter. infix is unreachable by tsgo's basename probe), got: " +
            JSON.stringify(diagSummary(diags)),
    );
    snap.dispose();
  } finally {
    api.close();
  }
}

// ============================================================================
// B. `.vue.tsx` (PRODUCTION component identity) satisfies the bare `.vue` import
//    and types flow.
// ============================================================================
{
  const consumer = path.join(DIR, "ConsumerB.ts");
  const companion = path.join(DIR, "CompB.vue.tsx"); // production component identity
  const CONSUMER_SRC = `import { widget } from "./CompB.vue";\nconst lbl = widget.label;\nexport const b: string = lbl;\n`;
  const COMPANION_SRC = `export const widget = { label: "hi" as string };\n`;
  const api = new API({
    tsserverPath: process.env.TSGO_PATH,
    cwd: FIXTURE,
    fs: makeOverlay({ [consumer]: CONSUMER_SRC, [companion]: COMPANION_SRC }),
  });
  try {
    const snap = api.updateSnapshot({
      openProject: TSCONFIG,
      fileChanges: { changed: [consumer, companion] },
    });
    const project = snap.getProject(TSCONFIG);
    const diags = project.program.getSemanticDiagnostics(consumer);
    console.log("[B] ConsumerB diags (serving .vue.tsx):", JSON.stringify(diagSummary(diags)));
    // Type at `lbl` binding (whose initializer is widget.label : string).
    const lblTy = typeAtStr(
      project,
      consumer,
      offsetOf(CONSUMER_SRC, "const lbl = widget.label;", "const ".length),
    );
    record(
      "vue_tsx_resolves_and_types_flow",
      !hasCode(diags, 2307) && diags.length === 0 && lblTy === "string",
      diags.length === 0
        ? "CompB.vue.tsx satisfies `import ./CompB.vue` with ZERO diags; widget.label type = " +
            JSON.stringify(lblTy) +
            " flows into the plain .ts. PRODUCTION component identity is .vue.tsx."
        : "unexpected: diags=" + JSON.stringify(diagSummary(diags)) + " lblTy=" + lblTy,
    );
    snap.dispose();
  } finally {
    api.close();
  }
}

// ============================================================================
// C. SVELTE HARD CASE (realistic layout) — `.svelte.tsx` component carrier COEXISTS
//    with a REAL `*.svelte.ts` rune module in the SAME directory; correct types
//    both ways, no clash. Rune modules use a DISTINCT stem from the component
//    (the real Svelte convention), so the bare `./Widget.svelte` import reaches the
//    `.svelte.tsx` carrier while `./state.svelte` reaches the `.svelte.ts` rune.
// ============================================================================
{
  const consumer = path.join(DIR, "ConsumerC.ts");
  const svelteComponentCarrier = path.join(DIR, "Widget.svelte.tsx"); // component carrier
  const svelteRuneModule = path.join(DIR, "state.svelte.ts"); // REAL rune module (SelfFile, distinct stem)

  const COMPONENT_CARRIER_SRC =
    `export interface WidgetProps { label: string }\n` +
    `declare const Widget: (props: WidgetProps) => unknown;\n` +
    `export default Widget;\n`;
  const RUNE_MODULE_SRC =
    `export const count = { value: 0 as number };\n` +
    `export function increment(): void { count.value++; }\n`;
  const CONSUMER_SRC =
    `import Widget, { type WidgetProps } from "./Widget.svelte";\n` + // -> component carrier (.svelte.tsx)
    `import { count, increment } from "./state.svelte";\n` + // -> REAL rune module (.svelte.ts)
    `const props: WidgetProps = { label: "hi" };\n` +
    `const c = count.value;\n` +
    `increment();\n` +
    `export const out = { props, c };\n` +
    `export type ComponentIsCallable = typeof Widget;\n`;
  const api = new API({
    tsserverPath: process.env.TSGO_PATH,
    cwd: FIXTURE,
    fs: makeOverlay({
      [consumer]: CONSUMER_SRC,
      [svelteComponentCarrier]: COMPONENT_CARRIER_SRC,
      [svelteRuneModule]: RUNE_MODULE_SRC,
    }),
  });
  try {
    const snap = api.updateSnapshot({
      openProject: TSCONFIG,
      fileChanges: { changed: [consumer, svelteComponentCarrier, svelteRuneModule] },
    });
    const project = snap.getProject(TSCONFIG);
    const diags = project.program.getSemanticDiagnostics(consumer);
    console.log(
      "[C] ConsumerC diags (svelte component carrier + rune module):",
      JSON.stringify(diagSummary(diags)),
    );

    const noModuleErr = !hasCode(diags, 2307);
    // WidgetProps.label is string (component carrier resolved).
    const labelTy = typeAtStr(
      project,
      consumer,
      offsetOf(CONSUMER_SRC, `label: "hi"`, "label: ".length - "label: ".length),
    );
    const propLabelTy = typeAtStr(
      project,
      consumer,
      offsetOf(CONSUMER_SRC, `{ label: "hi" }`, "{ ".length),
    );
    // count.value is number (rune module resolved).
    const cTy = typeAtStr(
      project,
      consumer,
      offsetOf(CONSUMER_SRC, "const c = count.value;", "const ".length),
    );

    record(
      "svelte_component_carrier_and_rune_module_coexist",
      noModuleErr && diags.length === 0 && propLabelTy === "string" && cTy === "number",
      diags.length === 0
        ? "BOTH resolve cleanly: ./Widget.svelte -> Widget.svelte.tsx component carrier (WidgetProps.label=" +
            JSON.stringify(propLabelTy) +
            "), ./state.svelte -> REAL .svelte.ts rune module (count.value=" +
            JSON.stringify(cTy) +
            "). .svelte.tsx component carrier and .svelte.ts rune module COEXIST with correct types both ways — no clash."
        : "unexpected: diags=" +
            JSON.stringify(diagSummary(diags)) +
            " propLabelTy=" +
            propLabelTy +
            " cTy=" +
            cTy,
    );
    record(
      "svelte_bare_import_targets_tsx_component_carrier",
      noModuleErr && propLabelTy === "string",
      propLabelTy === "string"
        ? "bare ./Widget.svelte resolved to the .svelte.tsx COMPONENT carrier (WidgetProps in scope), distinct from the .svelte.ts rune module reached via ./state.svelte."
        : "bare ./Widget.svelte did not pick the component carrier: propLabelTy=" + propLabelTy,
    );
    snap.dispose();
  } finally {
    api.close();
  }
}

// ============================================================================
// D. EMPIRICAL PROBE — tsgo's `.svelte` appended-extension PROBE ORDER, and the
//    SAME-STEM collision consequence. Two assertions:
//    (D1) probe order: bare `./Probe.svelte` with BOTH .svelte.ts and .svelte.tsx
//         served picks `.svelte.ts` first (records the order).
//    (D2) same-stem collision: a `Widget.svelte.tsx` carrier + `Widget.svelte.ts`
//         rune (same stem) -> bare `./Widget.svelte` hits the `.ts` rune (TS1192
//         no default export), shadowing the carrier. Documented edge; the carrier
//         identity stays `.svelte.tsx` (it wins whenever no same-stem `.svelte.ts`
//         exists, the normal case), and the REDIRECT-reached `.ts` API carrier must
//         avoid a bare `.svelte.ts` identity for exactly this reason.
// ============================================================================
let svelteProbeWinner = "?";
{
  const consumer = path.join(DIR, "ConsumerD.ts");
  const tsCandidate = path.join(DIR, "Probe.svelte.ts");
  const tsxCandidate = path.join(DIR, "Probe.svelte.tsx");
  const CONSUMER_SRC = `import { which } from "./Probe.svelte";\nexport const picked = which;\n`;
  const api = new API({
    tsserverPath: process.env.TSGO_PATH,
    cwd: FIXTURE,
    fs: makeOverlay({
      [consumer]: CONSUMER_SRC,
      [tsCandidate]: `export const which = "ts" as const;\n`,
      [tsxCandidate]: `export const which = "tsx" as const;\n`,
    }),
  });
  try {
    const snap = api.updateSnapshot({
      openProject: TSCONFIG,
      fileChanges: { changed: [consumer, tsCandidate, tsxCandidate] },
    });
    const project = snap.getProject(TSCONFIG);
    const ty = typeAtStr(
      project,
      consumer,
      offsetOf(CONSUMER_SRC, "export const picked = which;", "export const picked = ".length),
    );
    svelteProbeWinner = ty.replace(/"/g, "");
    console.log(
      `[D1] tsgo .svelte probe order: bare ./Probe.svelte (both .svelte.ts + .svelte.tsx served) -> picked "${svelteProbeWinner}" (type ${ty})`,
    );
    record(
      "svelte_probe_order_recorded",
      svelteProbeWinner === "ts" || svelteProbeWinner === "tsx",
      "RECORDED: tsgo probes .svelte." +
        svelteProbeWinner +
        " FIRST for a bare .svelte import. (.svelte.ts before .svelte.tsx => a bare .svelte.ts API-carrier identity would collide with a rune module; the REDIRECT-reached .ts API carrier must use a non-bare-probed identity.)",
    );
    snap.dispose();
  } finally {
    api.close();
  }
}
{
  const consumer = path.join(DIR, "ConsumerD2.ts");
  const carrier = path.join(DIR, "Same.svelte.tsx");
  const rune = path.join(DIR, "Same.svelte.ts");
  const CONSUMER_SRC = `import Comp from "./Same.svelte";\nexport const out = Comp;\n`;
  const api = new API({
    tsserverPath: process.env.TSGO_PATH,
    cwd: FIXTURE,
    fs: makeOverlay({
      [consumer]: CONSUMER_SRC,
      [carrier]: `declare const W: () => unknown;\nexport default W;\n`,
      [rune]: `export const x = 1;\n`, // a rune module: NO default export
    }),
  });
  try {
    const snap = api.updateSnapshot({
      openProject: TSCONFIG,
      fileChanges: { changed: [consumer, carrier, rune] },
    });
    const project = snap.getProject(TSCONFIG);
    const diags = project.program.getSemanticDiagnostics(consumer);
    console.log("[D2] same-stem collision diags:", JSON.stringify(diagSummary(diags)));
    // The .ts rune (no default export) shadows the .tsx carrier -> TS1192.
    record(
      "svelte_same_stem_ts_rune_shadows_tsx_carrier",
      hasCode(diags, 1192),
      hasCode(diags, 1192)
        ? "DOCUMENTED EDGE confirmed: a same-stem Same.svelte.ts rune SHADOWS the Same.svelte.tsx carrier on `import ./Same.svelte` (TS1192 no default export). The .verter. infix would NOT fix this; the carrier identity stays .svelte.tsx (wins when no same-stem .svelte.ts exists)."
        : "expected TS1192 (same-stem .ts shadows .tsx), got: " +
            JSON.stringify(diagSummary(diags)),
    );
    snap.dispose();
  } finally {
    api.close();
  }
}

// ---- Verdict ----------------------------------------------------------------
const order = [
  "verter_infix_rejected_for_bare_import",
  "vue_tsx_resolves_and_types_flow",
  "svelte_component_carrier_and_rune_module_coexist",
  "svelte_bare_import_targets_tsx_component_carrier",
  "svelte_probe_order_recorded",
  "svelte_same_stem_ts_rune_shadows_tsx_carrier",
];
console.log("\n================= GATE 5 VERDICT =================");
let allPass = true;
for (const k of order) {
  const r = results[k];
  if (!r) {
    console.log(`[MISS] ${k}: (not reached)`);
    allPass = false;
    continue;
  }
  if (!r.pass) allPass = false;
}
console.log(
  `\nGATE 5 (production companion identity: .verter. REJECTED for the component carrier; .vue.tsx/.svelte.tsx PROVEN + collision-free; same-stem rune edge recorded): ${allPass ? "PASS" : "FAIL"}`,
);
process.exit(allPass ? 0 : 1);
