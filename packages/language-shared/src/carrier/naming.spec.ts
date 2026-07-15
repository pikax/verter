import { describe, expect, it } from "vitest";
import {
  cleanupCarrierVirtualImportPath,
  containingFileAwareExists,
  isVue,
  toDeclarationCarrierFileName,
  toIdeCarrierFileName,
} from "./naming";

describe("toDeclarationCarrierFileName (extension-MIDDLE declaration carrier)", () => {
  it("maps a .vue carrier to the extension-middle .d.vue.ts declaration carrier", () => {
    expect(toDeclarationCarrierFileName("/x/B.vue")).toBe("/x/B.d.vue.ts");
  });

  it("maps a .svelte carrier to the extension-middle .d.svelte.ts declaration carrier", () => {
    expect(toDeclarationCarrierFileName("/x/B.svelte")).toBe("/x/B.d.svelte.ts");
  });

  it("is NEVER the extension-LAST spelling and NEVER carries the .verter. infix", () => {
    const vue = toDeclarationCarrierFileName("/x/B.vue");
    expect(vue).not.toBeNull();
    // Extension-MIDDLE (`B.d.vue.ts`), never extension-LAST (`B.vue.d.ts`) —
    // tsgo would not bare-resolve the extension-last form.
    expect(vue).not.toBe("/x/B.vue.d.ts");
    expect(vue).not.toContain(".vue.d.ts");
    expect(vue!.endsWith(".d.vue.ts")).toBe(true);
    // The reserved `.verter.` infix lives ONLY on the redirect-reached import
    // surface (`.verter.ts`), never on the bare-probed declaration carrier.
    expect(vue).not.toContain(".verter.");

    const svelte = toDeclarationCarrierFileName("/x/B.svelte");
    expect(svelte).not.toBe("/x/B.svelte.d.ts");
    expect(svelte).not.toContain(".svelte.d.ts");
    expect(svelte!.endsWith(".d.svelte.ts")).toBe(true);
    expect(svelte).not.toContain(".verter.");
  });

  it("agrees with the Rust declaration_carrier_identity spelling (descriptor.rs fixture)", () => {
    // The EXACT fixture values pinned by the Rust authority's test
    // `declaration_carrier_identity_inserts_d_infix_in_extension_middle_form`
    // (`crates/verter_session/src/framework/descriptor.rs`):
    //   vue_descriptor().declaration_carrier_identity("/ws/src/Foo.vue")
    //     == Some("/ws/src/Foo.d.vue.ts")
    //   svelte_descriptor().declaration_carrier_identity("/ws/src/Foo.svelte")
    //     == Some("/ws/src/Foo.d.svelte.ts")
    expect(toDeclarationCarrierFileName("/ws/src/Foo.vue")).toBe("/ws/src/Foo.d.vue.ts");
    expect(toDeclarationCarrierFileName("/ws/src/Foo.svelte")).toBe("/ws/src/Foo.d.svelte.ts");
  });

  it("returns null for a rune-module source (declarationSurface: none)", () => {
    // `store.svelte.ts` is the standalone Svelte rune-module extension — its
    // descriptor row projects NO declaration carrier. Longest-suffix matching
    // must classify it as the rune row, never as a `.svelte` component.
    expect(toDeclarationCarrierFileName("/x/store.svelte.ts")).toBeNull();
  });

  it("returns null for a non-carrier source (never fabricates Foo.d.ts.ts)", () => {
    expect(toDeclarationCarrierFileName("/x/util.ts")).toBeNull();
    expect(toDeclarationCarrierFileName("/x/util.js")).toBeNull();
    expect(toDeclarationCarrierFileName("/x/Foo")).toBeNull();
  });

  it("returns null when the carrier extension has no non-empty basename stem", () => {
    // Mirrors the Rust rule: the char before the extension must be a real
    // basename character, not a path separator, and the stem must not be empty.
    expect(toDeclarationCarrierFileName(".vue")).toBeNull();
    expect(toDeclarationCarrierFileName("/x/.vue")).toBeNull();
  });

  it("normalizes backslashes before composing", () => {
    expect(toDeclarationCarrierFileName("d:\\src\\Comp.vue")).toBe("d:/src/Comp.d.vue.ts");
  });
});

describe("relocated carrier naming CORE (browser-safe surface smoke)", () => {
  it("isVue classifies carriers from the descriptor mirror", () => {
    expect(isVue("./Foo.vue")).toBe(true);
    expect(isVue("./Comp.svelte")).toBe(true);
    expect(isVue("./Foo.vue.verter.ts")).toBe(false);
    expect(isVue("./util.ts")).toBe(false);
  });

  it("toIdeCarrierFileName maps a carrier to its IDE companion", () => {
    expect(toIdeCarrierFileName("/src/Comp.vue")).toBe("/src/Comp.vue.tsx");
    expect(toIdeCarrierFileName("/src/Comp.svelte")).toBe("/src/Comp.svelte.tsx");
    expect(toIdeCarrierFileName("/src/util.ts")).toBeNull();
  });

  it("cleanupCarrierVirtualImportPath strips the unambiguous Vue API carrier by shape", () => {
    expect(cleanupCarrierVirtualImportPath('import Foo from "./Foo.vue.verter.ts"')).toBe(
      'import Foo from "./Foo.vue"',
    );
  });

  it("containingFileAwareExists resolves a relative backing candidate WITHOUT Node builtins", () => {
    // The browser-safe internal POSIX resolver must reproduce the Node path.posix
    // behavior this wrapper shipped with: the host only knows the ABSOLUTE
    // backing `/app/Comp.svelte`, the token is RELATIVE, and the ambiguous
    // Svelte suffix strips only because the candidate is resolved against the
    // containing file's directory.
    const exists = containingFileAwareExists((p) => p === "/app/Comp.svelte", "/app/Parent.ts");
    expect(cleanupCarrierVirtualImportPath("./Comp.svelte.verter.ts", exists)).toBe(
      "./Comp.svelte",
    );
    // Safety negative: a real rune module with no backing carrier is untouched.
    expect(cleanupCarrierVirtualImportPath("./store.svelte.ts", exists)).toBe("./store.svelte.ts");
  });
});
