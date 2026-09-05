#!/usr/bin/env node
// Drive the NAPI transport's representative cases and print JSON for
// Rust-side comparison against the in-process host.
//
// Every compiled product here is demanded through the transport's typed
// compile request: `compileRequest` for the single-carrier cases, the typed
// plural route `compileRequests` for the batch cases — one call per demand,
// stating the products it wants, with the case then selecting what it is
// about out of the response. No compile profile is built, no ensure-then-read
// ordering is relied on, and no case drives the module more than once. The
// one exception is the profile-less `host-backed` batch lane, which owns no
// profile-bearing case and stays on `compileMany`.
//
// Enumerates the exported surface from the built artifact
// (`Object.keys` + class prototypes), never from source.
//
// Exit: 0 probed with the whole record on stdout; 2 the binding could not
// load, with a `{loaded: false}` record naming why (never a pass). A third
// state exists and is deliberate: a wire break a case cannot classify
// (`soleProductRow`) throws out of the case and past the single
// `process.stdout.write` at the end, so the process exits non-zero having
// printed NOTHING — including every unrelated case. The Rust side reads
// that as "the probe emitted no JSON" and fails with the captured stderr.
// Aborting the record is the intent: a response whose product rows do not
// match the demand makes every other case's reading of that response
// unsafe.

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
// The same SFC WITH a `<style>` block. The structural-absence case below
// reads `style[0]` out of a Vue runtime product; that absence only means
// "this carrier has no such block" if a Vue runtime product WOULD carry
// the row when the carrier does have one. This carrier is the positive
// control that pins it, on the same framework arm and the same typed demand.
const VUE_SFC_WITH_STYLE = `${VUE_SFC}\n<style>\n.root { color: red; }\n</style>\n`;
/// A second supported Svelte component, distinct from SUPPORTED_SVELTE, so a
/// fanned-out batch result would be visible.
const SUPPORTED_TWO =
  '<script>\n  let total = $state(7);\n</script>\n\n<span class="total">{total}</span>\n';
/// A component whose Svelte runtime surface the client backend refuses: an
/// instance-script prop WRITE, which official lowers through the prop SETTER
/// and this backend does not emit. An instance-script prop READ is a SUPPORTED
/// surface, so a read-only component is no longer a refusal witness. These bytes
/// are mirrored verbatim by the Rust transport-equivalence suite.
const ADVANCED_RUNE_REFUSAL =
  "<script>\n  let { count = 0 } = $props();\n  function inc() { count += 1; }\n</script>\n\n<button onclick={inc}>{count}</button>\n";

// ── The typed compile request ────────────────────────────────────────────
//
// One call carries the whole demand and answers with every product it asked
// for, so a case about one node of a carrier's runtime surface selects that
// node out of the response instead of issuing a read of its own. Sibling
// cases over the SAME demand share one compile: this route consults and
// publishes no compile cache slot, so re-stating a demand would compile it
// a second time rather than hit a warm slot.
//
// The request is the generated mirror's tag-discriminated shape
// (`packages/native/host-compile-request.generated.ts`), so every object is
// closed and every required field is stated. The HMR strategy has no slot on
// this request; the route compiles with HMR off, which is exactly what the
// legacy `"none"` strategy asked for.

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
      ? { kind: "runtimeServer", runtimeSourceMap: sourceMap }
      : { kind: "runtimeClient", runtimeSourceMap: sourceMap },
  ];
  if (fileKind === "svelte") {
    return { framework: "svelte", identity, products, options: {} };
  }
  return {
    framework: "vue",
    identity,
    products,
    options: { backend: "inferred", ssr, isCustomElement: [], babelParserPlugins: [] },
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
      kind: "ideCompanion",
      wantSourceMap: sourceMap,
      embedAmbientTypes: false,
      conditionalRootNarrowing: false,
      strictSlots: false,
      ideChunkBoundaries: false,
    },
  ];
  if (fileKind === "svelte") {
    return { framework: "svelte", identity, products, options: {} };
  }
  return {
    framework: "vue",
    identity,
    products,
    options: { backend: "inferred", ssr: false, isCustomElement: [], babelParserPlugins: [] },
  };
}

