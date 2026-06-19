#!/usr/bin/env node
/*
  Generates
  crates/verter_compiler/src/svelte/ide/bind_contract_data.rs
  from the CLOSED Svelte-5-documented binding-name vocabulary authored below.

  This registry IS the source of truth for the wide `bind:` family (F4): for
  every documented element binding name it pins the bound value's TS TYPE and
  its DIRECTION (read / write / read-write), plus the element/tag constraint
  that selects it. The Svelte IDE projector consults the generated table to
  emit a type-checked assignment-compatibility check in the projected `.svelte.tsx`
  (the one generic checker in the prelude is an implementation HELPER — the
  authority is this table).

  Directions (from the bound LOCAL's perspective):
    - "rw"  read-write: Svelte both reads the local to set the DOM and writes
            DOM changes back into the local → the local is INVARIANT with `V`.
    - "r"   read-direction (readonly DOM property, DOM → local only): the local
            RECEIVES `V` from the DOM and can never write back → `V` must be
            assignable to the local; a userland write to the binding is rejected.

  `bind:this` and `bind:group` are SPECIAL (host-instance assignment-compat /
  checkbox-vs-radio array shape) and carry a `special` marker so the projector
  routes them to their dedicated checkers rather than the generic value-type
  check. Their `value_type` column is documentary.

  The generated file is byte-pinned by
  crates/verter_compiler/tests/cases/svelte_bind_contract_freshness.rs — a registry
  edit without a regen (or a hand-edit of the generated file) fails that gate.
  Regenerate with `node scripts/generate-svelte-bind-contract.mjs`.
*/

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// The CLOSED Svelte 5 element-binding vocabulary. Each row:
//   name        the binding local (`value` in `bind:value`)
//   direction   "rw" | "r" | "w"
//   value_type  the TS type of the bound value (a `svelte`/DOM type expression).
//               For `host` value types, `{HOST}` is substituted by the projector
//               with the element's host-instance type (`__VerterHostEl<"tag">`
//               for an intrinsic, `InstanceType<typeof C>` for a component).
//   tags        the applicable lowercase tag set, or "*" for any element, or a
//               "contenteditable" pseudo-constraint (any element with the
//               attribute — the projector does not enforce it, documentary).
//   special     "this" | "group" | null — routes to a dedicated checker.
const REGISTRY = [
  // bind:this — host-instance assignment-compat (read-direction). value_type is
  // documentary; the projector substitutes the host-instance type and routes to
  // the `this` checker.
  { name: "this", direction: "r", value_type: "{HOST}", tags: "*", special: "this" },

  // bind:group — checkbox-vs-radio shared selection. Routed to the `group`
  // checker which inspects the sibling `type` attribute; value_type documentary.
  { name: "group", direction: "rw", value_type: "unknown", tags: "input", special: "group" },

  // bind:files — the FileList written to/read from a file input.
  { name: "files", direction: "rw", value_type: "FileList | null", tags: "input", special: null },

  // bind:indeterminate — the checkbox indeterminate boolean (read-write).
  { name: "indeterminate", direction: "rw", value_type: "boolean", tags: "input", special: null },

  // <details bind:open> — the open boolean.
  { name: "open", direction: "rw", value_type: "boolean", tags: "details", special: null },

  // contenteditable bindings — the element text content as a string.
  {
    name: "innerHTML",
    direction: "rw",
    value_type: "string",
    tags: "contenteditable",
    special: null,
  },
  {
    name: "innerText",
    direction: "rw",
    value_type: "string",
    tags: "contenteditable",
    special: null,
  },
  {
    name: "textContent",
    direction: "rw",
    value_type: "string",
    tags: "contenteditable",
    special: null,
  },

  // Writable media bindings (HTMLMediaElement; read-write).
  {
    name: "currentTime",
    direction: "rw",
    value_type: 'HTMLMediaElement["currentTime"]',
    tags: "audio,video",
    special: null,
  },
  {
    name: "playbackRate",
    direction: "rw",
    value_type: 'HTMLMediaElement["playbackRate"]',
    tags: "audio,video",
    special: null,
  },
  {
    name: "volume",
    direction: "rw",
    value_type: 'HTMLMediaElement["volume"]',
    tags: "audio,video",
    special: null,
  },
  {
    name: "muted",
    direction: "rw",
    value_type: 'HTMLMediaElement["muted"]',
    tags: "audio,video",
    special: null,
  },
  {
    name: "paused",
    direction: "rw",
    value_type: 'HTMLMediaElement["paused"]',
    tags: "audio,video",
    special: null,
  },

  // Readonly media bindings (DOM → local only; a userland write is rejected).
  {
    name: "duration",
    direction: "r",
    value_type: 'HTMLMediaElement["duration"]',
    tags: "audio,video",
    special: null,
  },
  {
    name: "buffered",
    direction: "r",
    value_type: 'HTMLMediaElement["buffered"]',
    tags: "audio,video",
    special: null,
  },
  {
    name: "seekable",
    direction: "r",
    value_type: 'HTMLMediaElement["seekable"]',
    tags: "audio,video",
    special: null,
  },
  {
    name: "played",
    direction: "r",
    value_type: 'HTMLMediaElement["played"]',
    tags: "audio,video",
    special: null,
  },
  {
    name: "seeking",
    direction: "r",
    value_type: 'HTMLMediaElement["seeking"]',
    tags: "audio,video",
    special: null,
  },
  {
    name: "ended",
    direction: "r",
    value_type: 'HTMLMediaElement["ended"]',
    tags: "audio,video",
    special: null,
  },
  {
    name: "readyState",
    direction: "r",
    value_type: 'HTMLMediaElement["readyState"]',
    tags: "audio,video",
    special: null,
  },

  // Readonly dimension bindings (DOM → local only; number).
  { name: "clientWidth", direction: "r", value_type: "number", tags: "*", special: null },
  { name: "clientHeight", direction: "r", value_type: "number", tags: "*", special: null },
  { name: "offsetWidth", direction: "r", value_type: "number", tags: "*", special: null },
  { name: "offsetHeight", direction: "r", value_type: "number", tags: "*", special: null },

  // Readonly media-dimension bindings on <img>/<video>.
  { name: "naturalWidth", direction: "r", value_type: "number", tags: "img", special: null },
  { name: "naturalHeight", direction: "r", value_type: "number", tags: "img", special: null },
  { name: "videoWidth", direction: "r", value_type: "number", tags: "video", special: null },
  { name: "videoHeight", direction: "r", value_type: "number", tags: "video", special: null },

  // Readonly resize-observer bindings (DOM → local only; any element).
  { name: "contentRect", direction: "r", value_type: "DOMRectReadOnly", tags: "*", special: null },
  {
    name: "contentBoxSize",
    direction: "r",
    value_type: "readonly ResizeObserverSize[]",
    tags: "*",
    special: null,
  },
  {
    name: "borderBoxSize",
    direction: "r",
    value_type: "readonly ResizeObserverSize[]",
    tags: "*",
    special: null,
  },
  {
    name: "devicePixelContentBoxSize",
    direction: "r",
    value_type: "readonly ResizeObserverSize[]",
    tags: "*",
    special: null,
  },
];

