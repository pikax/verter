import { describe, it, expect, vi } from "vitest";
import { carrierRootMembership, resolveCarrierImportTarget, scriptKindForCarrier } from "./policy";
import type { ManifestRole, ManifestScriptKind, OwnedSource } from "./store";
import { toImportSurfaceFileName } from "./naming";
import { VIRTUAL_FILE_NAMING } from "../virtual-file-naming.generated";

/**
 * The injected TS facade: the REAL `ts.ScriptKind` numeric values (JS=1,
 * JSX=2, TS=3, TSX=4) without importing the `typescript` package — the CORE
 * must stay free of a module-scope `typescript` import, so hosts inject the
 * enum values.
 */
const TS_FACADE = { ScriptKind: { JS: 1, JSX: 2, TS: 3, TSX: 4 } } as const;

describe("scriptKindForCarrier (descriptor-derived, TS facade injected)", () => {
  it("classifies IDE carriers as TSX (or JSX for the JS-source branch)", () => {
    expect(scriptKindForCarrier("/ws/Comp.vue.tsx", TS_FACADE)).toBe(4);
    expect(scriptKindForCarrier("/ws/Widget.svelte.tsx", TS_FACADE)).toBe(4);
    // Both component adapters publish `.jsx` for JavaScript-source carriers.
    expect(scriptKindForCarrier("/ws/Comp.vue.jsx", TS_FACADE)).toBe(2);
    expect(scriptKindForCarrier("/ws/Widget.svelte.jsx", TS_FACADE)).toBe(2);
    // Backslash (Windows) spelling normalizes.
    expect(scriptKindForCarrier("d:\\ws\\Comp.vue.tsx", TS_FACADE)).toBe(4);
  });

  it("classifies declaration carriers (.d.<ext>.ts, extension-middle) as TS", () => {
    expect(scriptKindForCarrier("/ws/Comp.d.vue.ts", TS_FACADE)).toBe(3);
    expect(scriptKindForCarrier("/ws/Widget.d.svelte.ts", TS_FACADE)).toBe(3);
  });

  it("classifies API carriers (.<ext>.verter.ts import surface) as TS", () => {
    expect(scriptKindForCarrier("/ws/Comp.vue.verter.ts", TS_FACADE)).toBe(3);
    expect(scriptKindForCarrier("/ws/Widget.svelte.verter.ts", TS_FACADE)).toBe(3);
  });

  it("returns undefined for non-carrier paths (host falls through)", () => {
    // A plain TS file.
    expect(scriptKindForCarrier("/ws/util.ts", TS_FACADE)).toBeUndefined();
    // A bare carrier SOURCE is not a generated carrier virtual file.
    expect(scriptKindForCarrier("/ws/Comp.vue", TS_FACADE)).toBeUndefined();
    // A REAL Svelte rune module (selfFile row) is NOT a carrier virtual file —
    // classifying it as one would corrupt a real user module.
    expect(scriptKindForCarrier("/ws/store.svelte.ts", TS_FACADE)).toBeUndefined();
  });
});

describe("carrierRootMembership (program-membership policy)", () => {
  it("IDE carriers are self-diagnostic ROOTS", () => {
    expect(carrierRootMembership("/ws/Comp.vue.tsx")).toBe("selfDiagnosticRoot");
    expect(carrierRootMembership("/ws/Widget.svelte.tsx")).toBe("selfDiagnosticRoot");
    expect(carrierRootMembership("/ws/Widget.svelte.jsx")).toBe("selfDiagnosticRoot");
  });

  it("declaration carriers are import-driven (NOT roots)", () => {
    expect(carrierRootMembership("/ws/Comp.d.vue.ts")).toBe("importDriven");
    expect(carrierRootMembership("/ws/Widget.d.svelte.ts")).toBe("importDriven");
  });

  it("API carriers are redirect-reached", () => {
    expect(carrierRootMembership("/ws/Comp.vue.verter.ts")).toBe("redirectReached");
    expect(carrierRootMembership("/ws/Widget.svelte.verter.ts")).toBe("redirectReached");
  });

  it("returns null for non-carrier paths (including real rune modules)", () => {
    expect(carrierRootMembership("/ws/util.ts")).toBeNull();
    expect(carrierRootMembership("/ws/Comp.vue")).toBeNull();
    expect(carrierRootMembership("/ws/store.svelte.ts")).toBeNull();
  });
});

