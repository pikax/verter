/**
 * Guards for the strict per-carrier position mapping wiring:
 * the playground maps through the CORE `CarrierMapper`/`CarrierMapperSet`
 * ONLY — the hand-rolled closest-segment `SourceMapMapper` and the
 * extension-LAST `vueModuleResolver` are deleted. Pinned to COMMITTED
 * WASM-produced fixtures.
 */
import { describe, it, expect } from "vitest";
import { buildCarrierMapperSet } from "./carrierMappers";
import { fixtures, VROOT } from "./__fixtures__/wasmLsKit";

describe("legacy mapper/resolver deletion", () => {
  it("the hand-rolled snap mapper and extension-LAST resolver modules are gone", () => {
    // import.meta.glob is evaluated at transform time against the REAL file
    // tree — a resurrected legacy module fails this immediately.
    const legacyMapper = import.meta.glob("./sourceMapMapper*.ts");
    const legacyResolver = import.meta.glob("./vueModuleResolver*.ts");
    expect(Object.keys(legacyMapper)).toHaveLength(0);
    expect(Object.keys(legacyResolver)).toHaveLength(0);
  });
});

describe("wasm_strict_no_snap_mapping (#3)", () => {
  const idePath = `${VROOT}/Comp.vue.tsx`;

  function compVueMapperSet() {
    return buildCarrierMapperSet([
      {
        providerPath: idePath,
        code: fixtures.compVue.ide!.code,
        sourceMap: fixtures.compVue.ide!.sourceMap,
      },
    ]);
  }

  it("an off-segment/synthetic generated position DROPS (null) — a closest-segment snap mapper fails this", () => {
    const set = compVueMapperSet();
    const mapper = set.forCarrier(idePath);
    expect(mapper).toBeDefined();
    const code = fixtures.compVue.ide!.code;

    // Synthetic `const ___VERTER___props=defineProps…` sits AFTER the mapped
    // type-argument segment on the same generated line. The legacy
    // closest-segment mapper snapped this to source offset 56 (inside
    // `)\n</script>` — a mis-map); strict mapping fails closed.
    const syntheticDecl = code.indexOf("const ___VERTER___props");
    expect(syntheticDecl).toBeGreaterThan(-1);
    expect(mapper!.mapGeneratedOffsetToSource(syntheticDecl)).toBeNull();

    // The template accessor's synthetic `__props.` prefix: the legacy mapper
    // snapped it to source offset 86 (`count }}`); strict mapping: null.
    const syntheticAccessor = code.indexOf("__props.count");
    expect(syntheticAccessor).toBeGreaterThan(-1);
    expect(mapper!.mapGeneratedOffsetToSource(syntheticAccessor)).toBeNull();

    // Span discipline: a span rooted in synthetic space drops entirely.
    expect(mapper!.mapGeneratedSpanToSource(syntheticDecl, syntheticDecl + 5)).toBeNull();
  });

  it("is not over-strict: a genuinely mapped token maps to the exact source offset", () => {
    const set = compVueMapperSet();
    const mapper = set.forCarrier(idePath)!;
    const code = fixtures.compVue.ide!.code;
    const source = fixtures.compVue.source;

    const genPos = code.indexOf("{ count: number }");
    expect(genPos).toBeGreaterThan(-1);
    const mapped = mapper.mapGeneratedOffsetToSource(genPos);
    expect(mapped).not.toBeNull();
    expect(mapped!.offset).toBe(source.indexOf("{ count: number }"));
  });

  it("an unknown carrier path yields NO mapper and a cross-file edit through it is suppressed", () => {
    const set = compVueMapperSet();
    expect(set.forCarrier(`${VROOT}/Other.vue.tsx`)).toBeUndefined();
    expect(
      set.mapWorkspaceEditToSource([
        { carrierPath: `${VROOT}/Other.vue.tsx`, spans: [{ start: 0, end: 1 }] },
      ]),
    ).toBeNull();
  });

  it("a carrier WITHOUT a source map (the Svelte declaration surface) contributes no mapper — spans drop, never identity-map", () => {
    const declPath = `${VROOT}/Comp.d.svelte.ts`;
    const set = buildCarrierMapperSet([
      {
        providerPath: declPath,
        code: fixtures.compSvelte.decl!.code,
        sourceMap: fixtures.compSvelte.decl!.sourceMap, // null in the fixture
      },
    ]);
    expect(fixtures.compSvelte.decl!.sourceMap).toBeNull();
    expect(set.forCarrier(declPath)).toBeUndefined();
    expect(
      set.mapWorkspaceEditToSource([{ carrierPath: declPath, spans: [{ start: 0, end: 4 }] }]),
    ).toBeNull();
  });
});