function rsStr(s) {
  return JSON.stringify(s);
}

function directionVariant(dir) {
  switch (dir) {
    case "rw":
      return "BindDirection::ReadWrite";
    case "r":
      return "BindDirection::Read";
    default:
      throw new Error(`unknown direction: ${dir}`);
  }
}

function specialVariant(special) {
  switch (special) {
    case "this":
      return "BindSpecial::This";
    case "group":
      return "BindSpecial::Group";
    case null:
      return "BindSpecial::None";
    default:
      throw new Error(`unknown special: ${special}`);
  }
}

function generate(root) {
  const lines = [];
  lines.push("// This file is auto-generated by scripts/generate-svelte-bind-contract.mjs");
  lines.push("// The CLOSED Svelte-5 element-binding vocabulary (F4). Do NOT hand-edit:");
  lines.push("// edit the registry in the generator and regenerate, or the freshness");
  lines.push("// gate (svelte_bind_contract_freshness.rs) fails.");
  lines.push("");
  lines.push("use super::bind_contract::{BindContract, BindDirection, BindSpecial};");
  lines.push("");
  lines.push("/// The complete CLOSED bind-contract table — the SOURCE OF TRUTH for the");
  lines.push("/// wide `bind:` family. Ordered as authored in the generator registry.");
  lines.push("pub(crate) const SVELTE_BIND_CONTRACTS: &[BindContract] = &[");
  for (const row of REGISTRY) {
    lines.push("    BindContract {");
    lines.push(`        name: ${rsStr(row.name)},`);
    lines.push(`        direction: ${directionVariant(row.direction)},`);
    lines.push(`        value_type: ${rsStr(row.value_type)},`);
    lines.push(`        tags: ${rsStr(row.tags)},`);
    lines.push(`        special: ${specialVariant(row.special)},`);
    lines.push("    },");
  }
  lines.push("];");
  lines.push("");

  // The freshness gate redirects output to a temp file via this override so it
  // can byte-compare a regen without mutating the committed tree.
  const outPath =
    process.env.VERTER_BIND_CONTRACT_OUT ||
    path.join(root, "crates", "verter_compiler", "src", "svelte", "ide", "bind_contract_data.rs");
  fs.writeFileSync(outPath, lines.join("\n"));
  return outPath;
}

const root = path.resolve(__dirname, "..");
const out = generate(root);
console.log(`Generated ${path.relative(root, out)}`);
