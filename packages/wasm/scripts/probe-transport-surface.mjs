#!/usr/bin/env node
// Drive the WASM transport's representative cases and print JSON for
// Rust-side comparison against the in-process host.
//
// Every compiled product here is demanded through the transport's typed
// compile request: one call per demand, stating the products it wants, with
// the case then selecting what it is about out of the response. No compile
// profile is built, no ensure-then-read ordering is relied on, and no case
// drives the module more than once.
//
// Enumerates the exported surface from the built artifact, never from
// source.
//
// Exit: 0 probed with the whole record on stdout; 2 the module could not
// load, with a `{loaded: false}` record naming why (never a pass). A third
// state exists and is deliberate: a wire break a case cannot classify
// (`soleProductRow`) throws out of the case and past the single
// `process.stdout.write` at the end, so the process exits non-zero having
// printed NOTHING — including every unrelated case. The Rust side reads
// that as "the probe emitted no JSON" and fails with the captured stderr.
// Aborting the record is the intent: a response whose product rows do not
// match the demand makes every other case's reading of that response
// unsafe.

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
// The same SFC WITH a `<style>` block. The structural-absence case below
// reads `style[0]` out of a Vue runtime product; that absence only means
// "this carrier has no such block" if a Vue runtime product WOULD carry the
// row when the carrier does have one. This carrier is the positive control
// that pins it, on the same framework arm and the same typed demand.
const VUE_SFC_WITH_STYLE = `${VUE_SFC}\n<style>\n.root { color: red; }\n</style>\n`;

// ── The typed compile request ────────────────────────────────────────────
//
// One call carries the whole demand and answers with every product it asked
// for, so a case about one node of a carrier's runtime surface selects that
// node out of the response instead of issuing a read of its own. Sibling
// cases over the SAME demand share one compile: this route consults and
// publishes no compile cache slot, so re-stating a demand would compile it
// a second time rather than hit a warm slot.
//
// The HMR strategy has no slot on this request. The route compiles with HMR
// off, which is exactly what a `"none"` strategy asked for.

/**
 * A carrier's runtime surface, as one typed request.
 *
 * The requested product kind IS the client/server demand: on the Svelte arm
 * there is no second bit that could disagree with it. The Vue arm keeps its
 * own `options.ssr` row, which the request schema carries separately and
 * which therefore CAN contradict the product tag; both are derived from the
 * one `ssr` argument here so this probe never states that contradiction.
 * The source-map axis is stated per product rather than once for the whole
 * compile.
 */
function runtimeRequest(fileKind, { sourceMap, ssr }) {
  const identity = { isProduction: true, forceJs: false };
  const products = [
    ssr
      ? { runtimeServer: { runtimeSourceMap: sourceMap } }
      : { runtimeClient: { runtimeSourceMap: sourceMap } },
  ];
  if (fileKind === "svelte") return { svelte: { identity, products, options: {} } };
  return {
    vue: {
      identity,
      products,
      options: { backend: "inferred", ssr, isCustomElement: [], babelParserPlugins: [] },
    },
  };
}

/**
 * A carrier's IDE surface, as one typed request.
 *
 * `ideChunkBoundaries` is REQUIRED to be false: the carrier bridge
 * substitutes its own value derived from the selected template block, so
 * the host route refuses the request outright when a caller asks for `true`
 * and `false` is the only admitted value. The remaining axes are stated at
 * the same values a bare IDE demand leaves them at.
 */
function ideRequest(fileKind, sourceMap) {
  const identity = { isProduction: false, forceJs: false };
  const products = [
    {
      ideCompanion: {
        wantSourceMap: sourceMap,
        embedAmbientTypes: false,
        conditionalRootNarrowing: false,
        strictSlots: false,
        ideChunkBoundaries: false,
      },
    },
  ];
  if (fileKind === "svelte") return { svelte: { identity, products, options: {} } };
  return {
    vue: {
      identity,
      products,
      options: { backend: "inferred", ssr: false, isCustomElement: [], babelParserPlugins: [] },
    },
  };
}

/**
 * Execute ONE typed request.
 *
 * The route is complete-only: a refusal at any stage throws the refusal
 * message as a string and publishes nothing, while a completed compile
 * carries every requested product. The two are separated here so no case
 * downstream has to infer which of them happened.
 */
function compile(host, canonicalId, request) {
  try {
    return { refusal: null, response: host.compileRequest(canonicalId, request) };
  } catch (error) {
    return { refusal: String(error?.message ?? error), response: null };
  }
}

