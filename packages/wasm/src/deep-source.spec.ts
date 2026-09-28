/**
 * A deeply nested script on the real WebAssembly module.
 *
 * oxc's parser and its walks recurse once per level of nesting on the
 * engine's own call stack, which nothing in the module can read or grow.
 * Past it the engine throws `RangeError: Maximum call stack size exceeded`
 * out of the module mid-call, leaving the instance unfit for another call.
 * The module parses under the V8 engine-stack profile
 * (`V8_DEFAULT_STACK_PROFILE` in `verter_parser::oxc_parse`): a script
 * whose nesting bound passes the profile's is not parsed, as typed
 * operational incompleteness: the upsert reports the typed refusal, the
 * host publishes nothing for the file, and the same instance goes on
 * serving.
 *
 * These run on the engine they load in, so an engine or oxc whose frames
 * cost more than the profile measured fails the admitted depth here: the
 * profile is then re-measured (`docs/evidence/signature-kernel/
 * oxc-deep-parse.md`, "WebAssembly").
 */

import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

import { beforeAll, describe, expect, it } from "vitest";

import { initSync, VerterHost } from "../wasm/verter_wasm.js";

const WASM_BINARY_PATH = resolve(import.meta.dirname, "../wasm/verter_wasm_bg.wasm");

/**
 * The nesting `V8_DEFAULT_STACK_PROFILE` admits: (984 KiB - 256 KiB) /
 * 2,376 bytes a level.
 */
const PROFILE_NESTING = 313;

/**
 * A `<script setup>` whose one constant is an object literal nested
 * `depth` deep (the costliest form a level): its scan bound is `depth + 1`,
 * the assignment's level included.
 */
function nestedObjects(depth: number): string {
  const value = `${"{ v: ".repeat(depth)}1${" }".repeat(depth)}`;
  return `<script setup lang="ts">\nconst n = ${value}\n</script>\n<template><div>{{ n }}</div></template>\n`;
}

const SHALLOW =
  '<script setup lang="ts">\nconst n: number = 1\n</script>\n<template><div>{{ n }}</div></template>\n';

type Analysis = { bindings: { name: string }[] };

function bindingNames(host: VerterHost, id: string): string[] {
  return (host.getAnalysis(id) as Analysis).bindings.map((binding) => binding.name);
}

describe("@verter/wasm deeply nested script", () => {
  let host: VerterHost;

  beforeAll(() => {
    if (!existsSync(WASM_BINARY_PATH)) {
      throw new Error(
        `WASM binary missing at ${WASM_BINARY_PATH}. Run \`pnpm --filter @verter/wasm build:wasm\` first.`,
      );
    }
    initSync({ module: readFileSync(WASM_BINARY_PATH) });
    // One instance for every case: a refused parse must leave it serving.
    host = new VerterHost(undefined);
  });

  it("analyzes a script at the profile's nesting on the engine it runs on", () => {
    host.upsert({ inputId: "/Admitted.vue", source: nestedObjects(PROFILE_NESTING - 1) });
    expect(bindingNames(host, "/Admitted.vue")).toEqual(["n"]);
  });

  it("refuses one level past the profile, and far past it, as typed incompleteness", () => {
    for (const depth of [PROFILE_NESTING, PROFILE_NESTING * 100]) {
      // The upsert reports the typed refusal: the source stage published
      // nothing for the file, rather than an empty file's facts.
      expect(
        () => host.upsert({ inputId: `/Refused${depth}.vue`, source: nestedObjects(depth) }),
        `depth ${depth}`,
      ).toThrow(/nests deeper than a stack this host can provide/);
      // No analysis is served for it, and no diagnostic is read off an
      // empty program: the template's `n` is not reported undefined.
      expect(host.getAnalysis(`/Refused${depth}.vue`) ?? null).toBeNull();
      const lint = host.lint(`/Refused${depth}.vue`, undefined) as { rule: string }[];
      expect(lint.filter((diagnostic) => diagnostic.rule === "no-undef-properties")).toEqual([]);
    }
  });

  it("serves a shallow script on the same instance after a refused one", () => {
    expect(() =>
      host.upsert({ inputId: "/Refused.vue", source: nestedObjects(PROFILE_NESTING * 100) }),
    ).toThrow(/nests deeper than a stack this host can provide/);
    host.upsert({ inputId: "/Shallow.vue", source: SHALLOW });
    expect(bindingNames(host, "/Shallow.vue")).toEqual(["n"]);
    // The refused file itself parses again once it is shallow.
    host.upsert({ inputId: "/Refused.vue", source: SHALLOW });
    expect(bindingNames(host, "/Refused.vue")).toEqual(["n"]);
  });
});