/**
 * Execute ONE typed request.
 *
 * The route is complete-only: a refusal at any stage throws the refusal
 * message and publishes nothing, while a completed compile carries every
 * requested product. The two are separated here so no case downstream has
 * to infer which of them happened. The binding states the host's typed
 * diagnostic code on the error's `diagnosticCode` property rather than in
 * the message, so the code is folded into the recorded refusal — the
 * legacy profile route's message carried it, and a record without it
 * cannot be told apart from an untyped failure.
 */
function compile(host, canonicalId, request) {
  try {
    return { refusal: null, response: host.compileRequest(canonicalId, request) };
  } catch (error) {
    const message = String(error?.message ?? error);
    const code = error?.diagnosticCode;
    return { refusal: code ? `${code}: ${message}` : message, response: null };
  }
}

/**
 * The single product row of a completed compile, taken by its EXACT
 * requested tag — never by position, never by a set of admissible tags.
 *
 * A response carries one row per requested product, tagged with the
 * request's own spelling, so a one-product demand answered by anything
 * else — a different tag included — is a wire break inside this probe. It
 * fails the probe loudly rather than riding into the record as an absent
 * product: a client demand answered by a server row (or the reverse) must
 * not pass just because a main node exists.
 */
function soleProductRow(response, tag) {
  const products = Array.isArray(response?.products) ? response.products : [];
  const observed = products.map((product) => product?.kind ?? "<untagged>");
  if (products.length !== 1 || observed[0] !== tag) {
    throw new Error(`expected exactly one ${tag} product row, got: [${observed.join(", ")}]`);
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
 */
function runtimeNodeCase(compiled, productTag, kind, index) {
  if (compiled.refusal !== null) return { outcome: "error", message: compiled.refusal };
  const nodes = soleProductRow(compiled.response, productTag).nodes ?? [];
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
  // The native row nests the projection under its `ide` arm; the tag alone
  // is not the payload.
  const ide = soleProductRow(compiled.response, "ideCompanion").ide;
  return {
    outcome: "published",
    code: ide.code,
    hasMap: ide.sourceMap !== null && ide.sourceMap !== undefined,
    isJsx: ide.isJsx ?? null,
  };
}

const results = { loaded: true, surface: enumerateSurface(binding), cases: {} };

// SUCCESS + the optional-product axis
{
  const host = new binding.VerterHost();
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
  host.close();
}

// REFUSAL + artifact absence
{
  const host = new binding.VerterHost();
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
  host.close();
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
  const host = new binding.VerterHost();
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
  host.close();
}

// IDE/TSX: the IDE surface as a requested product
//
// There is no ensure-then-read pair here and no ordering for a caller to
// get right: one typed call states the IDE demand and answers with the
// projection. `ensureIdeCompiled` is that same call's producibility answer
// — the demanded IDE product came back. On this complete-only route that
// answer is the error arm: a completed `compileRequest` response cannot
// lack the `ideCompanion` row (the binding maps products 1:1 or errors), so
// no live boolean exists to report on the ok arm and none is invented.
// `outcome: "ok"` plus the published `getIdeWithMap` case below are the
// producibility proof.
//
// The map axis is a second demand, so it is a second call. It is no longer
// a never-compiled slot that answers absent: a complete compile publishes
// the projection either way, with the map only when it was asked for.
{
  const host = new binding.VerterHost();
  host.upsert({
    canonicalId: "/probe/Ide.svelte",
    inputId: "/probe/Ide.svelte",
    source: SUPPORTED_SVELTE,
    fileKind: "svelte",
  });
  const mappedIde = compile(host, "/probe/Ide.svelte", ideRequest("svelte", true));
  const unmappedIde = compile(host, "/probe/Ide.svelte", ideRequest("svelte", false));
  results.cases.ensureIdeCompiled =
    mappedIde.refusal !== null
      ? { outcome: "error", message: mappedIde.refusal }
      : { outcome: "ok" };
  results.cases.getIdeWithMap = ideCase(mappedIde);
  results.cases.getIdeWithoutMap = ideCase(unmappedIde);
  host.close();
}

// PUBLIC API: option conversion of the `mode` argument
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

// BATCH: the same shape the in-process batch suite drives
//
// A supported Svelte component, a refusal-shaped input, and a second
// distinct supported component — the refusal in the middle so a shifted or
// fanned-out result would be visible in both directions. Ordering,
// per-item attribution and per-item independence are all observable from
// the returned array.
//
// The render lanes demand their runtime surface through the typed plural
// route: one input per carrier, its source riding beside its request, one
// entry per input in input order. Each entry pins the EXACT product tag it
// demanded — a client demand answered by a server row (or the reverse) is a
// wire break, not a pass. The profile-less `host-backed` lane owns no
// profile-bearing case and stays on the legacy batch spelling.

/** The typed plural route's batch input: source beside demand, never copied
 *  into the request. The route registers each canonical's source exactly
 *  once and executes each input's own request against it. */
function typedBatchInput(canonicalId, source, request) {
  return { canonicalId, source: Buffer.from(source, "utf8"), request };
}

/** One entry of the typed plural route, normalized to the batch case's
 *  question: the demanded runtime product's main node for this input, or
 *  the typed refusal that replaced the whole entry. */
function typedBatchEntry(entry, productTag) {
  if (entry?.failure != null) {
    const message = entry.failure.message ?? null;
    const code = entry.failure.diagnosticCode;
    return {
      canonicalId: entry.canonicalId ?? null,
      outcome: "error",
      message: code ? `${code}: ${message}` : message,
      code: null,
      hasMap: false,
      lang: null,
    };
  }
  const row = soleProductRow(entry?.response, productTag);
  const node = row.nodes?.find((candidate) => candidate?.node?.kind === "main");
  if (node === undefined) {
    throw new Error(
      `batch entry ${entry?.canonicalId ?? "<unattributed>"} completed its ${productTag} demand with no main node`,
    );
  }
  return {
    canonicalId: entry?.canonicalId ?? null,
    outcome: "published",
    code: node.code,
    hasMap: node.sourceMap !== null && node.sourceMap !== undefined,
    lang: node.lang ?? null,
  };
}

{
  const inputs = [
    { canonicalId: "/napiBatch/One.svelte", source: SUPPORTED_SVELTE },
    { canonicalId: "/napiBatch/Refused.svelte", source: ADVANCED_RUNE_REFUSAL },
    { canonicalId: "/napiBatch/Two.svelte", source: SUPPORTED_TWO },
  ];
  const host = new binding.VerterHost();
  for (const [label, demand] of [
    ["batchRuntimeRender", { ssr: false, sourceMap: true }],
    ["batchRuntimeRenderNoMap", { ssr: false, sourceMap: false }],
    ["batchServerProfile", { ssr: true, sourceMap: true }],
  ]) {
    const productTag = demand.ssr ? "runtimeServer" : "runtimeClient";
    let entries;
    try {
      entries = host.compileRequests(
        inputs.map(({ canonicalId, source }) =>
          typedBatchInput(canonicalId, source, runtimeRequest("svelte", demand)),
        ),
      );
    } catch (error) {
      // The transport call itself refused: record it as the case's error.
      // Entry classification below stays OUTSIDE this catch: a wire break a
      // case cannot classify aborts the record instead of reading as one
      // bad lane.
      results.cases[label] = { outcome: "error", message: String(error?.message ?? error) };
      continue;
    }
    results.cases[label] = entries.map((entry) => typedBatchEntry(entry, productTag));
  }

  // The profile-less host-backed lane: no render profile exists to state as
  // a typed demand, so it stays on the legacy batch spelling.
  try {
    const entries = host.compileMany(inputs, { target: "host-backed" });
    results.cases.batchHostBacked = entries.map((entry) => ({
      canonicalId: entry.canonicalId,
      code: entry.code,
      hasMap: entry.sourceMap !== null && entry.sourceMap !== undefined,
      lang: entry.lang ?? null,
      errors: entry.errors,
    }));
  } catch (error) {
    results.cases.batchHostBacked = { outcome: "error", message: String(error?.message ?? error) };
  }
  host.close();
}

// AUDITED COMPILE: what the transport spelling actually returns
//
// The in-process entry returns the compiled product; the NAPI spelling returns
// the AUDIT RECORD only (`crates/verter_napi/src/lib.rs` encodes `.audit()`
// and drops the result), and `null` when audit is disabled. Both states are
// probed so the divergence is recorded rather than assumed.
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

// STANDALONE CSS: option conversion + artifact shape
{
  const transformed = binding.transformVueStyle(".x{color:red}", {
    scopeId: "probe1234",
    scoped: true,
    sourcemap: true,
  });
  results.cases.transformVueStyle = {
    code: transformed.code,
    hasMap: transformed.sourceMap !== null && transformed.sourceMap !== undefined,
    moduleClasses: transformed.moduleClasses,
    refusals: transformed.refusals,
  };
}

process.stdout.write(JSON.stringify(results));
