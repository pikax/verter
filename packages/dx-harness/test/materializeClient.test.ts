import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import {
  buildMaterializeRequest,
  parseMaterializeResult,
  runMaterialize,
} from "../src/baseline/materializeClient.js";

const HERE = dirname(fileURLToPath(import.meta.url));
const FAKE = join(HERE, "fixtures", "fakeMaterialize.mjs");

describe("buildMaterializeRequest", () => {
  it("emits the exact camelCase wire fields the bridge CLI deserializes", () => {
    const req = buildMaterializeRequest({
      workspaceRoot: "/ws",
      entries: ["/ws/A.vue", "/ws/B.vue"],
      vendorNodeModules: "/vendor/node_modules",
      expectedVueVersion: "3.5.34",
      strictVueVersion: true,
    });
    // Keys must match serde's camelCase rename exactly (snake_case would be
    // silently dropped to defaults on the Rust side).
    expect(Object.keys(req).sort()).toEqual([
      "entries",
      "expectedVueVersion",
      "strictVueVersion",
      "vendorNodeModules",
      "workspaceRoot",
    ]);
    expect(req).toMatchObject({
      workspaceRoot: "/ws",
      vendorNodeModules: "/vendor/node_modules",
      expectedVueVersion: "3.5.34",
      strictVueVersion: true,
    });
    // Negative: no snake_case leaks onto the wire.
    expect(req).not.toHaveProperty("vendor_node_modules");
    expect(req).not.toHaveProperty("strict_vue_version");
  });

  it("omits optional fields when unset and defaults strictVueVersion to true", () => {
    const req = buildMaterializeRequest({ workspaceRoot: "/ws", entries: [] });
    expect(req).not.toHaveProperty("vendorNodeModules");
    expect(req).not.toHaveProperty("expectedVueVersion");
    // The B↔C contract is strict-by-default: the harness hard-fails on vendored
    // Vue declaration drift unless the caller explicitly opts out.
    expect(req.strictVueVersion).toBe(true);
    // Negative: the default is NOT the permissive (warn-only) mode.
    expect(req.strictVueVersion).not.toBe(false);
  });

  it("honors an explicit strictVueVersion:false opt-out", () => {
    const req = buildMaterializeRequest({
      workspaceRoot: "/ws",
      entries: [],
      strictVueVersion: false,
    });
    expect(req.strictVueVersion).toBe(false);
  });
});

describe("parseMaterializeResult", () => {
  it("parses the full CLI result DTO including the authoritative source map", () => {
    const json = JSON.stringify({
      ideArtifacts: [
        {
          sourceVue: "/ws/A.vue",
          generatedPath: "/ws/A.vue.tsx",
          sourceMapPresent: true,
          sourceMap: "MAP",
        },
      ],
      publicApiTwins: [],
      verterTypesDts: "/ws/node_modules/@verter/types/index.d.ts",
      mapAbsent: ["/ws/B.vue"],
      sourceMapIdentities: { "/ws/A.vue": "id" },
      compileErrors: [{ canonical: "/ws/C.vue", message: "boom" }],
      tsconfigPath: "/ws/tsconfig.json",
      synthesizedTsconfig: true,
      supportRewrites: ["/ws/barrel.ts"],
      vueVersionWarnings: [{ package: "vue", expected: "3.5.34", found: "3.4.0" }],
    });
    const r = parseMaterializeResult(json);
    expect(r.ideArtifacts[0].sourceMap).toBe("MAP");
    expect(r.mapAbsent).toEqual(["/ws/B.vue"]);
    expect(r.compileErrors[0]).toEqual({ canonical: "/ws/C.vue", message: "boom" });
    expect(r.synthesizedTsconfig).toBe(true);
    expect(r.vueVersionWarnings[0].found).toBe("3.4.0");
  });

  it("throws on a structurally invalid result", () => {
    expect(() => parseMaterializeResult("not json")).toThrow();
    expect(() => parseMaterializeResult(JSON.stringify({ ideArtifacts: "nope" }))).toThrow();
  });
});

describe("runMaterialize (hermetic fake binary)", () => {
  it("spawns the one-shot, pipes the request over stdin, and surfaces the map verbatim", async () => {
    const result = await runMaterialize(
      process.execPath,
      buildMaterializeRequest({
        workspaceRoot: "/ws",
        entries: ["/ws/A.vue"],
        expectedVueVersion: "3.5.34",
        strictVueVersion: true,
      }),
      { extraArgs: [FAKE], cwd: HERE },
    );

    // The request body crossed stdin: the fake echoes expectedVueVersion back.
    expect(result.verterTypesDts).toBe("3.5.34");
    // The (already-shifted) source map is surfaced verbatim — B never recomputes it.
    expect(result.ideArtifacts[0].sourceMap).toBe("SHIFTED-MAP-A");
    expect(result.sourceMapIdentities["/ws/A.vue"]).toBe("identity-A");
  });

  it("rejects with the child's stderr when the one-shot exits non-zero", async () => {
    await expect(
      runMaterialize(
        process.execPath,
        buildMaterializeRequest({ workspaceRoot: "/ws", entries: [] }),
        {
          extraArgs: [FAKE],
          cwd: HERE,
          env: { ...process.env, FAKE_MAT_FAIL: "1" },
        },
      ),
    ).rejects.toThrow(/boom/);
  });
});