describe("resolveCarrierImportTarget (ordinary-import surface selection)", () => {
  interface Row {
    source_uri: string;
    provider_uri: string;
    role: ManifestRole;
    script_kind: ManifestScriptKind;
  }

  /**
   * A minimal owned-set reader over the shared contract shape. `caseSensitive`
   * models the host filesystem identity policy the Node/WASM readers apply in
   * `canonicalPath` (Windows drive-letter case + NTFS/APFS folding).
   */
  function reader(rows: readonly Row[], caseSensitive = true) {
    const key = (p: string) => {
      const normalized = p.replace(/\\/g, "/");
      return caseSensitive ? normalized : normalized.toLowerCase();
    };
    return {
      canonicalPath: key,
      ownedSourceFor(path: string): OwnedSource | undefined {
        const wanted = key(path);
        return rows.find(
          (row) => key(row.source_uri) === wanted || key(row.provider_uri) === wanted,
        );
      },
    };
  }

  const ideRow = (ext: string): Row => ({
    source_uri: `/ws/src/A${ext}`,
    provider_uri: `/ws/src/A${ext}.tsx`,
    role: "CarrierIde",
    script_kind: "TSX",
  });
  const apiRow = (ext: string): Row => ({
    source_uri: `/ws/src/A${ext}`,
    provider_uri: `/ws/src/A${ext}.verter.ts`,
    role: "CarrierApi",
    script_kind: "TS",
  });

  it("selects the descriptor-generated import surface for every component carrier", () => {
    // Vue and Svelte take the SAME path — no per-framework branch.
    for (const ext of [".vue", ".svelte"]) {
      const target = resolveCarrierImportTarget(
        reader([ideRow(ext), apiRow(ext)]),
        `/ws/src/A${ext}`,
      );
      expect(target).toEqual({ kind: "resolve", provider: `/ws/src/A${ext}.verter.ts` });
    }
  });

  it("ABSTAINS when the project owns no import-surface row — never the JSX IDE carrier", () => {
    for (const ext of [".vue", ".svelte"]) {
      const target = resolveCarrierImportTarget(reader([ideRow(ext)]), `/ws/src/A${ext}`);
      expect(target).toEqual({ kind: "abstain", reason: "unowned" });
      // The defect being fixed: an ordinary import must NEVER be handed a JSX surface.
      expect(JSON.stringify(target)).not.toContain(".tsx");
      expect(JSON.stringify(target)).not.toContain(".jsx");
    }
  });

  it("selects the TS import surface even for a JavaScript carrier published as .jsx", () => {
    const jsIde: Row = {
      source_uri: "/ws/src/A.vue",
      provider_uri: "/ws/src/A.vue.jsx",
      role: "CarrierIde",
      script_kind: "JSX",
    };
    const target = resolveCarrierImportTarget(reader([jsIde, apiRow(".vue")]), "/ws/src/A.vue");
    expect(target).toEqual({ kind: "resolve", provider: "/ws/src/A.vue.verter.ts" });
  });

  it("matches the owned row through the host's canonical identity (drive case / separators)", () => {
    const rows: Row[] = [
      {
        source_uri: "d:/ws/src/A.vue",
        provider_uri: "d:/ws/src/A.vue.verter.ts",
        role: "CarrierApi",
        script_kind: "TS",
      },
    ];
    const target = resolveCarrierImportTarget(reader(rows, false), "D:\\ws\\src\\A.vue");
    expect(target).toEqual({ kind: "resolve", provider: "d:/ws/src/A.vue.verter.ts" });
  });

  it("abstains for a non-carrier path and for a real self-file rune module", () => {
    expect(resolveCarrierImportTarget(reader([]), "/ws/src/util.ts")).toEqual({
      kind: "abstain",
      reason: "notCarrier",
    });
    expect(resolveCarrierImportTarget(reader([]), "/ws/src/store.svelte.ts")).toEqual({
      kind: "abstain",
      reason: "notCarrier",
    });
  });

  it("refuses an owned row whose provider is not the descriptor-generated import surface", () => {
    const forged: Row = {
      source_uri: "/ws/src/A.vue",
      provider_uri: "/ws/src/A.vue.tsx",
      role: "CarrierApi",
      script_kind: "TSX",
    };
    expect(resolveCarrierImportTarget(reader([forged]), "/ws/src/A.vue")).toEqual({
      kind: "abstain",
      reason: "unowned",
    });
  });

  it("refuses a row REACHED by the derived surface whose provider points somewhere else", () => {
    // The reader contract matches a query against EITHER `source_uri` OR
    // `provider_uri`, so a row whose SOURCE happens to be spelled like the
    // derived surface is reachable. Returning its `provider_uri` unchecked
    // would hand an ordinary import a JSX carrier — the one thing this policy
    // exists to prevent.
    const misdirected: Row = {
      source_uri: "/ws/src/A.vue.verter.ts",
      provider_uri: "/ws/src/A.vue.tsx",
      role: "CarrierApi",
      script_kind: "TSX",
    };
    expect(resolveCarrierImportTarget(reader([misdirected]), "/ws/src/A.vue")).toEqual({
      kind: "abstain",
      reason: "unowned",
    });
  });

  it("picks the LONGEST carrier extension before interpreting its import surface", async () => {
    // Constructed against an OVERLAPPING descriptor pair, because the shipped
    // rows cannot discriminate: `.svelte.ts` does not end in `.svelte`, so a
    // filter-first implementation and a longest-match-first one agree on every
    // real path today. Here a hypothetical `.component.vue` SELF-FILE adapter
    // overlaps the `.vue` component row: filtering the self-file row out BEFORE
    // the longest match lets the shorter `.vue` row claim `Card.component.vue`
    // and fabricate `Card.component.vue.verter.ts` for a file that serves its
    // own path.
    vi.resetModules();
    vi.doMock("../virtual-file-naming.generated", async () => {
      const actual = await vi.importActual<typeof import("../virtual-file-naming.generated")>(
        "../virtual-file-naming.generated",
      );
      return {
        ...actual,
        VIRTUAL_FILE_NAMING: {
          ...actual.VIRTUAL_FILE_NAMING,
          FRAMEWORK_TAG_OVERLAPPING: {
            carrierExtension: ".component.vue",
            ide: { kind: "selfFile" },
            importSurface: { kind: "selfFile" },
            testingApiSuffix: null,
            sidecarSuffixes: [],
            declarationSurface: { kind: "none" },
          },
        },
      };
    });
    const naming = await import("./naming");

    // The longer self-file row OWNS this path, so there is no import surface…
    expect(naming.toImportSurfaceFileName("/ws/src/Card.component.vue")).toBeNull();
    // …while a plain `.vue` component is unaffected.
    expect(naming.toImportSurfaceFileName("/ws/src/Card.vue")).toBe("/ws/src/Card.vue.verter.ts");

    vi.doUnmock("../virtual-file-naming.generated");
    vi.resetModules();
  });
});
