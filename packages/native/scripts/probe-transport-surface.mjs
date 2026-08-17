#!/usr/bin/env node
// Executes the NAPI transport's representative cases and prints the result as
// JSON on stdout, for a Rust-side comparison against the in-process host route.
//
// It also ENUMERATES the transport's exported surface from the BUILT ARTIFACT
// itself — `Object.keys` over the loaded binding plus each exported class's own
// prototype — never by reading source. Exhaustion is a claim about the pinned
// tree, so the enumeration is what a caller can actually reach.
//
// Usage: node scripts/probe-transport-surface.mjs
// Exit codes: 0 = probed (the JSON says what happened); 2 = the binding could
// not be loaded (an execution prerequisite, reported as such, never a pass).

import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

let binding;
try {
  binding = require(path.join(here, "..", "index.js"));
} catch (error) {
  process.stdout.write(JSON.stringify({ loaded: false, error: String(error?.message ?? error) }));
  process.exit(2);
}

/** Every reachable spelling on the built artifact: module exports plus each
 *  exported class's own instance methods. */
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
/// A second supported Svelte component, distinct from SUPPORTED_SVELTE, so a
/// fanned-out batch result would be visible.
const SUPPORTED_TWO =
  '<script>\n  let total = $state(7);\n</script>\n\n<span class="total">{total}</span>\n';
/// The committed fixture whose Svelte runtime surface the client backend
/// refuses (`$props()` read from the instance script).
const ADVANCED_RUNE_REFUSAL =
  "<script>\n  let { label, disabled = false, ontoggle } = $props();\n\n  function onClick() {\n    ontoggle?.(!disabled);\n  }\n</script>\n\n<button {disabled} onclick={onClick}>{label}</button>\n";

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

const results = { loaded: true, surface: enumerateSurface(binding), cases: {} };

// ── SUCCESS: a supported Svelte client component publishes its module ───────
{
  const host = new binding.VerterHost();
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
  // OPTIONAL-PRODUCT AXIS: the same request with the map axis off.
  results.cases.svelteMainWithoutMap = virtualFile(host, "/probe/Ok.svelte", "main", {
    isProduction: true,
    sourceMap: false,
    hmrStrategy: "none",
  });
  // The CSS product travels alongside the module.
  results.cases.svelteStyleWithMap = virtualFile(
    host,
    "/probe/Ok.svelte",
    "style",
    { isProduction: true, sourceMap: true, hmrStrategy: "none" },
    0,
  );
  results.cases.svelteNodeList = host.listVirtualFiles("/probe/Ok.svelte");
  host.close();
}

// ── REFUSAL: the same component under a server profile ─────────────────────
{
  const host = new binding.VerterHost();
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
  // Artifact ABSENCE on a refusal: the CSS product must not survive it.
  results.cases.svelteServerStyle = virtualFile(
    host,
    "/probe/Server.svelte",
    "style",
    { isProduction: true, sourceMap: true, ssr: true, hmrStrategy: "none" },
    0,
  );
  host.close();
}

// ── STRUCTURAL ABSENCE: a node the carrier simply does not have ────────────
//
// The refusal case above reaches a missing node THROUGH a refused compilation.
// This one never involves a refusal at all: the carrier compiles normally and
// the requested node does not exist, because the SFC has no `<style>` block.
// The two are distinct classes of "no product" and a transport can serialize
// them differently, so both are probed.
{
  const host = new binding.VerterHost();
  host.upsert({
    canonicalId: "/probe/NoStyle.vue",
    inputId: "/probe/NoStyle.vue",
    source: VUE_SFC,
    fileKind: "vue",
  });
  results.cases.vueMissingStyle = virtualFile(
    host,
    "/probe/NoStyle.vue",
    "style",
    { isProduction: true, sourceMap: true, hmrStrategy: "none" },
    0,
  );
  // The SUCCESSFUL control on the same carrier: the node that DOES exist is
  // published, so an absent answer above cannot be a host that failed to load
  // the file at all.
  results.cases.vueMissingStyleControl = virtualFile(host, "/probe/NoStyle.vue", "main", {
    isProduction: true,
    sourceMap: true,
    hmrStrategy: "none",
  });
  host.close();
}

// ── IDE/TSX: ensure + read, on the profile the LSP uses ────────────────────
{
  const host = new binding.VerterHost();
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
  host.close();
}

// ── PUBLIC API: option conversion of the `mode` argument ────────────────────
{
  const host = new binding.VerterHost();
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
  host.close();
}

