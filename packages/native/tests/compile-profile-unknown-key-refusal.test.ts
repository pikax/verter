/**
 * NAPI-side "unknown compileProfile key silently dropped" gap.
 *
 * `#[napi(object)]`'s derived `FromNapiValue` only reads DECLARED field
 * names off a JS object — unlike serde's `deny_unknown_fields` (already
 * used to fix the FFI/WASM half of this same class of gap on
 * `FfiCompileProfile`), it never enumerates the object's own keys. Before
 * this fix, a JS caller passing an unrecognized `compileProfile` property
 * (a typo, or a field from a different/older API shape) had that key
 * silently dropped before any Rust-side validation ever ran.
 *
 * `getIde`/`ensureIdeCompiled` receive a `compileProfile` object directly;
 * `getVirtualFile`/`applyBlockOverrides` receive it nested inside their
 * `query`/`request` argument. All four routes must now refuse an
 * unrecognized key with a clear error instead of silently ignoring it.
 *
 * Discrimination contract: on the pre-fix tree these calls succeed
 * (the extra `compatConfig` key is silently dropped); on the post-fix
 * tree they throw `InvalidArg` naming the unrecognized key.
 */

import { describe, expect, it } from "vitest";

const native = require("../index.js") as typeof import("../index");

const SFC_SOURCE =
  '<script setup lang="ts">\nconst greeting: string = "hello";\n</script>\n<template><div>{{ greeting }}</div></template>\n';

function buildHost(): InstanceType<typeof native.VerterHost> {
  const host = new native.VerterHost({});
  host.upsert({
    canonicalId: "/widget.vue",
    inputId: "/widget.vue",
    source: Buffer.from(SFC_SOURCE, "utf-8"),
  });
  return host;
}

describe("NAPI compileProfile refuses an unrecognized JS-object key", () => {
  it("getIde throws on an unrecognized compileProfile key", () => {
    const host = buildHost();
    expect(() =>
      host.getIde("/widget.vue", {
        filename: "/widget.vue",
        compatConfig: true,
      } as never),
    ).toThrow(/unrecognized compileProfile field 'compatConfig'/);
    host.close();
  });

  it("getIde succeeds with only recognized keys (positive control)", () => {
    const host = buildHost();
    expect(() => host.getIde("/widget.vue", { filename: "/widget.vue" })).not.toThrow();
    host.close();
  });

  it("ensureIdeCompiled throws on an unrecognized compileProfile key", () => {
    const host = buildHost();
    expect(() =>
      host.ensureIdeCompiled("/widget.vue", {
        target: "ide",
        compatConfig: true,
      } as never),
    ).toThrow(/unrecognized compileProfile field 'compatConfig'/);
    host.close();
  });

  it("getVirtualFile throws on an unrecognized nested compileProfile key", () => {
    const host = buildHost();
    expect(() =>
      host.getVirtualFile({
        canonicalId: "/widget.vue",
        compileProfile: {
          filename: "/widget.vue",
          compatConfig: true,
        },
      } as never),
    ).toThrow(/unrecognized compileProfile field 'compatConfig'/);
    host.close();
  });

  it("getVirtualFile succeeds with only recognized nested keys (positive control)", () => {
    const host = buildHost();
    expect(() =>
      host.getVirtualFile({
        canonicalId: "/widget.vue",
        nodeKind: { kind: "main" },
        compileProfile: { filename: "/widget.vue" },
      }),
    ).not.toThrow();
    host.close();
  });

  it("applyBlockOverrides throws on an unrecognized nested compileProfile key", () => {
    const host = buildHost();
    expect(() =>
      host.applyBlockOverrides({
        canonicalId: "/widget.vue",
        compileProfile: {
          filename: "/widget.vue",
          compatConfig: true,
        },
        overrides: [],
      } as never),
    ).toThrow(/unrecognized compileProfile field 'compatConfig'/);
    host.close();
  });
});
