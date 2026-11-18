/**
 * Planning is a pure function of the identification and the binding; the
 * executors serve only what the carrier's record holds; the main wrapper has
 * one fixed shape.
 */
import { afterEach, describe, expect, it } from "vitest";
import { EXPORT_HELPER_ID } from "../constants";
import type { CarrierRecord } from "../host-session/catalog";
import { canonicalizeId } from "@verter/native";
import type { HostVirtualNodeKind } from "@verter/native";
import type {
  BundlerSourceRevision,
  HookInvocationId,
  HostCanonicalId,
} from "../host-session/identity";
import { BundlerHostSession } from "../host-session/session";
import { PipelineRefusalError } from "./admission";
import type { CarrierPipeline, DemandContext } from "./carrier-demand";
import { emitMainModule, executeLoad, planLoad, planTransform, transformIncludes } from "./plan";

const sessions: BundlerHostSession[] = [];

afterEach(() => {
  for (const session of sessions.splice(0)) session.close();
});

/** A session for planning; identifying a request asks its native host. */
function sessionWith(hostCssPipeline: boolean): BundlerHostSession {
  const session = BundlerHostSession.bind({
    adapter: hostCssPipeline ? "vite" : "rollup",
    root: "/p",
    mode: "dev",
    defaultTarget: "client",
    frameworkSelection: "auto",
    options: {},
    hostCssPipeline,
    hostStripsTypeScript: hostCssPipeline,
    hostConfig: null,
    nuxtFileRoles: false,
  });
  sessions.push(session);
  return session;
}

function recordOf(overrides: Partial<CarrierRecord> = {}): CarrierRecord {
  return {
    carrier: canonicalizeId("/p/App.vue") as HostCanonicalId,
    rawPath: "/p/App.vue",
    graph: "client-dev",
    framework: "vue",
    revision: "sha256:test" as BundlerSourceRevision,
    epoch: 0,
    profile: {
      filename: "/p/App.vue",
      componentId: "0123abcd",
      isProduction: false,
      customElement: false,
      ssr: false,
      forceJs: false,
      hmrStrategy: "vite",
      sourceMap: true,
    },
    main: { code: "export default {}", map: null, lang: "ts" },
    authoredStyles: [],
    compiledStyles: [],
    nodes: [],
    closure: new Set(),
    ...overrides,
  };
}

/** The ids a carrier's sub-modules get from the session's native host. */
function moduleIdsOf(session: BundlerHostSession, record: CarrierRecord) {
  const host = session.host();
  return (node: HostVirtualNodeKind, lang?: string) =>
    host.renderBundlerId(record.carrier, node, lang);
}

/** A pipeline whose every demand yields `record`. */
function pipelineServing(record: CarrierRecord): CarrierPipeline {
  return { demand: async () => record } as unknown as CarrierPipeline;
}

const ctx: DemandContext = {
  invocation: 1 as HookInvocationId,
  resolve: null,
  warn: null,
  addWatchFile: null,
};

