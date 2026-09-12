#!/usr/bin/env node
// The validation-probe JavaScript driver.
//
// The public compile routes are addon methods bound for JavaScript, so the
// Rust runner never links the addon: it spawns this driver and speaks a
// line-delimited protocol to it.
//
// The protocol is PHASE-AUTHENTICATED. Before each step the driver announces
// the phase it is entering, so a termination with no line is attributable to
// that step and nothing else:
//
//   { probe_id, phase: "load" }       before the addon is loaded
//   { probe_id, phase: "compile" }    before the bracketed compileRequests call
//   { probe_id, phase: "reference" }  before any reference work
//
// and emits two authenticated frames per probe, in this order:
//
//   { probe_id, frame: "compile", elapsed_ns, entries }
//   { probe_id, frame: "reference", reference }
//
// The compile frame is written IMMEDIATELY after compileRequests returns and
// before any reference work, so a reference-phase death can only ever cost the
// Structural cell — never the Route/Compile terminals the runner has already
// ingested.
//
// Every JavaScript step is wrapped: a failure is reported as a LINE, never as
// an exit. A JS failure before the native call is `stage: "pre-native"`, after
// it `stage: "post-native"`; both are harness failures. What is left — an
// unacknowledged termination inside phase "compile" — can therefore only have
// happened inside the native call, which is what makes crash and timeout
// truthful classes rather than guesses.

import { createInterface } from "node:readline";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const DRIVER_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(DRIVER_DIR, "..", "..", "..");
const require = createRequire(import.meta.url);

const PROBE_KEYS = new Set(["probe_id", "entries"]);
const ENTRY_KEYS = new Set(["canonicalId", "source", "request"]);

function emit(line) {
  process.stdout.write(`${JSON.stringify(line)}\n`);
}

function describe(error) {
  if (error && typeof error === "object") {
    const name = typeof error.name === "string" ? error.name : "Error";
    const message = typeof error.message === "string" ? error.message : String(error);
    return `${name}: ${message}`;
  }
  return String(error);
}

// ---------------------------------------------------------------------------
// Addon
// ---------------------------------------------------------------------------

let host = null;

/** Load the addon and construct one host for the whole process. */
function ensureHost() {
  if (host) return host;
  const native = require(path.join(REPO_ROOT, "packages", "native", "index.js"));
  host = new native.VerterHost();
  return host;
}

// ---------------------------------------------------------------------------
// Reference producers
//
// One producer per framework, registered explicitly. A framework with no
// registered producer yields `{ inapplicable: <framework> }`; another
// framework's producer is NEVER run over it. The runner decides what that
// means from the framework's own validated manifest: `comparison = none`
// accepts it as not applicable, `comparison = structural` classifies it a
// reference failure — so deleting the Vue producer turns every Vue Structural
// cell red rather than silently inapplicable.
// ---------------------------------------------------------------------------

const referenceProducers = new Map();

/** The pinned upstream Vue reference compiler, loaded on first use. */
let vueSfc = null;

function loadVueSfc() {
  if (!vueSfc) vueSfc = require("@vue/compiler-sfc");
  return vueSfc;
}

// The reference product for one SFC, produced exactly as the direct layer of
// the repository's own per-file comparison does: parse then compileScript then
// compileTemplate, mode dev, non-SSR, module output.
referenceProducers.set("vue", (source, filename) => {
  const sfc = loadVueSfc();
  const { descriptor, errors: parseErrors } = sfc.parse(source, { filename });
  if (parseErrors?.length > 0) {
    return { error: parseErrors.map((e) => e.message ?? String(e)).join("; ") };
  }

  let scriptCode = "";
  let bindingMetadata;
  if (descriptor.script || descriptor.scriptSetup) {
    try {
      const script = sfc.compileScript(descriptor, {
        id: filename,
        inlineTemplate: false,
        isProd: false,
      });
      scriptCode = script.content;
      bindingMetadata = script.bindings;
    } catch (error) {
      return { error: `compileScript: ${describe(error)}` };
    }
  }

  let templateCode = "";
  if (descriptor.template) {
    try {
      const template = sfc.compileTemplate({
        source: descriptor.template.content,
        filename,
        id: filename,
        scoped: descriptor.styles.some((style) => style.scoped),
        isProd: false,
        ssr: false,
        compilerOptions: { mode: "module", bindingMetadata },
      });
      if (template.errors?.length > 0) {
        const messages = template.errors.map((e) => (typeof e === "string" ? e : e.message));
        return { error: `compileTemplate: ${messages.join("; ")}` };
      }
      templateCode = template.code;
    } catch (error) {
      return { error: `compileTemplate: ${describe(error)}` };
    }
  }

  return { code: [scriptCode, templateCode].filter(Boolean).join("\n\n") };
});

