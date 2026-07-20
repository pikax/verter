/**
 * Capture WASM-produced carrier fixtures for the in-context LanguageService
 * guards.
 *
 * Runs the REAL WASM host (packages/wasm/wasm — the same binary the playground
 * ships) over a small set of committed carrier sources and snapshots the three
 * generated surfaces per source:
 *
 *   - IDE carrier      — `getIde(id, profile)`        → `Comp.vue.tsx`
 *   - declaration      — `getPublicApi(id, "declaration")` → `Comp.d.vue.ts`
 *   - API carrier      — `getPublicApi(id)`           → `Comp.vue.verter.ts`
 *
 * The snapshot is committed at `src/editor/__fixtures__/wasm-carriers.json` so
 * the guards stay hermetic (no live WASM host load per test). Regenerate after
 * a compiler-output change with:
 *
 *   node scripts/capture-wasm-carrier-fixtures.mjs
 */
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const thisDir = dirname(fileURLToPath(import.meta.url));
const wasmJs = resolve(thisDir, "../../wasm/wasm/verter_wasm.js");
const wasmBin = resolve(thisDir, "../../wasm/wasm/verter_wasm_bg.wasm");
const outFile = resolve(thisDir, "../src/editor/__fixtures__/wasm-carriers.json");

const wasmModule = await import(pathToFileURL(wasmJs).href);
await wasmModule.default({ module_or_path: readFileSync(wasmBin) });

/** The committed fixture sources. Key = fixture id in the JSON. */
const FIXTURES = [
  {
    key: "compVue",
    filename: "Comp.vue",
    fileKind: "vue",
    source: `<script setup lang="ts">
defineProps<{ count: number }>()
</script>
<template><div>{{ count }}</div></template>`,
  },
  {
    key: "compVueEdited",
    filename: "Comp.vue",
    fileKind: "vue",
    source: `<script setup lang="ts">
defineProps<{ count: string, label: string }>()
</script>
<template><div>{{ count }}{{ label }}</div></template>`,
  },
  {
    key: "astralVue",
    filename: "Astral.vue",
    fileKind: "vue",
    source: `<script setup lang="ts">
const msg = "ok"
</script>
<template><div>🎉🎉{{ msg }}</div></template>`,
  },
  {
    key: "compSvelte",
    filename: "Comp.svelte",
    fileKind: "svelte",
    source: `<script lang="ts">
  let { count }: { count: number } = $props();
</script>
<div>{count}</div>`,
  },
];

function surface(response) {
  if (!response || typeof response.code !== "string") return null;
  return {
    code: response.code,
    sourceMap: typeof response.sourceMap === "string" ? response.sourceMap : null,
    destructuredBlock: response.destructuredBlock ?? null,
  };
}

function publicApiSurface(result) {
  if (result.error) {
    throw Object.assign(
      new Error(`public API projection failed: ${result.error.code}/${result.error.detailCode}`),
      result.error,
    );
  }
  return surface(result.value);
}

function capture({ filename, fileKind, source }) {
  // A fresh host per fixture keeps every entry independent of capture order.
  const host = new wasmModule.VerterHost({
    devMode: true,
    compileErrorPolicy: "devServeLastKnownGood",
    maxProfilesPerFile: 8,
  });
  const profile = { filename, sourceMap: true, target: "ide", forceJs: true };
  host.upsert({ inputId: filename, source, fileKind, aliases: [], compileProfile: profile });

  let ide = null;
  let ideUnavailable = null;
  try {
    host.ensureIdeCompiled(filename, profile);
    ide = surface(host.getIde(filename, profile));
  } catch (err) {
    // An IDE compile failure is recorded explicitly (`ideUnavailable`) so the
    // fixture stays honest about what the host produced — a guard asserting
    // IDE parity fails on the recorded gap instead of a silent null.
    ideUnavailable = String(err?.message ?? err).split("\n")[0];
  }
  const decl = publicApiSurface(host.getPublicApi(filename, "declaration"));
  const api = publicApiSurface(host.getPublicApi(filename));
  // No explicit `host.free()`: if an IDE compile trapped, the host borrow is
  // poisoned and `free` would throw; the short-lived capture process reclaims
  // everything on exit.
  return { filename, source, ide, ideUnavailable, decl, api };
}

const out = {
  $comment:
    "Committed snapshot of REAL WASM-host carrier output (getIde / getPublicApi). " +
    "Captured once by scripts/capture-wasm-carrier-fixtures.mjs so guards run hermetically. " +
    "Regenerate with: node scripts/capture-wasm-carrier-fixtures.mjs",
};
for (const fixture of FIXTURES) {
  out[fixture.key] = capture(fixture);
  const entry = out[fixture.key];
  console.log(
    `${fixture.key}: ide=${entry.ide ? `${entry.ide.code.length}b` : `UNAVAILABLE (${entry.ideUnavailable})`}` +
      ` decl=${entry.decl ? `${entry.decl.code.length}b` : "MISSING"}` +
      ` api=${entry.api ? `${entry.api.code.length}b` : "MISSING"}`,
  );
}

mkdirSync(dirname(outFile), { recursive: true });
writeFileSync(outFile, `${JSON.stringify(out, null, 2)}\n`);
console.log(`wrote ${outFile}`);