/** The batch render profile, with every REQUIRED field of the transport's own
 *  option object spelled explicitly — the lane is fail-closed by design and
 *  never substitutes defaults. */
function renderProfile({ ssr, sourceMap }) {
  return {
    isProduction: true,
    customElement: false,
    ssr,
    forceJs: false,
    forceVapor: false,
    sourceMap,
    hmrStrategy: "none",
  };
}

// ── BATCH: the same shape the in-process batch suite drives ────────────────
//
// A supported Svelte component, both refusal-shaped inputs, and a second
// distinct supported component — the refusals in the middle so a shifted or
// fanned-out result would be visible in both directions. Ordering, per-item
// attribution and per-item independence are all observable from the returned
// array.
{
  const host = new binding.VerterHost();
  const inputs = [
    { canonicalId: "/napiBatch/One.svelte", source: SUPPORTED_SVELTE },
    { canonicalId: "/napiBatch/Refused.svelte", source: ADVANCED_RUNE_REFUSAL },
    { canonicalId: "/napiBatch/Two.svelte", source: SUPPORTED_TWO },
  ];
  for (const [label, options] of [
    [
      "batchRuntimeRender",
      {
        target: "runtime-render",
        compileProfile: renderProfile({ ssr: false, sourceMap: true }),
      },
    ],
    ["batchHostBacked", { target: "host-backed" }],
    [
      "batchRuntimeRenderNoMap",
      {
        target: "runtime-render",
        compileProfile: renderProfile({ ssr: false, sourceMap: false }),
      },
    ],
    [
      "batchServerProfile",
      {
        target: "runtime-render",
        compileProfile: renderProfile({ ssr: true, sourceMap: true }),
      },
    ],
  ]) {
    try {
      const entries = host.compileMany(inputs, options);
      results.cases[label] = entries.map((entry) => ({
        canonicalId: entry.canonicalId,
        code: entry.code,
        hasMap: entry.sourceMap !== null && entry.sourceMap !== undefined,
        lang: entry.lang ?? null,
        errors: entry.errors,
      }));
    } catch (error) {
      results.cases[label] = { outcome: "error", message: String(error?.message ?? error) };
    }
  }
  host.close();
}

// ── AUDITED COMPILE: what the transport spelling actually returns ──────────
//
// The in-process entry returns the compiled product; the NAPI spelling returns
// the AUDIT RECORD only (`crates/verter_napi/src/lib.rs:2525-2540` encodes
// `.audit()` and drops the result), and `null` when audit is disabled. Both
// states are probed so the divergence is recorded rather than assumed.
{
  const withoutAudit = new binding.VerterHost();
  withoutAudit.upsert({
    canonicalId: "/probe/Audited.vue",
    inputId: "/probe/Audited.vue",
    source: VUE_SFC,
    fileKind: "vue",
  });
  results.cases.auditedCompileDisabled = {
    returned: withoutAudit.compileWithAudit("/probe/Audited.vue", "BUNDLER"),
  };
  withoutAudit.close();

  const host = new binding.VerterHost({ auditEnabled: true });
  for (const [label, canonical, source, fileKind] of [
    ["auditedCompileVue", "/probe/Audited.vue", VUE_SFC, "vue"],
    ["auditedCompileSvelte", "/probe/Audited.svelte", SUPPORTED_SVELTE, "svelte"],
  ]) {
    host.upsert({ canonicalId: canonical, inputId: canonical, source, fileKind });
    try {
      const encoded = host.compileWithAudit(canonical, "BUNDLER");
      const record = encoded === null ? null : JSON.parse(Buffer.from(encoded).toString("utf8"));
      results.cases[label] = {
        outcome: "published",
        canonicalId: record?.canonical_id ?? null,
        recordKeys: record === null ? null : Object.keys(record).sort(),
        carriesProduct:
          record !== null && ("script" in record || "code" in record || "template" in record),
      };
    } catch (error) {
      results.cases[label] = { outcome: "error", message: String(error?.message ?? error) };
    }
  }
  host.close();
}

// ── STANDALONE CSS: option conversion + artifact shape ─────────────────────
{
  const processed = binding.processStyle(".x{color:red}", {
    scopeId: "probe1234",
    scoped: true,
    sourcemap: true,
  });
  results.cases.processStyle = {
    code: processed.code,
    hasMap: processed.sourceMap !== null && processed.sourceMap !== undefined,
    moduleClasses: processed.moduleClasses,
  };
}

process.stdout.write(JSON.stringify(results));
