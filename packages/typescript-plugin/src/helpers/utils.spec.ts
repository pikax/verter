import { describe, it, expect } from "vitest";
import {
  cleanupCarrierVirtualImportPath,
  containingFileAwareExists,
  getVueVirtualFileInfo,
  isLikelyTestFileName,
  isRelativeVue,
  isRelativeVueTs,
  isVue,
  isVueTs,
  isVueTestingTs,
  resolveVuePublicApiMode,
  stripVueVirtualSuffixBackingAware,
} from "./utils";

describe("isVue", () => {
  it("matches .vue suffix", () => {
    expect(isVue("./Foo.vue")).toBe(true);
    expect(isVue("../components/Bar.vue")).toBe(true);
    expect(isVue("Comp.vue")).toBe(true);
  });
  it("does not match .vue.ts", () => {
    expect(isVue("./Foo.vue.ts")).toBe(false);
  });
  it("does not match .vue.d.ts", () => {
    expect(isVue("./Foo.vue.d.ts")).toBe(false);
  });
  it("does not match .ts", () => {
    expect(isVue("./Foo.ts")).toBe(false);
  });
});

describe("isRelativeVue", () => {
  it("matches relative .vue", () => {
    expect(isRelativeVue("./Foo.vue")).toBe(true);
    expect(isRelativeVue("../Foo.vue")).toBe(true);
  });
  it("does not match non-relative .vue", () => {
    expect(isRelativeVue("@/Foo.vue")).toBe(false);
    expect(isRelativeVue("vue")).toBe(false);
  });
});

describe("isVueTs", () => {
  it("matches .vue.ts suffix", () => {
    expect(isVueTs("./Foo.vue.ts")).toBe(true);
    expect(isVueTs("../components/Bar.vue.ts")).toBe(true);
    expect(isVueTs("Comp.vue.ts")).toBe(true);
  });
  it("does not match plain .vue", () => {
    expect(isVueTs("./Foo.vue")).toBe(false);
  });
  it("does not match plain .ts", () => {
    expect(isVueTs("./Foo.ts")).toBe(false);
  });
  it("does not match .vue.d.ts", () => {
    expect(isVueTs("./Foo.vue.d.ts")).toBe(false);
  });
});

describe("isRelativeVueTs", () => {
  it("matches relative .vue.ts", () => {
    expect(isRelativeVueTs("./Foo.vue.ts")).toBe(true);
    expect(isRelativeVueTs("../Foo.vue.ts")).toBe(true);
  });
  it("does not match non-relative .vue.ts", () => {
    expect(isRelativeVueTs("@/Foo.vue.ts")).toBe(false);
    expect(isRelativeVueTs("vue.vue.ts")).toBe(false);
  });
});

describe("getVueVirtualFileInfo", () => {
  it("parses the public virtual suffixes", () => {
    expect(getVueVirtualFileInfo("/src/Foo.vue.ts")).toEqual({
      sourceFileName: "/src/Foo.vue",
      mode: "public",
    });
    expect(getVueVirtualFileInfo("/src/Foo.vue.d.ts")).toEqual({
      sourceFileName: "/src/Foo.vue",
      mode: "public",
    });
  });

  it("parses the testing virtual suffix", () => {
    expect(getVueVirtualFileInfo("/src/Foo.vue.__verter_test.ts")).toEqual({
      sourceFileName: "/src/Foo.vue",
      mode: "testing",
    });
  });

  it("returns null for non-virtual paths", () => {
    expect(getVueVirtualFileInfo("/src/Foo.vue")).toBeNull();
    expect(getVueVirtualFileInfo("/src/Foo.ts")).toBeNull();
  });
});

