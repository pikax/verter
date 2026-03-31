/**
 * @ai-generated - Locks the real-project meta-ui benchmark normalization, transform parity, and aggregation helpers.
 */

import { describe, expect, it } from "vitest";

import {
  applyDefaultBenchmarkTransforms,
  compareNormalizedArtifacts,
  normalizeForBenchmark,
  rotateComponentOrder,
  summarizeLatencySeries,
  type NormalizedMetaArtifact,
} from "./meta-ui-core.js";

describe("applyDefaultBenchmarkTransforms", () => {
  it("rewrites MDCSlot and useSlots access into explicit slot usage", () => {
    const source = `<script setup lang="ts">
const slots = useSlots()
const { footer: footerSlot } = useSlots()
</script>
<template>
  <MDCSlot name="title" />
  <div v-if="slots.default">hello</div>
  <div v-if="slots.footer">footer</div>
</template>`;

    const transformed = applyDefaultBenchmarkTransforms(source);

    expect(transformed).toContain(`<slot name="title" />`);
    expect(transformed).toContain(`<slot name="default" />`);
    expect(transformed).toContain(`<slot name="footer" />`);
    expect(transformed).not.toContain("MDCSlot");
  });

  it("rewrites __VLS_export declarations to a stable export default shape", () => {
    const source = `import _default from './component'
declare const __VLS_export: __VLS_WithSlots<import("vue").DefineComponent<{ foo: string }>, __VLS_Slots>
export default _default;`;

    const transformed = applyDefaultBenchmarkTransforms(source);

    expect(transformed).toContain(
      `export default {} as (import("vue").DefineComponent<{ foo: string }> & { new (): { $slots: __VLS_Slots } });`,
    );
    expect(transformed).not.toContain("export default _default;");
    expect(transformed).not.toContain("declare const __VLS_export");
  });
});

describe("normalizeForBenchmark", () => {
  it("sorts collections, keeps empty top-level fields, and strips volatile schema noise", () => {
    const artifact = normalizeForBenchmark(
      "src/runtime/components/Alert.vue",
      {
        componentName: "Alert",
        props: [
          {
            name: "beta",
            type: "boolean",
            required: false,
            description: undefined,
            default: undefined,
            tags: [
              { text: "two", name: "b" },
              { name: "a", text: "one" },
            ],
            schema: { z: 1, a: undefined, loc: { line: 10, column: 2 } },
          },
          {
            name: "alpha",
            type: "string",
            required: true,
            description: "A",
            tags: [],
            schema: { kind: "enum", schema: ["b", "a"] },
          },
        ],
        events: [{ name: "close", type: "void", description: undefined, tags: [], schema: null }],
        slots: [],
        exposed: [],
        models: undefined,
      },
      {
        beta: { type: "boolean", description: undefined, z: 2, a: 1 },
        alpha: { enum: ["b", "a"], type: "string" },
      },
    );

    expect(artifact.componentPath).toBe("src/runtime/components/Alert.vue");
    expect(artifact.props.map((prop) => prop.name)).toEqual(["alpha", "beta"]);
    expect(artifact.models).toEqual([]);
    expect(artifact.diagnostics).toEqual([]);
    expect(artifact.props[1]?.description).toBeNull();
    expect(artifact.props[1]?.default).toBeNull();
    expect(artifact.props[1]?.tags.map((tag) => tag.name)).toEqual(["a", "b"]);
    expect(Object.keys(artifact.propsJsonSchema)).toEqual(["alpha", "beta"]);
    expect(Object.keys(artifact.propsJsonSchema.alpha ?? {})).toEqual(["enum", "type"]);
    expect((artifact.props[1]?.schema as Record<string, unknown>).loc).toBeUndefined();
  });
});

describe("compareNormalizedArtifacts", () => {
  it("reports missing, extra, and field mismatches by collection", () => {
    const baseline: NormalizedMetaArtifact = {
      componentPath: "a.vue",
      componentName: "A",
      props: [
        {
          name: "title",
          type: "string",
          required: true,
          default: null,
          description: null,
          tags: [],
          schema: null,
        },
      ],
      events: [{ name: "close", type: "void", description: null, tags: [], schema: null }],
      slots: [],
      exposed: [],
      models: [],
      propsJsonSchema: { title: { type: "string" } },
      diagnostics: [],
    };

    const actual: NormalizedMetaArtifact = {
      ...baseline,
      props: [
        {
          name: "title",
          type: "number",
          required: true,
          default: null,
          description: null,
          tags: [],
          schema: null,
        },
        {
          name: "tone",
          type: "string",
          required: false,
          default: null,
          description: null,
          tags: [],
          schema: null,
        },
      ],
      events: [],
      propsJsonSchema: {
        title: { type: "number" },
        tone: { type: "string" },
      },
    };

    const comparison = compareNormalizedArtifacts(actual, baseline);

    expect(comparison.exact).toBe(false);
    expect(comparison.totalExtra).toBe(2);
    expect(comparison.totalMissing).toBe(1);
    expect(comparison.totalFieldMismatches).toBe(2);
    expect(comparison.collections.props.extra).toEqual(["tone"]);
    expect(comparison.collections.events.missing).toEqual(["close"]);
  });

  // @ai-generated - Ensures non-equivalent native-only model metadata is excluded from parity totals.
  it("excludes models from parity scoring while keeping the exclusion explicit", () => {
    const baseline: NormalizedMetaArtifact = {
      componentPath: "a.vue",
      componentName: "A",
      props: [],
      events: [],
      slots: [],
      exposed: [],
      models: [],
      propsJsonSchema: {},
      diagnostics: [],
    };

    const actual: NormalizedMetaArtifact = {
      ...baseline,
      models: [{ name: "modelValue", type: "string", description: null, tags: [], schema: null }],
    };

    const comparison = compareNormalizedArtifacts(actual, baseline);

    expect(comparison.exact).toBe(true);
    expect(comparison.totalMissing).toBe(0);
    expect(comparison.totalExtra).toBe(0);
    expect(comparison.totalFieldMismatches).toBe(0);
    expect(comparison.excludedCollections).toEqual(["models"]);
    expect(comparison.collections.models.extra).toEqual(["modelValue"]);
  });
});

describe("rotateComponentOrder", () => {
  it("rotates deterministically by repeat index", () => {
    const order = rotateComponentOrder(["A.vue", "B.vue", "C.vue", "D.vue"], 2);

    expect(order).toEqual(["C.vue", "D.vue", "A.vue", "B.vue"]);
  });
});

describe("summarizeLatencySeries", () => {
  it("computes stable summary stats for report rendering", () => {
    const summary = summarizeLatencySeries([1, 2, 3, 4, 5]);

    expect(summary.min).toBe(1);
    expect(summary.max).toBe(5);
    expect(summary.p50).toBe(3);
    expect(summary.p95).toBe(5);
    expect(summary.mean).toBe(3);
    expect(summary.stddev).toBeGreaterThan(1.4);
  });
});