describe("load and transform plans", () => {
  it("planning is pure and total over identifications", () => {
    const cases: Array<{
      id: string;
      load: [string, string];
      transform: [string, string];
      included: [boolean, boolean];
    }> = [
      // [without the host CSS pipeline, with it]
      {
        id: "/p/App.vue",
        load: ["skip", "skip"],
        transform: ["carrier-main", "carrier-main"],
        included: [true, true],
      },
      {
        id: "/p/App.vue?vue&type=script&lang.ts",
        load: ["carrier-script", "carrier-script"],
        transform: ["skip", "skip"],
        included: [false, false],
      },
      {
        id: "/p/App.vue?vue&type=style&index=1&lang.css",
        load: ["style:compiled", "style:bundler-css"],
        transform: ["style-compiled", "style-scope"],
        included: [false, true],
      },
      {
        id: "/p/App.vue?vue&type=style&index=0&lang.scss",
        load: ["style:compiled", "style:bundler-css"],
        transform: ["style-compiled", "style-scope"],
        included: [true, true],
      },
      {
        id: "/p/App.vue?vue&type=template",
        load: ["compiled-node:template:0", "compiled-node:template:0"],
        transform: ["skip", "skip"],
        included: [false, false],
      },
      {
        id: "/p/App.vue?vue&type=i18n&index=2",
        load: ["compiled-node:custom:2", "compiled-node:custom:2"],
        transform: ["skip", "skip"],
        included: [false, false],
      },
      {
        id: "/p/App.svelte",
        load: ["skip", "skip"],
        transform: ["carrier-main", "carrier-main"],
        included: [true, true],
      },
      {
        id: "/p/App.svelte?verter&type=style&index=0&lang.css",
        load: ["style:compiled", "style:compiled"],
        transform: ["skip", "skip"],
        included: [false, false],
      },
      {
        id: EXPORT_HELPER_ID,
        load: ["generated", "generated"],
        transform: ["skip", "skip"],
        included: [false, false],
      },
      {
        id: "/p/readme.md",
        load: ["skip", "skip"],
        transform: ["skip", "skip"],
        included: [false, false],
      },
    ];

    const describeLoad = (plan: ReturnType<typeof planLoad>): string => {
      switch (plan.kind) {
        case "style":
          return `style:${plan.lane}`;
        case "compiled-node":
          return `compiled-node:${plan.nodeKind}:${plan.index}`;
        default:
          return plan.kind;
      }
    };

    [false, true].forEach((hostCssPipeline, column) => {
      const session = sessionWith(hostCssPipeline);
      const graph = session.graphFor({});
      for (const testCase of cases) {
        const identification = session.identify(testCase.id, graph);
        const label = `${testCase.id} (host CSS pipeline: ${hostCssPipeline})`;
        const load = planLoad(identification, session.binding);
        expect(describeLoad(load), label).toBe(testCase.load[column]);
        // Pure: the same inputs plan the same value.
        expect(planLoad(identification, session.binding), label).toEqual(load);
        expect(planTransform(identification, session.binding).kind, label).toBe(
          testCase.transform[column],
        );
        expect(transformIncludes(identification, session.binding), label).toBe(
          testCase.included[column],
        );
      }

      const rejected = session.identify("/p/App.vue?vue", graph);
      expect(rejected.kind).toBe("rejected");
      expect(() => planLoad(rejected, session.binding)).toThrow(/marker-without-type/);
      expect(() => planTransform(rejected, session.binding)).toThrow(/marker-without-type/);
      expect(transformIncludes(rejected, session.binding)).toBe(false);
    });
  });

  it("a style block the carrier does not have is refused, not skipped", async () => {
    const session = sessionWith(false);
    const request = "/p/App.vue?vue&type=style&index=5&lang.css";
    const plan = planLoad(session.identify(request, session.graphFor({})), session.binding);
    const record = recordOf({ compiledStyles: [{ code: ".a{}", map: null, lang: "css" }] });

    const error = await executeLoad(plan, pipelineServing(record), ctx).catch(
      (reason: unknown) => reason,
    );

    expect(error).toBeInstanceOf(PipelineRefusalError);
    expect((error as PipelineRefusalError).refusal).toEqual({
      kind: "module-not-published",
      request,
    });
    expect((error as Error).message).toContain(request);
  });

  it("the main module lists its parts in a fixed order", () => {
    const session = sessionWith(true);
    const record = recordOf({
      authoredStyles: [
        { blockToken: "s0", content: ".a{}", lang: "css", scoped: true, module: false },
        { blockToken: "s1", content: ".b{}", lang: "css", scoped: false, module: true },
      ],
    });

    const output = emitMainModule(record, session.binding, moduleIdsOf(session, record));

    const script = JSON.stringify("/p/App.vue?vue&type=script&lang.ts");
    expect(output.code.split("\n")).toEqual([
      `import _sfc_main from ${script}`,
      `import ${JSON.stringify("/p/App.vue?vue&type=style&index=0&lang.css")}`,
      `import style1 from ${JSON.stringify("/p/App.vue?vue&type=style&index=1&lang.module.css")}`,
      `import _export_sfc from ${JSON.stringify(EXPORT_HELPER_ID)}`,
      "const cssModules = {",
      `  "$style": style1`,
      "}",
      `export * from ${script}`,
      `export default /*@__PURE__*/_export_sfc(_sfc_main, [["__cssModules", cssModules]])`,
    ]);
    expect(output.map).toBeNull();
    expect(output.lang).toBe("ts");
    // Every id the module imports is the host's own spelling of that request.
    const host = session.host();
    for (const line of output.code.split("\n")) {
      const emitted = /"(\/p\/App\.vue\?[^"]+)"/.exec(line)?.[1];
      if (emitted) expect(host.resolve(emitted)).toMatchObject({ bundlerId: emitted });
    }
  });

  it("a request's host flags never reach the published ids", () => {
    const session = sessionWith(true);
    const identification = session.identify(
      "/p/App.vue?vue&type=style&index=0&inline&used&lang.css",
      session.graphFor({}),
    );
    expect(identification.kind).toBe("carrier-module");
    const plan = planLoad(identification, session.binding);
    expect(plan.kind).toBe("style");
    const record = recordOf({
      rawPath: plan.kind === "style" ? plan.key.rawPath : "",
      authoredStyles: [
        { blockToken: "s0", content: ".a{}", lang: "css", scoped: true, module: false },
      ],
    });

    const output = emitMainModule(record, session.binding, moduleIdsOf(session, record));

    expect(output.code).not.toContain("inline");
    expect(output.code).not.toContain("used");
  });
});