describe("isLikelyTestFileName", () => {
  it("matches common spec and test file names", () => {
    expect(isLikelyTestFileName("/src/App.spec.ts")).toBe(true);
    expect(isLikelyTestFileName("/src/App.test.tsx")).toBe(true);
    expect(isLikelyTestFileName("/src/__tests__/App.ts")).toBe(true);
  });

  it("does not flag normal source files", () => {
    expect(isLikelyTestFileName("/src/App.ts")).toBe(false);
    expect(isLikelyTestFileName("/src/components/Foo.vue")).toBe(false);
  });
});

describe("carrier generalization (Svelte)", () => {
  it("recognizes .svelte as a carrier (generalized from the registry column)", () => {
    expect(isVue("./Comp.svelte")).toBe(true);
    expect(isRelativeVue("./Comp.svelte")).toBe(true);
    expect(isRelativeVue("svelte")).toBe(false);
  });

  it("recognizes the .svelte.ts api virtual-file SHAPE (suffix-only)", () => {
    // These are pure PATH functions: `Comp.svelte.ts` matches the api
    // virtual-file SHAPE. The `.svelte.ts` ambiguity against a real rune module
    // (`store.svelte.ts`) is disambiguated by the CONSUMER's file-exists
    // check (the `readFile`/`fileExists` overrides fall through to the real
    // file when the backing `.svelte` source is absent) — not in this function.
    expect(isVueTs("./Comp.svelte.ts")).toBe(true);
    expect(isVueTs("./Comp.svelte")).toBe(false);
  });

  it("parses .svelte virtual-file SHAPES (public only — Svelte has no testing surface)", () => {
    expect(getVueVirtualFileInfo("/src/Comp.svelte.ts")).toEqual({
      sourceFileName: "/src/Comp.svelte",
      mode: "public",
    });
    expect(getVueVirtualFileInfo("/src/Comp.svelte.d.ts")).toEqual({
      sourceFileName: "/src/Comp.svelte",
      mode: "public",
    });
    // Svelte ships NO testing-API surface — `.svelte.__verter_test.ts` is not
    // a recognized virtual file.
    expect(getVueVirtualFileInfo("/src/Comp.svelte.__verter_test.ts")).toBeNull();
    expect(isVueTestingTs("/src/Comp.svelte.__verter_test.ts")).toBe(false);
  });

  it("preserves Vue behavior exactly (negative — generalization did not regress Vue)", () => {
    expect(isVue("./Foo.vue")).toBe(true);
    expect(isVue("./Foo.vue.ts")).toBe(false);
    expect(isVueTs("./Foo.vue.ts")).toBe(true);
    expect(isVueTestingTs("./Foo.vue.__verter_test.ts")).toBe(true);
    expect(getVueVirtualFileInfo("/src/Foo.vue.__verter_test.ts")).toEqual({
      sourceFileName: "/src/Foo.vue",
      mode: "testing",
    });
  });
});

describe("resolveVuePublicApiMode", () => {
  it("stays public when testing bindings are disabled", () => {
    expect(resolveVuePublicApiMode(false, "/src/App.spec.ts", () => true)).toBe("public");
  });

  it("stays public for non-test importers when testing bindings are enabled", () => {
    expect(resolveVuePublicApiMode(true, "/src/components/App.vue", () => false)).toBe("public");
  });

  it("switches to testing for test importers when enabled", () => {
    expect(
      resolveVuePublicApiMode(
        true,
        "/src/__tests__/App.spec.ts",
        (fileName) => fileName === "/src/__tests__/App.spec.ts",
      ),
    ).toBe("testing");
  });
});

