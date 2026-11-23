import { withNativeTypeFacts } from "./type-graph.test-utils.js";
/**
 * @ai-generated
 *
 * Tests for `ComponentMetaSession.getComponentMetaBatch`.
 *
 * Batch projection contract:
 *
 * - N inputs → N positional results.
 * - Per-id misses surface as the empty-meta default.
 * - Per-id slots align positionally with input order.
 * - An unavailable batch method throws without issuing scalar calls.
 */

import { describe, it, expect, vi } from "vitest";
import { ComponentMetaSession } from "./project.js";
import { ComponentMetaChecker } from "./compat/checker.js";

function resolvedTypeRow(display: string) {
  return {
    publication: {
      kind: "published",
      semanticAuthority: "resolved",
      exactness: "exactConcrete",
      reason: { kind: "resolvedExactConcrete" },
      provenance: { kind: "resolved", value: "semanticEvaluator" },
    } as const,
    terminalDisplay: { text: display },
  };
}

function nativeMetaPayload(filePath: string, propName: string) {
  return withNativeTypeFacts({
    filePath,
    optionsApi: false,
    props: [
      {
        name: propName,
        type: { kind: "primitive", name: "string" },
        typeFact: { kind: "complete", value: { kind: "primitive", name: "string" } },
        ...resolvedTypeRow("string"),
        required: true,
        hasDefault: false,
      },
    ],
    events: [],
    slots: [],
    models: [],
    exposed: [],
    components: [],
    templateRefs: [],
    imports: [],
    bindings: [],
    vueApiCalls: [],
    styles: [],
    flags: {
      asyncSetup: false,
      hasReactiveState: false,
      hasComputed: false,
      hasWatchers: false,
      hasLifecycleHooks: false,
      hasProvide: false,
      hasInject: false,
      hasInheritAttrsFalse: false,
      hasStoreUsage: false,
    },
    acceptedProps: [],
    acceptedEvents: [],
    acceptedSurfaceCompleteness: "exact",
    rootReachability: { kind: "noFallthrough", reason: "noTemplate" },
    fallthroughSurface: { kind: "none", reason: "noTemplate" },
    orderedSfcStructure: {
      schemaVersion: 1,
      artifactToken: "a".repeat(43),
      blocks: [],
      markupNodes: [],
    },
  });
}

describe("ComponentMetaSession.getComponentMetaBatch", () => {
  it("returns N positional slots for N inputs", async () => {
    const inputs = ["Alpha.vue", "Bravo.vue", "Charlie.vue"];
    const propNames = ["alpha_prop", "bravo_prop", "charlie_prop"];

    const getComponentMetaBatch = vi.fn((ids: string[]) =>
      ids.map((id, i) => nativeMetaPayload(id, propNames[i] ?? "default")),
    );

    const session = {
      closed: false,
      engine: { state: "active" as const, clearCaches() {} },
      upsert() {},
      delete() {},
      getResolvedComponentMeta: vi.fn(),
      getComponentMeta: vi.fn(),
      getComponentMetaBatch,
      getEffectiveSource() {
        return `<script setup lang="ts">defineProps<{ x: string }>()</script>`;
      },
      hasFile() {
        return true;
      },
      trackedFileIds() {
        return [];
      },
      close() {},
    };

    const project = new ComponentMetaSession(session as any, "/test");
    const results = await project.getComponentMetaBatch(inputs);

    expect(results).toHaveLength(inputs.length);
    expect(results[0].props.map((p) => p.name)).toEqual([propNames[0]]);
    expect(results[1].props.map((p) => p.name)).toEqual([propNames[1]]);
    expect(results[2].props.map((p) => p.name)).toEqual([propNames[2]]);
    expect(getComponentMetaBatch).toHaveBeenCalledOnce();
    // The native batch surface gets resolved canonical paths.
    expect(getComponentMetaBatch).toHaveBeenCalledWith([
      "/test/Alpha.vue",
      "/test/Bravo.vue",
      "/test/Charlie.vue",
    ]);
  });

  it("preserves input order across non-alphabetic inputs", async () => {
    const inputs = ["Zulu.vue", "Alpha.vue", "Mike.vue"];
    // Per-id deterministic prop name derived from the file's leaf
    // basename so the assertion below remains stable regardless of the
    // canonical-path prefix the consumer threads in.
    const getComponentMetaBatch = vi.fn((ids: string[]) =>
      ids.map((id) => {
        const leaf = id.substring(id.lastIndexOf("/") + 1).replace(".vue", "");
        return nativeMetaPayload(id, `${leaf}_prop`);
      }),
    );

    const session = {
      closed: false,
      engine: { state: "active" as const, clearCaches() {} },
      upsert() {},
      delete() {},
      getResolvedComponentMeta: vi.fn(),
      getComponentMeta: vi.fn(),
      getComponentMetaBatch,
      getEffectiveSource() {
        return `<script setup lang="ts">defineProps<{ x: string }>()</script>`;
      },
      hasFile() {
        return true;
      },
      trackedFileIds() {
        return [];
      },
      close() {},
    };

    const project = new ComponentMetaSession(session as any, "/test");
    const results = await project.getComponentMetaBatch(inputs);

    expect(results[0].props.map((p) => p.name)).toEqual(["Zulu_prop"]);
    expect(results[1].props.map((p) => p.name)).toEqual(["Alpha_prop"]);
    expect(results[2].props.map((p) => p.name)).toEqual(["Mike_prop"]);
  });

  it("returns empty-meta default for null slots (missing canonical)", async () => {
    const inputs = ["Real.vue", "Missing.vue"];
    const getComponentMetaBatch = vi.fn((ids: string[]) =>
      ids.map((id) => (id.includes("Missing") ? null : nativeMetaPayload(id, "real_prop"))),
    );

    const session = {
      closed: false,
      engine: { state: "active" as const, clearCaches() {} },
      upsert() {},
      delete() {},
      getResolvedComponentMeta: vi.fn(),
      getComponentMeta: vi.fn(),
      getComponentMetaBatch,
      getEffectiveSource() {
        return `<script setup lang="ts">defineProps<{ x: string }>()</script>`;
      },
      hasFile() {
        return true;
      },
      trackedFileIds() {
        return [];
      },
      close() {},
    };

    const project = new ComponentMetaSession(session as any, "/test");
    const results = await project.getComponentMetaBatch(inputs);

    expect(results).toHaveLength(2);
    expect(results[0].props.map((p) => p.name)).toEqual(["real_prop"]);
    // Missing slot surfaces as empty-meta.
    expect(results[1].props).toEqual([]);
    expect(results[1].events).toEqual([]);
    expect(results[1].slots).toEqual([]);
    expect(results[1].exposed).toEqual([]);
  });

  it("throws without scalar calls when the batch method is absent", async () => {
    const inputs = ["A.vue", "B.vue"];
    const getComponentMeta = vi.fn((id: string) => {
      const leaf = id.substring(id.lastIndexOf("/") + 1).replace(".vue", "");
      return nativeMetaPayload(id, `${leaf}_prop`);
    });

    // No `getComponentMetaBatch` on this session — falls back.
    const session = {
      closed: false,
      engine: { state: "active" as const, clearCaches() {} },
      upsert() {},
      delete() {},
      getResolvedComponentMeta: vi.fn(),
      getComponentMeta,
      getEffectiveSource() {
        return `<script setup lang="ts">defineProps<{ x: string }>()</script>`;
      },
      hasFile() {
        return true;
      },
      trackedFileIds() {
        return [];
      },
      close() {},
    };

    const project = new ComponentMetaSession(session as any, "/test");
    await expect(project.getComponentMetaBatch(inputs)).rejects.toThrow(
      /batch query is unavailable/i,
    );
    expect(getComponentMeta).not.toHaveBeenCalled();
  });
});