/**
 * Whether a completed compile's response carries a row for the named
 * product — the producibility question, read straight off the response.
 *
 * Deliberately NOT routed through `soleProductRow`: this answers `false`
 * for a response that carried no such row instead of aborting the record,
 * so it is a question the route can fail. `soleProductRow` stays the strict
 * reader for cases that go on to take BYTES out of a row.
 */
function carriesProductRow(response, tag) {
  const products = Array.isArray(response?.products) ? response.products : [];
  return products.some((product) => product?.kind === tag);
}

/**
 * The single product row of a completed compile, taken by tag — never by
 * position.
 *
 * A response carries one row per requested product, tagged with the
 * request's own spelling, so a one-product demand answered by anything else
 * is a wire break inside this probe. It fails the probe loudly rather than
 * riding into the record as an absent product.
 */
function soleProductRow(response, tags) {
  const products = Array.isArray(response?.products) ? response.products : [];
  const observed = products.map((product) => product?.kind ?? "<untagged>");
  if (products.length !== 1 || !tags.includes(observed[0])) {
    throw new Error(
      `expected exactly one [${tags.join("|")}] product row, got: [${observed.join(", ")}]`,
    );
  }
  return products[0];
}

/**
 * One node of a carrier's compiled runtime surface.
 *
 * Three classes, kept apart. A REFUSED compile is an `error` carrying the
 * host's typed diagnostic code: the transaction produced nothing, so no
 * node of it exists. A node the carrier simply does not have is `missing`:
 * the compile completed and its product carries no such row. Everything
 * else is the published node. Collapsing any two of these would hide which
 * one the transport produced.
 *
 * `productTag` is the exact requested product (`runtimeClient` or
 * `runtimeServer`). A completed compile whose sole row is the other tag
 * is a wire break, not a pass.
 */
function runtimeNodeCase(compiled, productTag, kind, index) {
  if (compiled.refusal !== null) return { outcome: "error", message: compiled.refusal };
  const nodes = soleProductRow(compiled.response, [productTag]).nodes ?? [];
  const node = nodes.find(
    (row) => row?.node?.kind === kind && (row?.node?.index ?? null) === (index ?? null),
  );
  if (node === undefined) return { outcome: "missing" };
  return {
    outcome: "published",
    code: node.code,
    hasMap: node.sourceMap !== null && node.sourceMap !== undefined,
    lang: node.lang ?? null,
  };
}

/** The IDE projection of a completed compile, or the refusal that replaced it. */
function ideCase(compiled) {
  if (compiled.refusal !== null) return { outcome: "error", message: compiled.refusal };
  const row = soleProductRow(compiled.response, ["ideCompanion"]);
  return {
    outcome: "published",
    code: row.code,
    hasMap: row.sourceMap !== null && row.sourceMap !== undefined,
    isJsx: row.isJsx ?? null,
  };
}

const results = { loaded: true, surface: enumerateSurface(module_), cases: {} };

// SUCCESS + the optional-product axis
{
  const host = new module_.VerterHost({});
  host.upsert({
    canonicalId: "/probe/Ok.svelte",
    inputId: "/probe/Ok.svelte",
    source: SUPPORTED_SVELTE,
    fileKind: "svelte",
  });
  // One compile per DEMAND, not per case: the main and style cases below are
  // two nodes of the one mapped runtime surface, so they read the same
  // response rather than compiling that surface twice.
  const mapped = compile(
    host,
    "/probe/Ok.svelte",
    runtimeRequest("svelte", { sourceMap: true, ssr: false }),
  );
  // The map axis is its own demand, so it is its own compile.
  const unmapped = compile(
    host,
    "/probe/Ok.svelte",
    runtimeRequest("svelte", { sourceMap: false, ssr: false }),
  );
  results.cases.svelteMainWithMap = runtimeNodeCase(mapped, "runtimeClient", "main");
  results.cases.svelteMainWithoutMap = runtimeNodeCase(unmapped, "runtimeClient", "main");
  results.cases.svelteStyleWithMap = runtimeNodeCase(mapped, "runtimeClient", "style", 0);
  results.cases.svelteNodeList = host.listVirtualFiles("/probe/Ok.svelte");
}