describe("cleanupCarrierVirtualImportPath", () => {
  it("strips the UNAMBIGUOUS Vue carrier virtual suffixes by shape (no host needed)", () => {
    // `.vue.ts` / `.vue.d.ts` / `.vue.__verter_test.ts` never collide with a
    // real source extension, so they strip by shape regardless of `fileExists`.
    expect(cleanupCarrierVirtualImportPath("./Foo.vue.ts")).toBe("./Foo.vue");
    expect(cleanupCarrierVirtualImportPath("./Foo.vue.d.ts")).toBe("./Foo.vue");
    expect(cleanupCarrierVirtualImportPath("./Foo.vue.__verter_test.ts")).toBe("./Foo.vue");
  });

  it("strips Vue suffixes embedded in free-form text (quick-fix descriptions / edits)", () => {
    expect(cleanupCarrierVirtualImportPath('import Foo from "./Foo.vue.ts"')).toBe(
      'import Foo from "./Foo.vue"',
    );
  });

  it("does NOT strip an AMBIGUOUS .svelte.ts without a backing-file check (no host)", () => {
    // `.svelte.ts` collides with a real standalone rune-module extension, so
    // without a `fileExists` predicate it must be left intact — a real
    // `./store.svelte.ts` rune import in display text is never mangled to
    // `./store.svelte`. This is the F2 regression: the prior implementation
    // stripped it unconditionally.
    expect(cleanupCarrierVirtualImportPath("./store.svelte.ts")).toBe("./store.svelte.ts");
    expect(cleanupCarrierVirtualImportPath("./store.svelte.d.ts")).toBe("./store.svelte.d.ts");
    expect(cleanupCarrierVirtualImportPath('import { count } from "./store.svelte.ts"')).toBe(
      'import { count } from "./store.svelte.ts"',
    );
  });

  it("strips an AMBIGUOUS .svelte.ts ONLY when the backing .svelte carrier exists", () => {
    // Backing carrier present ⇒ proven virtual ⇒ strip.
    const backingExists = (p: string) => p === "./Comp.svelte";
    expect(cleanupCarrierVirtualImportPath("./Comp.svelte.ts", backingExists)).toBe(
      "./Comp.svelte",
    );
    expect(
      cleanupCarrierVirtualImportPath('import Comp from "./Comp.svelte.ts"', backingExists),
    ).toBe('import Comp from "./Comp.svelte"');

    // No backing carrier ⇒ real rune module ⇒ left intact even WITH a predicate.
    const noBacking = () => false;
    expect(cleanupCarrierVirtualImportPath("./store.svelte.ts", noBacking)).toBe(
      "./store.svelte.ts",
    );
  });

  it("leaves a bare carrier path and a plain .ts module untouched", () => {
    expect(cleanupCarrierVirtualImportPath("./Foo.vue")).toBe("./Foo.vue");
    expect(cleanupCarrierVirtualImportPath("./Bar.svelte")).toBe("./Bar.svelte");
    expect(cleanupCarrierVirtualImportPath("./util.ts")).toBe("./util.ts");
    expect(cleanupCarrierVirtualImportPath("nothing to strip here")).toBe("nothing to strip here");
  });
});

describe("stripVueVirtualSuffixBackingAware (F2 rune-module disambiguation)", () => {
  it("strips a virtual X.svelte.ts to X.svelte when the backing carrier exists", () => {
    const backingExists = (p: string) => p === "/src/Comp.svelte";
    expect(stripVueVirtualSuffixBackingAware("/src/Comp.svelte.ts", backingExists)).toBe(
      "/src/Comp.svelte",
    );
    expect(stripVueVirtualSuffixBackingAware("/src/Comp.svelte.d.ts", backingExists)).toBe(
      "/src/Comp.svelte",
    );
  });

  it("leaves a real X.svelte.ts rune module unchanged when no backing carrier exists", () => {
    const noBacking = () => false;
    expect(stripVueVirtualSuffixBackingAware("/src/store.svelte.ts", noBacking)).toBe(
      "/src/store.svelte.ts",
    );
  });

  it("preserves Vue behavior — Foo.vue.ts always strips because Foo.vue exists", () => {
    const vueBackingExists = (p: string) => p === "/src/Foo.vue";
    expect(stripVueVirtualSuffixBackingAware("/src/Foo.vue.ts", vueBackingExists)).toBe(
      "/src/Foo.vue",
    );
    // A plain module with no virtual shape passes through normalised.
    expect(stripVueVirtualSuffixBackingAware("/src/util.ts", () => true)).toBe("/src/util.ts");
  });
});

