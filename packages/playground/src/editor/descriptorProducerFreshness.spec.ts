/**
 * descriptor_producer_freshness (#12): the CORE `toDeclarationCarrierFileName`
 * producer matches the generated descriptor mirror (`VIRTUAL_FILE_NAMING`,
 * itself byte-pinned to the Rust `declaration_carrier_identity` column by the
 * `virtual_file_naming_ts_freshness` guard) for Vue AND Svelte, and the
 * committed WASM fixtures are served under exactly the producer-named paths.
 */
import { describe, it, expect } from "vitest";
import { toDeclarationCarrierFileName, VIRTUAL_FILE_NAMING } from "@verter/language-shared";
import { fixtures } from "./__fixtures__/wasmLsKit";

describe("descriptor_producer_freshness (#12)", () => {
  it("every extensionMiddleTs descriptor row produces {stem}.d{ext}.ts — never extension-LAST", () => {
    const middleRows = Object.entries(VIRTUAL_FILE_NAMING).filter(
      ([, row]) =>
        row.carrierExtension !== null && row.declarationSurface.kind === "extensionMiddleTs",
    );
    // Vue AND Svelte are both extension-middle declaration carriers.
    const extensions = middleRows.map(([, row]) => row.carrierExtension);
    expect(extensions).toContain(".vue");
    expect(extensions).toContain(".svelte");

    for (const [tag, row] of middleRows) {
      const ext = row.carrierExtension as string;
      const produced = toDeclarationCarrierFileName(`/x/Comp${ext}`);
      expect(produced, tag).toBe(`/x/Comp.d${ext}.ts`);
      expect(produced, tag).not.toBe(`/x/Comp${ext}.d.ts`);
    }
  });

  it("rows without a declaration surface (and non-carriers) produce null", () => {
    const noneRows = Object.entries(VIRTUAL_FILE_NAMING).filter(
      ([, row]) => row.carrierExtension !== null && row.declarationSurface.kind === "none",
    );
    for (const [tag, row] of noneRows) {
      const ext = row.carrierExtension as string;
      expect(toDeclarationCarrierFileName(`/x/store${ext}`), tag).toBeNull();
    }
    // The Svelte rune module wins by longest suffix — never mis-classified
    // as a `.svelte` component carrier.
    expect(toDeclarationCarrierFileName("/x/store.svelte.ts")).toBeNull();
    expect(toDeclarationCarrierFileName("/x/plain.ts")).toBeNull();
    // A bare extension with no basename stem is not a carrier path.
    expect(toDeclarationCarrierFileName("/x/.vue")).toBeNull();
  });

  it("the committed WASM fixtures pair with the producer-named declaration paths for Vue AND Svelte", () => {
    expect(toDeclarationCarrierFileName(fixtures.compVue.filename)).toBe("Comp.d.vue.ts");
    expect(toDeclarationCarrierFileName(fixtures.compSvelte.filename)).toBe("Comp.d.svelte.ts");
    // Both fixture declaration surfaces are REAL WASM output for those paths.
    expect(fixtures.compVue.decl?.code.length).toBeGreaterThan(0);
    expect(fixtures.compSvelte.decl?.code.length).toBeGreaterThan(0);
  });
});
