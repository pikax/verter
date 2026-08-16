#!/usr/bin/env node
// Executes the WASM transport's representative cases and prints the result as
// JSON on stdout, for a Rust-side comparison against the in-process host route.
//
// It also ENUMERATES the transport's exported surface from the BUILT ARTIFACT
// itself — the wasm-bindgen-generated module's own exports plus each exported
// class's prototype — never by reading source.
//
// Usage: node scripts/probe-transport-surface.mjs
// Exit codes: 0 = probed; 2 = the built module could not be loaded (an
// execution prerequisite, reported as such, never a pass).

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const wasmDir = path.join(here, "..", "wasm");
const jsEntry = path.join(wasmDir, "verter_wasm.js");
const wasmBinary = path.join(wasmDir, "verter_wasm_bg.wasm");

let module_;
try {
  // `pathToFileURL`, not a `file://` prefix: a Windows path (`C:\…`, backslash
  // separators) is not a valid file URL when concatenated.
  module_ = await import(pathToFileURL(jsEntry).href);
  // The `web` target's default export is the async initializer; feed it the
  // binary bytes directly so no fetch/URL machinery is involved.
  await module_.default({ module_or_path: fs.readFileSync(wasmBinary) });
} catch (error) {
  process.stdout.write(JSON.stringify({ loaded: false, error: String(error?.message ?? error) }));
  process.exit(2);
}

function enumerateSurface(module) {
  const surface = { moduleExports: [], classes: {} };
  for (const name of Object.keys(module).sort()) {
    surface.moduleExports.push(name);
    const value = module[name];
    if (typeof value === "function" && value.prototype !== undefined) {
      const methods = Object.getOwnPropertyNames(value.prototype)
        .filter((member) => member !== "constructor")
        .sort();
      if (methods.length > 0) surface.classes[name] = methods;
    }
  }
  return surface;
}

const SUPPORTED_SVELTE =
  '<script>\n  let count = $state(0);\n</script>\n\n<div class="root">{count}</div>\n\n<style>\n  .root { color: red; }\n</style>\n';
const VUE_SFC =
  "<script setup>\nconst props = defineProps({ label: { type: String, required: true } });\n</script>\n\n<template>\n  <button>{{ label }}</button>\n</template>\n";

function virtualFile(host, canonicalId, kind, compileProfile, index) {
  const nodeKind = index === undefined ? { kind } : { kind, index };
  try {
    const response = host.getVirtualFile({ canonicalId, nodeKind, compileProfile });
    // A MISSING node comes back as a null/absent response; a REFUSED one
    // throws. Both are recorded distinctly — collapsing them would hide which
    // one the transport produced.
    if (response === null || response === undefined) return { outcome: "missing" };
    return {
      outcome: "published",
      code: response.code,
      hasMap: response.sourceMap !== null && response.sourceMap !== undefined,
      lang: response.lang ?? null,
    };
  } catch (error) {
    return { outcome: "error", message: String(error?.message ?? error) };
  }
}

const results = { loaded: true, surface: enumerateSurface(module_), cases: {} };

// ── SUCCESS + the optional-product axis ────────────────────────────────────
{
  const host = new module_.VerterHost({});
  host.upsert({
    canonicalId: "/probe/Ok.svelte",
    inputId: "/probe/Ok.svelte",
    source: SUPPORTED_SVELTE,
    fileKind: "svelte",
  });
  results.cases.svelteMainWithMap = virtualFile(host, "/probe/Ok.svelte", "main", {
    isProduction: true,
    sourceMap: true,
    hmrStrategy: "none",
  });
  results.cases.svelteMainWithoutMap = virtualFile(host, "/probe/Ok.svelte", "main", {
    isProduction: true,
    sourceMap: false,
    hmrStrategy: "none",
  });
  results.cases.svelteStyleWithMap = virtualFile(
    host,
    "/probe/Ok.svelte",
    "style",
    { isProduction: true, sourceMap: true, hmrStrategy: "none" },
    0,
  );
  results.cases.svelteNodeList = host.listVirtualFiles("/probe/Ok.svelte");
}

// ── REFUSAL + artifact absence ─────────────────────────────────────────────
{
  const host = new module_.VerterHost({});
  host.upsert({
    canonicalId: "/probe/Server.svelte",
    inputId: "/probe/Server.svelte",
    source: SUPPORTED_SVELTE,
    fileKind: "svelte",
  });
  results.cases.svelteServerRefusal = virtualFile(host, "/probe/Server.svelte", "main", {
    isProduction: true,
    sourceMap: true,
    ssr: true,
    hmrStrategy: "none",
  });
  results.cases.svelteServerStyle = virtualFile(
    host,
    "/probe/Server.svelte",
    "style",
    { isProduction: true, sourceMap: true, ssr: true, hmrStrategy: "none" },
    0,
  );
}

