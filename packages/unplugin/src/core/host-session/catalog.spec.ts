import { describe, expect, it } from "vitest";
import { type CarrierRecord, ModuleCatalog } from "./catalog";
import type {
  BundlerGraphKind,
  BundlerModuleId,
  BundlerSourceRevision,
  HostCanonicalId,
} from "./identity";

const carrierA = "/app/A.vue" as HostCanonicalId;
const carrierB = "/app/B.vue" as HostCanonicalId;
const rev1 = "sha256:one" as BundlerSourceRevision;
const rev2 = "sha256:two" as BundlerSourceRevision;
const client: BundlerGraphKind = "client-dev";
const server: BundlerGraphKind = "ssr-dev";

function newCatalog(): ModuleCatalog {
  return new ModuleCatalog(
    (record, role) =>
      JSON.stringify([record.carrier, record.graph, record.framework, role]) as BundlerModuleId,
  );
}

function recordOf(overrides: Partial<CarrierRecord> = {}): CarrierRecord {
  return {
    carrier: carrierA,
    rawPath: carrierA,
    graph: client,
    framework: "vue",
    revision: rev1,
    epoch: 0,
    profile: {
      isProduction: false,
      customElement: false,
      ssr: false,
      forceJs: false,
      hmrStrategy: "none",
      sourceMap: false,
    },
    main: null,
    authoredStyles: [],
    compiledStyles: [],
    nodes: [],
    closure: new Set(),
    ...overrides,
  };
}

const main = (code: string) => ({ code, map: null, lang: null });
const authored = (token: string) => ({
  blockToken: token,
  content: ".a {}",
  lang: "css",
  scoped: false,
  module: false as const,
});
const compiled = (code: string) => ({ code, map: null, lang: "css" });

describe("ModuleCatalog", () => {
  it("lists main, script, every style and every other node of a record", () => {
    const catalog = newCatalog();
    catalog.publish(
      recordOf({
        main: main("export default {}"),
        authoredStyles: [authored("a"), authored("b")],
        compiledStyles: [compiled("1"), compiled("2"), compiled("3")],
        nodes: [
          { node: { kind: "template" }, code: "", meta: {} },
          { node: { kind: "custom", index: 1 }, code: "", meta: { blockType: "i18n" } },
        ],
      }),
    );
    const roles = catalog.modules(carrierA, client).map((entry) => entry.role);
    expect(roles).toEqual([
      { kind: "main" },
      { kind: "script" },
      { kind: "style", index: 0 },
      { kind: "style", index: 1 },
      { kind: "style", index: 2 },
      { kind: "template" },
      { kind: "custom", index: 1, blockType: "i18n" },
    ]);
  });

  it("omits the script module while the record has no main", () => {
    const catalog = newCatalog();
    catalog.publish(recordOf());
    expect(catalog.modules(carrierA, client).map((entry) => entry.role)).toEqual([
      { kind: "main" },
    ]);
  });

  it("finds a module by its id and nothing by an unknown id", () => {
    const catalog = newCatalog();
    catalog.publish(recordOf({ main: main("x") }));
    const [first] = catalog.modules(carrierA, client);
    expect(catalog.module(first.id)).toBe(first);
    expect(catalog.module("nope" as BundlerModuleId)).toBeUndefined();
  });

  it("keeps one record per graph for the same carrier", () => {
    const catalog = newCatalog();
    catalog.publish(recordOf({ graph: client }));
    catalog.publish(recordOf({ graph: server }));
    expect(catalog.size()).toBe(2);
    const clientIds = catalog.modules(carrierA, client).map((entry) => entry.id);
    const serverIds = catalog.modules(carrierA, server).map((entry) => entry.id);
    expect(clientIds).not.toEqual(serverIds);
  });

  it("publishing replaces the whole record", () => {
    const catalog = newCatalog();
    const first = recordOf({ authoredStyles: [authored("a"), authored("b")] });
    const second = recordOf({ authoredStyles: [authored("a")], main: main("b") });
    catalog.publish(first);
    const staleId = catalog.modules(carrierA, client).find((entry) => {
      return entry.role.kind === "style" && entry.role.index === 1;
    })!.id;
    catalog.publish(second);
    expect(catalog.record(carrierA, client)).toBe(second);
    const styles = catalog.modules(carrierA, client).filter((entry) => entry.role.kind === "style");
    expect(styles).toHaveLength(1);
    expect(catalog.module(staleId)).toBeUndefined();
  });

  describe("retiring", () => {
    it("retiring a carrier removes its records in every graph and their ids", () => {
      const catalog = newCatalog();
      catalog.publish(recordOf({ graph: client }));
      catalog.publish(recordOf({ graph: server }));
      catalog.publish(recordOf({ carrier: carrierB, rawPath: carrierB }));
      const id = catalog.modules(carrierA, client)[0].id;
      expect(catalog.retire(carrierA)).toBe(2);
      expect(catalog.size()).toBe(1);
      expect(catalog.module(id)).toBeUndefined();
      expect(catalog.retire(carrierA)).toBe(0);
    });

    it("retiring a dependency removes every record that read it", () => {
      const catalog = newCatalog();
      const dependency = "/app/types.ts" as HostCanonicalId;
      catalog.publish(recordOf({ closure: new Set([dependency]) }));
      catalog.publish(recordOf({ carrier: carrierB, rawPath: carrierB }));
      expect(catalog.retireDependents(dependency)).toEqual([carrierA]);
      expect(catalog.size()).toBe(1);
      expect(catalog.record(carrierB, client)).toBeDefined();
    });

    it("names a carrier once when several of its graphs read the dependency", () => {
      const catalog = newCatalog();
      const dependency = "/app/types.ts" as HostCanonicalId;
      const closure = new Set([dependency]);
      catalog.publish(recordOf({ graph: client, closure }));
      catalog.publish(recordOf({ graph: server, closure }));
      expect(catalog.retireDependents(dependency)).toEqual([carrierA]);
      expect(catalog.size()).toBe(0);
    });
  });

  it("clearing empties the catalog", () => {
    const catalog = newCatalog();
    catalog.publish(recordOf({ main: main("x") }));
    const id = catalog.modules(carrierA, client)[0].id;
    catalog.clear();
    expect(catalog.size()).toBe(0);
    expect(catalog.module(id)).toBeUndefined();
    expect(catalog.modules(carrierA, client)).toEqual([]);
  });
});