describe("wasm_svelte_ide_carrier_parity", () => {
  it("the Svelte .svelte.tsx IDE carrier is captured (non-null) and a known script token maps back through the CORE mapper", () => {
    const fx = fixtures.compSvelte;
    // Parity with Vue: the WASM host must produce the Svelte IDE surface. A
    // wasm32-panicking Svelte IDE path captures `ide: null` + a non-null
    // `ideUnavailable` — this guard discriminates exactly that gap.
    expect(fx.ideUnavailable).toBeNull();
    expect(fx.ide).not.toBeNull();

    const idePath = `${VROOT}/Comp.svelte.tsx`;
    const set = buildCarrierMapperSet([
      { providerPath: idePath, code: fx.ide!.code, sourceMap: fx.ide!.sourceMap },
    ]);
    const mapper = set.forCarrier(idePath);
    expect(mapper).toBeDefined();

    // The `$props()` destructuring's type annotation is authored script text
    // preserved in the carrier: it must map back to the exact source offset —
    // mirroring the Vue IDE-mapping coverage.
    const genPos = fx.ide!.code.indexOf("{ count: number }");
    expect(genPos).toBeGreaterThan(-1);
    const mapped = mapper!.mapGeneratedOffsetToSource(genPos);
    expect(mapped).not.toBeNull();
    expect(mapped!.offset).toBe(fx.source.indexOf("{ count: number }"));

    // Span discipline holds for the Svelte carrier too: both endpoints of the
    // token span map within the source.
    const span = mapper!.mapGeneratedSpanToSource(genPos, genPos + "{ count: number }".length);
    expect(span).not.toBeNull();
    expect(span!.start).toBe(fx.source.indexOf("{ count: number }"));
  });
});

describe("wasm_mapping_is_utf16 (#11)", () => {
  it("a span after astral characters maps by UTF-16 code units", () => {
    const idePath = `${VROOT}/Astral.vue.tsx`;
    const fx = fixtures.astralVue;
    const set = buildCarrierMapperSet([
      { providerPath: idePath, code: fx.ide!.code, sourceMap: fx.ide!.sourceMap },
    ]);
    const mapper = set.forCarrier(idePath)!;

    // The template interpolation `{{ msg }}` sits AFTER two astral emoji
    // (🎉🎉 = 4 UTF-16 code units, 8 UTF-8 bytes, 2 code points) on the SAME
    // source line, so a byte- or codepoint-columned mapper lands 4 (or 2)
    // units off. The generated template emission `{ msg }` must map back to
    // the exact UTF-16 source offset.
    const genPos = fx.ide!.code.lastIndexOf("{ msg }") + 2;
    const expected = fx.source.indexOf("{{ msg }}") + 3;
    const mapped = mapper.mapGeneratedOffsetToSource(genPos);
    expect(mapped).not.toBeNull();
    expect(mapped!.offset).toBe(expected);
    expect(fx.source.slice(mapped!.offset, mapped!.offset + 3)).toBe("msg");

    // Span endpoints both map in UTF-16 space.
    const span = mapper.mapGeneratedSpanToSource(genPos, genPos + 3);
    expect(span).toEqual({ source: "Astral.vue", start: expected, end: expected + 3 });
  });
});
