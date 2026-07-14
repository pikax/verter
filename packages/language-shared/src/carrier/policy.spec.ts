import { describe, it, expect } from "vitest";
import { carrierRootMembership, scriptKindForCarrier } from "./policy";

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
    // Vue's jsxConditional IDE policy has a `.jsx` branch for JS-source carriers.
    expect(scriptKindForCarrier("/ws/Comp.vue.jsx", TS_FACADE)).toBe(2);
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