// ── IDE/TSX: ensure + read, on the profile the LSP uses ────────────────────
{
  const host = new module_.VerterHost({});
  host.upsert({
    canonicalId: "/probe/Ide.svelte",
    inputId: "/probe/Ide.svelte",
    source: SUPPORTED_SVELTE,
    fileKind: "svelte",
  });
  const ideProfile = { target: "ide", sourceMap: true, hmrStrategy: "none" };
  try {
    results.cases.ensureIdeCompiled = {
      outcome: "ok",
      value: host.ensureIdeCompiled("/probe/Ide.svelte", ideProfile),
    };
  } catch (error) {
    results.cases.ensureIdeCompiled = {
      outcome: "error",
      message: String(error?.message ?? error),
    };
  }
  for (const [label, profile] of [
    ["getIdeWithMap", ideProfile],
    ["getIdeWithoutMap", { target: "ide", sourceMap: false, hmrStrategy: "none" }],
  ]) {
    try {
      const ide = host.getIde("/probe/Ide.svelte", profile);
      results.cases[label] =
        ide === null || ide === undefined
          ? { outcome: "missing" }
          : {
              outcome: "published",
              code: ide.code,
              hasMap: ide.sourceMap !== null && ide.sourceMap !== undefined,
              isJsx: ide.isJsx ?? null,
            };
    } catch (error) {
      results.cases[label] = { outcome: "error", message: String(error?.message ?? error) };
    }
  }
}

// ── PUBLIC API: option conversion of the `mode` argument ───────────────────
{
  const host = new module_.VerterHost({});
  host.upsert({
    canonicalId: "/probe/Api.vue",
    inputId: "/probe/Api.vue",
    source: VUE_SFC,
    fileKind: "vue",
  });
  for (const [label, mode] of [
    ["vuePublicApiDefault", undefined],
    ["vuePublicApiDeclaration", "declaration"],
  ]) {
    try {
      // The public-API result is a typed two-arm envelope: `value` carries the
      // rendered surface, `error` a typed projection failure. Both arms are
      // recorded so a silently-empty envelope cannot read as a success.
      const response = host.getPublicApi("/probe/Api.vue", mode);
      results.cases[label] = {
        outcome: response?.value ? "published" : response?.error ? "projection-error" : "empty",
        code: response?.value?.code ?? null,
        hasMap: response?.value?.sourceMap !== null && response?.value?.sourceMap !== undefined,
        error: response?.error ?? null,
      };
    } catch (error) {
      results.cases[label] = { outcome: "error", message: String(error?.message ?? error) };
    }
  }
}

// ── A typed refusal this transport owns: analysis is unavailable on wasm32 ──
{
  const host = new module_.VerterHost({});
  try {
    host.analyzeWithAudit("/probe/Ok.svelte");
    results.cases.analyzeWithAudit = { outcome: "published" };
  } catch (error) {
    results.cases.analyzeWithAudit = {
      outcome: "error",
      message: String(error?.message ?? error),
    };
  }
}

// ── AUDITED COMPILE: the same spelling the NAPI probe drives ───────────────
//
// `crates/verter_wasm/src/lib.rs:874` exposes `compileWithAudit`. It is driven
// on an audit-enabled host for both carriers so the two transports' answers to
// one request are comparable.
{
  const host = new module_.VerterHost({ auditEnabled: true });
  for (const [label, canonical, source, fileKind] of [
    ["auditedCompileVue", "/probe/AuditedW.vue", VUE_SFC, "vue"],
    ["auditedCompileSvelte", "/probe/AuditedW.svelte", SUPPORTED_SVELTE, "svelte"],
  ]) {
    host.upsert({ canonicalId: canonical, inputId: canonical, source, fileKind });
    try {
      // The binding hands back the encoded record as bytes, exactly as the
      // NAPI spelling does, or `null` when no stored record was captured.
      const encoded = host.compileWithAudit(canonical, "BUNDLER");
      const record =
        encoded === null || encoded === undefined
          ? null
          : JSON.parse(Buffer.from(encoded).toString("utf8"));
      results.cases[label] = {
        outcome: "published",
        isNull: record === null,
        canonicalId: record?.canonical_id ?? null,
      };
    } catch (error) {
      results.cases[label] = { outcome: "error", message: String(error?.message ?? error) };
    }
  }
}

process.stdout.write(JSON.stringify(results));