describe("shared native projection", () => {
  it("projects the same fixture through checker and session scalar and batch calls", async () => {
    const native = nativeMetaPayload("/test/Order.vue", "order");
    native.props[0].type = {
      kind: "union",
      types: [
        { kind: "literal", literalKind: "string", value: "a" },
        { kind: "literal", literalKind: "string", value: "b" },
      ],
    } as any;
    native.props[0].typeFact.value = native.props[0].type;
    native.props[0].terminalDisplay.text = '"a" | "b"';
    (native.props[0] as any).tags = [{ name: "defaultValue", text: "'b'" }];
    (native as any).acceptedProps = [
      {
        ...native.props[0],
        provenance: { kind: "declared" },
        availability: { kind: "always" },
        kind: "declaredProp",
      },
      {
        name: "id",
        type: { kind: "primitive", name: "string" },
        typeFact: { kind: "complete", value: { kind: "primitive", name: "string" } },
        terminalDisplay: { text: "string" },
        required: false,
        provenance: { kind: "inherited", sources: [{ kind: "nativeTag", tag: "div" }] },
        availability: { kind: "always" },
        kind: "attr",
      },
    ];
    const getComponentMeta = vi.fn(() => native);
    const getComponentMetaBatch = vi.fn(() => [native]);
    const session = {
      closed: false,
      engine: { state: "active" },
      getComponentMeta,
      getComponentMetaBatch,
      getEffectiveSource: () => "<script setup />",
      hasFile: () => true,
    };
    const checker = new ComponentMetaChecker({} as any, "/test", {}, session as any);
    const project = new ComponentMetaSession(session as any, "/test");
    const expected = await checker.getComponentMeta("Order.vue");
    expect(expected.props[0].type).toBe('"a" | "b"');
    expect(expected._verter?.acceptedProps?.map((prop) => prop.name)).toEqual(["order"]);
    expect((await checker.getComponentMetaBatch(["Order.vue"]))[0]).toEqual(expected);
    expect(await project.getComponentMeta("Order.vue")).toEqual(expected);
    expect((await project.getComponentMetaBatch(["Order.vue"]))[0]).toEqual(expected);
    expect(getComponentMeta).toHaveBeenCalledTimes(2);
    expect(getComponentMetaBatch).toHaveBeenCalledTimes(2);
  });
});