// ---------------------------------------------------------------------------
// Probe execution
// ---------------------------------------------------------------------------

function readProbe(line) {
  const probe = JSON.parse(line);
  if (probe === null || typeof probe !== "object" || Array.isArray(probe)) {
    throw new Error("a probe request must be an object");
  }
  for (const key of Object.keys(probe)) {
    if (!PROBE_KEYS.has(key)) throw new Error(`unknown field \`${key}\``);
  }
  if (typeof probe.probe_id !== "string" || probe.probe_id === "") {
    throw new Error("probe_id must be a non-empty string");
  }
  if (!Array.isArray(probe.entries) || probe.entries.length === 0) {
    throw new Error("entries must be a non-empty array");
  }
  for (const entry of probe.entries) {
    if (entry === null || typeof entry !== "object" || Array.isArray(entry)) {
      throw new Error("an entry must be an object");
    }
    for (const key of Object.keys(entry)) {
      if (!ENTRY_KEYS.has(key)) throw new Error(`unknown entry field \`${key}\``);
    }
    if (typeof entry.canonicalId !== "string" || entry.canonicalId === "") {
      throw new Error("entry canonicalId must be a non-empty string");
    }
    if (typeof entry.source !== "string") throw new Error("entry source must be a string");
    if (entry.request === null || typeof entry.request !== "object") {
      throw new Error("entry request must be an object");
    }
  }
  return probe;
}

function runProbe(probe) {
  const { probe_id, entries } = probe;

  emit({ probe_id, phase: "load" });
  ensureHost();

  emit({ probe_id, phase: "compile" });
  let inputs;
  try {
    inputs = entries.map((entry) => ({
      canonicalId: entry.canonicalId,
      source: Buffer.from(entry.source, "utf8"),
      request: entry.request,
    }));
  } catch (error) {
    emit({ probe_id, error: describe(error), phase: "compile", stage: "pre-native" });
    return;
  }

  const started = process.hrtime.bigint();
  const answered = host.compileRequests(inputs);
  const elapsed_ns = Number(process.hrtime.bigint() - started);

  try {
    emit({ probe_id, frame: "compile", elapsed_ns, entries: answered });
  } catch (error) {
    emit({ probe_id, error: describe(error), phase: "compile", stage: "post-native" });
    return;
  }

  emit({ probe_id, phase: "reference" });
  const reference = entries.map((entry) => {
    const framework = entry.request?.framework;
    const producer = referenceProducers.get(framework);
    if (!producer) return { inapplicable: String(framework) };
    try {
      return producer(entry.source, entry.canonicalId);
    } catch (error) {
      return { error: describe(error) };
    }
  });
  emit({ probe_id, frame: "reference", reference });
}

const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of lines) {
  if (line.trim() === "") continue;
  let probe;
  try {
    probe = readProbe(line);
  } catch (error) {
    // No probe id is trustworthy here, so the line is reported without one and
    // the runner attributes it to the probe it is waiting on.
    emit({ error: describe(error), phase: "protocol" });
    continue;
  }
  try {
    runProbe(probe);
  } catch (error) {
    emit({
      probe_id: probe.probe_id,
      error: describe(error),
      phase: "compile",
      stage: "pre-native",
    });
  }
}
