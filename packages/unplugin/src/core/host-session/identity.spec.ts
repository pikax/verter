import { canonicalizeId } from "@verter/native";
import { describe, expect, it } from "vitest";
import {
  DEFAULT_COMPILE_INTENT,
  adapterKindFrom,
  encodeFramed,
  graphKind,
  graphMode,
  graphTarget,
  mintLocalProjectIdentity,
  mintLocalSourceModuleEnvironment,
  mintModuleId,
  mintSessionId,
  roleKey,
  type BundlerGraphKind,
  type BundlerHostSessionId,
  type BundlerModuleRole,
  type CarrierFramework,
  type HostCanonicalId,
  type SessionIdentityParts,
} from "./identity";

function sessionParts(overrides: Partial<SessionIdentityParts> = {}): SessionIdentityParts {
  return {
    adapter: "vite",
    adapterVersion: "6.0.0",
    root: "/proj",
    mode: "dev",
    frameworkSelection: "vue",
    optionsDigest: "o",
    compileIntent: DEFAULT_COMPILE_INTENT,
    project: mintLocalProjectIdentity("/proj"),
    sourceModuleEnvironment: mintLocalSourceModuleEnvironment({
      adapter: "vite",
      adapterVersion: "6.0.0",
      mode: "dev",
    }),
    instance: 1,
    ...overrides,
  };
}

describe("module identity minting", () => {
  it("never collides across role, graph, framework, session and carrier", () => {
    const sessions = [
      mintSessionId(sessionParts({ instance: 1 })),
      mintSessionId(sessionParts({ instance: 2 })),
    ];
    const graphs: BundlerGraphKind[] = ["client-dev", "client-prod", "ssr-dev", "ssr-prod"];
    const frameworks: CarrierFramework[] = ["vue", "sveltejs"];
    const roles: BundlerModuleRole[] = [
      { kind: "main" },
      { kind: "script" },
      { kind: "template" },
      { kind: "style", index: 0 },
      { kind: "style", index: 1 },
      { kind: "custom", index: 0, blockType: "i18n" },
    ];
    const carriers = [
      canonicalizeId("/a/App.vue"),
      canonicalizeId("/a|b:c/App.vue"),
    ] as HostCanonicalId[];
    const ids = new Set<string>();
    let count = 0;
    for (const session of sessions) {
      for (const graph of graphs) {
        for (const framework of frameworks) {
          for (const role of roles) {
            for (const carrier of carriers) {
              ids.add(mintModuleId(session, graph, framework, role, carrier));
              count++;
            }
          }
        }
      }
    }
    expect(ids.size).toBe(count);
  });

  it("gives equal ids for equal inputs", () => {
    const session = mintSessionId(sessionParts());
    const carrier = canonicalizeId("/a/App.vue") as HostCanonicalId;
    const role: BundlerModuleRole = { kind: "style", index: 2 };
    expect(mintModuleId(session, "ssr-prod", "vue", role, carrier)).toBe(
      mintModuleId(session, "ssr-prod", "vue", role, carrier),
    );
    expect(mintSessionId(sessionParts())).toBe(mintSessionId(sessionParts()));
  });

  it("mints a different session for any differing part", () => {
    const base = mintSessionId(sessionParts());
    const variants: Partial<SessionIdentityParts>[] = [
      { adapter: "rollup" },
      { adapterVersion: null },
      { adapterVersion: "" },
      { root: "/other" },
      { mode: "prod" },
      { frameworkSelection: "svelte" },
      { optionsDigest: "p" },
      { instance: 2 },
    ];
    const seen = new Set<BundlerHostSessionId>([base]);
    for (const variant of variants) seen.add(mintSessionId(sessionParts(variant)));
    expect(seen.size).toBe(variants.length + 1);
    expect(base).toMatch(/^bhs:[0-9a-f]{16}$/);
  });

  it("mints local identities with their documented prefixes", () => {
    expect(mintLocalProjectIdentity("d:\\p")).toBe(mintLocalProjectIdentity("D:/p"));
    expect(mintLocalProjectIdentity("/p")).toMatch(/^local-project:[0-9a-f]{64}$/);
    expect(
      mintLocalSourceModuleEnvironment({ adapter: "vite", adapterVersion: null, mode: "dev" }),
    ).toMatch(/^local-env:[0-9a-f]{64}$/);
  });

  it("frames fields so that distinct lists never give the same text", () => {
    expect(encodeFramed(["ab", "c"])).toBe("2:ab1:c");
    expect(encodeFramed(["a", "bc"])).not.toBe(encodeFramed(["ab", "c"]));
    expect(encodeFramed(["", "a"])).not.toBe(encodeFramed(["a", ""]));
  });

  it("names every role with stable text", () => {
    expect(roleKey({ kind: "main" })).toBe("main");
    expect(roleKey({ kind: "script" })).toBe("script");
    expect(roleKey({ kind: "template" })).toBe("template");
    expect(roleKey({ kind: "style", index: 3 })).toBe("style:3");
    expect(roleKey({ kind: "custom", index: 1, blockType: "i18n" })).toBe("custom:1:i18n");
    expect(roleKey({ kind: "generated", name: "export-helper" })).toBe("generated:export-helper");
  });

  it("keeps graph target and mode as independent axes", () => {
    expect(graphKind("ssr", "dev")).toBe("ssr-dev");
    expect(graphTarget("client-prod")).toBe("client");
    expect(graphMode("client-prod")).toBe("prod");
    expect(graphMode("ssr-dev")).toBe("dev");
  });

  it("refuses an unknown adapter name and accepts the seven known ones", () => {
    expect(() => adapterKindFrom("bun")).toThrow("[verter] unsupported bundler adapter 'bun'");
    for (const name of ["vite", "rollup", "rolldown", "webpack", "rspack", "esbuild", "farm"]) {
      expect(adapterKindFrom(name)).toBe(name);
    }
  });
});