describe("containingFileAwareExists (relative-backing resolution at the cleanup call sites)", () => {
  // The underlying TS host `fileExists` only recognises host-rooted / absolute
  // paths — it answers `false` to a relative `./Comp.svelte`. A completion
  // edit / display token is FREQUENTLY relative (`./Comp.svelte.ts` from a
  // containing file `/app/Parent.ts`). The wrapper resolves a non-absolute
  // candidate against `path.dirname(containingFile)` before delegating, so the
  // backing proof succeeds and the AMBIGUOUS Svelte virtual suffix strips —
  // closing the Svelte-vs-Vue parity gap — while the cleanup still returns the
  // ORIGINAL (relative) text minus the suffix, never an absolutized path.
  //
  // The host predicate below is FAITHFUL to the real `_fileExists`: it only
  // knows the ABSOLUTE backing `/app/Comp.svelte`, never the relative spelling.
  const ONLY_ABS_COMP_SVELTE = (p: string) => p === "/app/Comp.svelte";

  it("strips a RELATIVE virtual ./Comp.svelte.ts when the backing /app/Comp.svelte exists", () => {
    // RED pre-fix: the raw host predicate sees `fileExists("./Comp.svelte")` →
    // false (unresolved), so the token is left as `./Comp.svelte.ts`.
    const exists = containingFileAwareExists(ONLY_ABS_COMP_SVELTE, "/app/Parent.ts");
    expect(cleanupCarrierVirtualImportPath("./Comp.svelte.ts", exists)).toBe("./Comp.svelte");
    expect(cleanupCarrierVirtualImportPath('import Comp from "./Comp.svelte.ts"', exists)).toBe(
      'import Comp from "./Comp.svelte"',
    );
  });

  it("leaves a RELATIVE real rune ./store.svelte.ts unchanged when no backing exists (safety)", () => {
    // The safety invariant: a real `./store.svelte.ts` rune module (no
    // `/app/store.svelte` carrier) must NEVER be corrupted into `./store.svelte`
    // even though the candidate is now resolved against the containing dir.
    const exists = containingFileAwareExists(ONLY_ABS_COMP_SVELTE, "/app/Parent.ts");
    expect(cleanupCarrierVirtualImportPath("./store.svelte.ts", exists)).toBe("./store.svelte.ts");
    expect(
      cleanupCarrierVirtualImportPath('import { count } from "./store.svelte.ts"', exists),
    ).toBe('import { count } from "./store.svelte.ts"');
  });

  it("strips a RELATIVE ./Comp.vue.ts by SHAPE regardless of backing (Vue unchanged)", () => {
    // Unambiguous Vue suffixes strip by shape — the containing-file-aware
    // predicate must NOT accidentally backing-gate them. A host that knows NO
    // backing at all still strips `./Comp.vue.ts` → `./Comp.vue`.
    const noBacking = containingFileAwareExists(() => false, "/app/Parent.ts");
    expect(cleanupCarrierVirtualImportPath("./Comp.vue.ts", noBacking)).toBe("./Comp.vue");
    expect(cleanupCarrierVirtualImportPath("./Comp.vue.d.ts", noBacking)).toBe("./Comp.vue");
    expect(cleanupCarrierVirtualImportPath('import Comp from "./Comp.vue.ts"', noBacking)).toBe(
      'import Comp from "./Comp.vue"',
    );
  });

  it("passes an ABSOLUTE candidate through to the host unchanged", () => {
    // An already-absolute backing must not be re-resolved (idempotent for the
    // resolve/upsert path where tokens are already host-rooted).
    const exists = containingFileAwareExists((p) => p === "/app/Comp.svelte", "/app/Parent.ts");
    expect(cleanupCarrierVirtualImportPath("/app/Comp.svelte.ts", exists)).toBe("/app/Comp.svelte");
  });
});