// REFUSAL + artifact absence
{
  const host = new module_.VerterHost({});
  host.upsert({
    canonicalId: "/probe/Server.svelte",
    inputId: "/probe/Server.svelte",
    source: SUPPORTED_SVELTE,
    fileKind: "svelte",
  });
  // The server surface is ONE demand, and the route refuses it as a whole:
  // no product is assembled, so neither the main node nor the CSS node of
  // that surface exists. Both cases therefore observe the same typed
  // refusal, carrying the host's diagnostic code, and neither carries bytes.
  const server = compile(
    host,
    "/probe/Server.svelte",
    runtimeRequest("svelte", { sourceMap: true, ssr: true }),
  );
  results.cases.svelteServerRefusal = runtimeNodeCase(server, "runtimeServer", "main");
  results.cases.svelteServerStyle = runtimeNodeCase(server, "runtimeServer", "style", 0);
}

// STRUCTURAL ABSENCE: a node the carrier simply does not have
//
// The case above never produces a node because the whole compile is refused.
// This one involves no refusal at all: the carrier compiles normally, its
// runtime product is complete, and the requested node is still not in it —
// the SFC has no `<style>` block. The two are distinct classes of "no
// product" and the transport serializes them differently, so both are
// probed and the successful control below proves the file loaded.
{
  const host = new module_.VerterHost({});
  host.upsert({
    canonicalId: "/probe/NoStyle.vue",
    inputId: "/probe/NoStyle.vue",
    source: VUE_SFC,
    fileKind: "vue",
  });
  const styleless = compile(
    host,
    "/probe/NoStyle.vue",
    runtimeRequest("vue", { sourceMap: true, ssr: false }),
  );
  results.cases.vueMissingStyle = runtimeNodeCase(styleless, "runtimeClient", "style", 0);
  // The SUCCESSFUL control, out of the SAME completed compile: the node that
  // DOES exist is published, so the absent answer above cannot be a host that
  // failed to load the file at all.
  results.cases.vueMissingStyleControl = runtimeNodeCase(styleless, "runtimeClient", "main");

  // Internal positive control for the absent node itself. A Vue carrier that
  // HAS a `<style>` block must publish `style[0]` on this same demand, or the
  // absence above is vacuous. This is NOT a probe output key: adding one
  // would change the case set. A miss aborts the record instead.
  host.upsert({
    canonicalId: "/probe/WithStyle.vue",
    inputId: "/probe/WithStyle.vue",
    source: VUE_SFC_WITH_STYLE,
    fileKind: "vue",
  });
  const styled = compile(
    host,
    "/probe/WithStyle.vue",
    runtimeRequest("vue", { sourceMap: true, ssr: false }),
  );
  const styleControl = runtimeNodeCase(styled, "runtimeClient", "style", 0);
  if (styleControl.outcome !== "published") {
    throw new Error(
      `Vue carrier with a style block produced ${styleControl.outcome}, so vueMissingStyle cannot discriminate a missing style node`,
    );
  }
}

// IDE/TSX: the IDE surface as a requested product
//
// There is no ensure-then-read pair here and no ordering for a caller to
// get right: one typed call states the IDE demand and answers with the
// projection. `ensureIdeCompiled` is that same call's producibility answer
// — the demanded IDE product came back — read off the one response rather
// than compiled a second time.
//
// The map axis is a second demand, so it is a second call. It is no longer
// a never-compiled slot that answers absent: a complete compile publishes
// the projection either way, with the map only when it was asked for.
{
  const host = new module_.VerterHost({});
  host.upsert({
    canonicalId: "/probe/Ide.svelte",
    inputId: "/probe/Ide.svelte",
    source: SUPPORTED_SVELTE,
    fileKind: "svelte",
  });
  const mappedIde = compile(host, "/probe/Ide.svelte", ideRequest("svelte", true));
  const unmappedIde = compile(host, "/probe/Ide.svelte", ideRequest("svelte", false));
  // Producibility on this complete-only route is the error arm: if the IDE
  // product is admitted and publishes nothing, `compileRequest` throws and
  // this case is `{outcome: "error"}`. A completed response cannot lack the
  // row (`compile_request_response_to_wasm` maps 1:1 or errors), so `value`
  // on the ok arm is not independently falsifiable here. The host comparison
  // still pins `true` against `ensure_ide_compiled`.
  results.cases.ensureIdeCompiled =
    mappedIde.refusal !== null
      ? { outcome: "error", message: mappedIde.refusal }
      : { outcome: "ok", value: carriesProductRow(mappedIde.response, "ideCompanion") };
  results.cases.getIdeWithMap = ideCase(mappedIde);
  results.cases.getIdeWithoutMap = ideCase(unmappedIde);
}

// PUBLIC API: option conversion of the `mode` argument
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

// AUDITED COMPILE: the same spelling the NAPI probe drives
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
