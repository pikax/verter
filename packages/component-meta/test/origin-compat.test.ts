import { describe, test, expect, afterAll } from "vitest";
import { join } from "path";
import { createCheckerByJson } from "../src/compat/checker.js";
import { shutdownMetaRuntime } from "../src/runtime/index.js";
import { getMetaOrigin, walkOriginChain, findOriginNodesByKind } from "../src/origin-walk.js";
import type { NativeOriginGraph } from "../src/native-component-meta.js";

const fixtureDir = join(__dirname, "fixtures");

afterAll(() => {
  shutdownMetaRuntime();
});

describe("origin graph pipeline", () => {
  test("GenericProps.vue produces component meta with origin field shape", async () => {
    const checker = await createCheckerByJson(fixtureDir, {
      compilerOptions: { strict: true },
      include: ["**/*.vue", "**/*.ts"],
    });
    const meta = await checker.getComponentMeta(join(fixtureDir, "GenericProps.vue"));

    expect(meta.props.length).toBeGreaterThan(0);
    expect(meta.props.some((p) => p.name === "value")).toBe(true);
    expect(meta.props.some((p) => p.name === "label")).toBe(true);

    const verter = (meta as Record<string, unknown>)._verter as Record<string, unknown> | undefined;
    const nativeMeta = meta as Record<string, unknown>;
    const origin = (verter?.origin ?? nativeMeta.origin) as NativeOriginGraph | undefined;
    if (origin !== undefined && origin.edges.length > 0) {
      expect(origin.nodes.length).toBeGreaterThan(0);
      expect(origin.metaStrings).toBeInstanceOf(Array);
      for (const edge of origin.edges) {
        expect(typeof edge.source).toBe("number");
        expect(typeof edge.target).toBe("number");
        expect(typeof edge.kind).toBe("string");
      }
    }
  });
});

describe("origin-walk API", () => {
  const testGraph: NativeOriginGraph = {
    nodes: [
      { id: 0, kind: "Object", label: "{...}" },
      { id: 1, kind: "Primitive", label: "string" },
      { id: 2, kind: "TypeParam", label: "T" },
    ],
    edges: [
      { source: 0, target: 1, kind: "instantiate" },
      { source: 0, target: 2, kind: "substituteTypeParam", metaIndex: 0 },
      { source: 2, target: 1, kind: "projectMember" },
    ],
    metaStrings: ['SubstitutedParam("T")'],
  };

  test("getMetaOrigin returns edges and targets for a node", () => {
    const result = getMetaOrigin(testGraph, 0);
    expect(result).toBeDefined();
    expect(result!.node.id).toBe(0);
    expect(result!.edges).toHaveLength(2);
    expect(result!.edges[0].kind).toBe("instantiate");
    expect(result!.edges[1].kind).toBe("substituteTypeParam");
    expect(result!.targets).toHaveLength(2);
  });

  test("getMetaOrigin returns undefined for missing node", () => {
    expect(getMetaOrigin(testGraph, 99)).toBeUndefined();
  });

  test("walkOriginChain performs BFS from start node", () => {
    const chain = walkOriginChain(testGraph, 0);
    expect(chain).toHaveLength(3);
    expect(chain[0].node.id).toBe(0);
    expect(chain[0].depth).toBe(0);
    expect(chain[0].edge).toBeUndefined();

    expect(chain[1].depth).toBe(1);
    expect(chain[1].edge).toBeDefined();

    expect(chain[2].depth).toBe(1);
  });

  test("walkOriginChain handles cycles", () => {
    const cyclicGraph: NativeOriginGraph = {
      nodes: [
        { id: 0, kind: "Object" },
        { id: 1, kind: "Object" },
      ],
      edges: [
        { source: 0, target: 1, kind: "instantiate" },
        { source: 1, target: 0, kind: "instantiate" },
      ],
      metaStrings: [],
    };
    const chain = walkOriginChain(cyclicGraph, 0);
    expect(chain).toHaveLength(2);
  });

  test("findOriginNodesByKind filters by kind string", () => {
    const objects = findOriginNodesByKind(testGraph, "Object");
    expect(objects).toHaveLength(1);
    expect(objects[0].id).toBe(0);

    const primitives = findOriginNodesByKind(testGraph, "Primitive");
    expect(primitives).toHaveLength(1);
    expect(primitives[0].label).toBe("string");

    const missing = findOriginNodesByKind(testGraph, "NonExistent");
    expect(missing).toHaveLength(0);
  });
});
