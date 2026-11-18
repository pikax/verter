import { createRequire } from "node:module";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { VerterHost } from "@verter/native";
import type { CarrierRecord } from "./catalog";
import type { BundlerSourceRevision, HostCanonicalId } from "./identity";
import { BundlerHostSession, type SessionBindingInput } from "./session";

const nativeModule = createRequire(import.meta.url)(
  "@verter/native",
) as typeof import("@verter/native");

const sessions: BundlerHostSession[] = [];

function bind(overrides: Partial<SessionBindingInput> = {}): BundlerHostSession {
  const session = BundlerHostSession.bind({
    adapter: "vite",
    root: "/app",
    mode: "prod",
    defaultTarget: "client",
    frameworkSelection: "auto",
    options: {},
    hostCssPipeline: true,
    hostStripsTypeScript: true,
    hostConfig: null,
    nuxtFileRoles: false,
    ...overrides,
  });
  sessions.push(session);
  return session;
}

function recordOf(overrides: Partial<CarrierRecord> = {}): CarrierRecord {
  return {
    carrier: "/app/A.vue" as HostCanonicalId,
    rawPath: "/app/A.vue",
    graph: "client-prod",
    framework: "vue",
    revision: "sha256:one" as BundlerSourceRevision,
    epoch: 0,
    profile: {
      isProduction: true,
      customElement: false,
      ssr: false,
      forceJs: false,
      hmrStrategy: "none",
      sourceMap: false,
    },
    main: { code: "export default {}", map: null, lang: null },
    authoredStyles: [],
    compiledStyles: [],
    nodes: [],
    closure: new Set(),
    ...overrides,
  };
}

afterEach(() => {
  for (const session of sessions.splice(0)) session.close();
  vi.restoreAllMocks();
});

describe("BundlerHostSession binding", () => {
  it("captures the working directory once when the host gives no root", () => {
    const cwd = vi.spyOn(process, "cwd").mockReturnValue("/first");
    const session = bind({ root: null });
    cwd.mockReturnValue("/second");
    expect(session.binding.rootSource).toBe("process-cwd-at-bind");
    expect(session.binding.root).toBe("/first");
  });

  it("uses the host's root as given", () => {
    const session = bind({ root: "/given" });
    expect(session.binding.rootSource).toBe("host");
    expect(session.binding.root).toBe("/given");
  });

  it("refuses an adapter it does not know", () => {
    expect(() => bind({ adapter: "bun" })).toThrow();
  });

  it.each([
    ["vite", "vite"],
    ["rolldown", "vite"],
    ["webpack", "webpack"],
    ["rspack", "webpack"],
    ["rollup", "none"],
    ["esbuild", "none"],
    ["farm", "none"],
  ] as const)("the %s adapter reloads through the %s strategy", (adapter, strategy) => {
    expect(bind({ adapter }).binding.hmrStrategy).toBe(strategy);
  });

  it("mints local identities and the default intent when none are supplied", () => {
    const { binding } = bind();
    expect(binding.project).toBeTruthy();
    expect(binding.sourceModuleEnvironment).toBeTruthy();
    expect(binding.compileIntent).toBeTruthy();
  });

  it("the options digest ignores key order and describes functions and patterns", () => {
    const a = bind({ options: { include: [/\.vue$/i, "x.vue"], lang: "vue" } });
    const b = bind({ options: { lang: "vue", include: [/\.vue$/i, "x.vue"] } });
    const other = bind({ options: { lang: "vue", include: [/\.vue$/, "x.vue"] } });
    expect(a.binding.optionsDigest).toBe(b.binding.optionsDigest);
    expect(a.binding.optionsDigest).not.toBe(other.binding.optionsDigest);
    const withFunction = bind({ options: { onWarn: () => 1 } as never });
    const withOther = bind({ options: { onWarn: () => 2 } as never });
    expect(withFunction.binding.optionsDigest).toBe(withOther.binding.optionsDigest);
  });

  it("refuses to digest an option it cannot serialise", () => {
    expect(() => bind({ options: { big: 1n } as never })).toThrow();
  });

  it("two sessions bound from equal input have different ids and separate hosts", () => {
    const a = bind();
    const b = bind();
    expect(a.id).not.toBe(b.id);
    expect(a.host()).not.toBe(b.host());
  });
});

describe("BundlerHostSession host", () => {
  it("closing one session leaves another session's host usable", () => {
    const a = bind();
    const b = bind();
    a.host();
    const survivor = b.host();
    a.close();
    expect(() =>
      survivor.upsert({ inputId: "/app/A.vue", source: "<template><div/></template>" } as never),
    ).not.toThrow();
  });

  it("the host is created on first use and closed exactly once", () => {
    const close = vi.spyOn(nativeModule.VerterHost.prototype, "close");
    const session = bind();
    expect(session.peekHost()).toBeNull();
    const host = session.host();
    expect(session.host()).toBe(host);
    expect(session.peekHost()).toBe(host);
    expect(close).toHaveBeenCalledTimes(0);
    session.close();
    expect(close).toHaveBeenCalledTimes(1);
    session.close();
    expect(close).toHaveBeenCalledTimes(1);
    expect(session.state).toBe("closed");
  });

  it("a session that never made a host closes without a native call", () => {
    const close = vi.spyOn(nativeModule.VerterHost.prototype, "close");
    bind().close();
    expect(close).not.toHaveBeenCalled();
  });

  it("a closed session refuses to hand out a host", () => {
    const session = bind();
    session.close();
    expect(() => session.host()).toThrow("[verter] bundler host session is closed");
  });

  it("closing empties the catalog", () => {
    const session = bind();
    session.catalog.publish(recordOf());
    session.close();
    expect(session.catalog.size()).toBe(0);
  });

  it("restart moves to a new epoch and empties the catalog", () => {
    const session = bind();
    const before = session.host();
    session.catalog.publish(recordOf());
    expect(session.epoch).toBe(0);
    session.restart();
    expect(session.epoch).toBe(1);
    expect(session.catalog.size()).toBe(0);
    expect(session.state).toBe("open");
    expect(session.host()).not.toBe(before);
  });

  it("restart closes the host it forgets", () => {
    const close = vi.spyOn(nativeModule.VerterHost.prototype, "close");
    const session = bind();
    session.host();
    session.restart();
    expect(close).toHaveBeenCalledTimes(1);
    expect(session.peekHost()).toBeNull();
  });

  it("numbers hook invocations in order", () => {
    const session = bind();
    const first = session.nextInvocation();
    expect(session.nextInvocation()).toBeGreaterThan(first);
  });
});

describe("BundlerHostSession identify", () => {
  it("a marked request for a file the host does not read as a carrier is foreign", () => {
    const session = bind();
    expect(session.identify("/a/readme.md?vue&type=style&index=0&lang.css", "client-prod")).toEqual(
      { kind: "foreign" },
    );
  });

  it("a marker that disagrees with the carrier's framework is rejected", () => {
    const session = bind();
    expect(
      session.identify("/a/A.svelte?vue&type=style&index=0&lang.css", "client-prod"),
    ).toMatchObject({ kind: "rejected", reason: "marker-framework-mismatch" });
  });

  it("the verter marker on a Vue carrier is rejected", () => {
    const session = bind();
    expect(
      session.identify("/a/A.vue?verter&type=style&index=0&lang.css", "client-prod"),
    ).toMatchObject({ kind: "rejected", reason: "marker-framework-mismatch" });
  });

  it("admits a marked style request of an included carrier", () => {
    const session = bind();
    const result = session.identify("/a/A.vue?vue&type=style&index=1&lang.css", "client-prod");
    expect(result).toMatchObject({
      kind: "carrier-module",
      framework: "vue",
      graph: "client-prod",
    });
    if (result.kind === "carrier-module") {
      expect(result.request.nodeKind).toEqual({ kind: "style", index: 1 });
    }
  });

  it("a custom block's type is part of its module id", () => {
    const session = bind();
    const one = session.identify("/a/A.vue?vue&type=i18n&index=0", "client-prod");
    const other = session.identify("/a/A.vue?vue&type=docs&index=0", "client-prod");
    if (one.kind !== "carrier-module" || other.kind !== "carrier-module") {
      throw new Error("expected carrier modules");
    }
    expect(one.id).not.toBe(other.id);
  });

  it("a fixed framework selection decides the framework of any included carrier", () => {
    const session = bind({ frameworkSelection: "sveltejs", options: { include: /\.vue$/ } });
    expect(session.frameworkFor("/a/x.vue")).toBe("sveltejs");
    expect(session.includesCarrier("/a/x.vue")).toBe(true);
  });

  it("a fixed selection and no custom include admit only that framework's carriers", () => {
    const session = bind({ frameworkSelection: "sveltejs" });
    expect(session.includesCarrier("/a/x.svelte")).toBe(true);
    expect(session.includesCarrier("/a/x.vue")).toBe(false);
  });

  it("auto detection reads the carrier's framework from its extension", () => {
    const session = bind({ options: { include: [/\.vue$/, /\.svelte$/] } });
    expect(session.frameworkFor("/a/x.vue")).toBe("vue");
    expect(session.frameworkFor("/a/x.svelte")).toBe("sveltejs");
  });

  it("a custom include alone never makes a suffix the registry does not know a carrier", () => {
    const session = bind({ options: { include: [/\.md$/] } });
    expect(session.includesCarrier("/a/x.md")).toBe(false);
    expect(session.identify("/a/x.md?vue&type=style&index=0&lang.css", "client-prod")).toEqual({
      kind: "foreign",
    });
  });

  it("a registered suffix compiles as the framework it was registered for", () => {
    const session = bind({ options: { carriers: { ".md": "vue" } } });
    expect(session.includesCarrier("/a/x.md")).toBe(true);
    expect(session.frameworkFor("/a/x.md")).toBe("vue");
    expect(
      session.identify("/a/x.md?vue&type=style&index=0&lang.css", "client-prod"),
    ).toMatchObject({ kind: "carrier-module", framework: "vue" });
  });

  it("a registered suffix still needs the include filter to admit it", () => {
    const session = bind({ options: { carriers: { md: "vue" }, include: /\.vue$/ } });
    expect(session.includesCarrier("/a/x.md")).toBe(false);
  });

  it("the carriers a session registers change its identity", () => {
    const plain = bind();
    const withMarkdown = bind({ options: { carriers: { ".md": "vue" } } });
    expect(withMarkdown.binding.optionsDigest).not.toBe(plain.binding.optionsDigest);
  });

  it("a global include pattern gives the same answer on every call", () => {
    const session = bind({ options: { include: /\.vue$/g } });
    expect(session.includesCarrier("/a/A.vue")).toBe(true);
    expect(session.includesCarrier("/a/A.vue")).toBe(true);
  });

  it("passes a generated module and a malformed request through", () => {
    const session = bind();
    expect(session.identify("\0plugin-vue:export-helper", "client-prod")).toEqual({
      kind: "generated",
      name: "export-helper",
    });
    expect(
      session.identify("/a/A.vue?vue&type=style&index=x&lang.css", "client-prod"),
    ).toMatchObject({
      kind: "rejected",
      reason: "invalid-index",
    });
  });

  it("the same request in two graphs has two module ids", () => {
    const session = bind();
    const raw = "/a/A.vue?vue&type=style&index=0&lang.css";
    const client = session.identify(raw, "client-prod");
    const server = session.identify(raw, "ssr-prod");
    if (client.kind !== "carrier-module" || server.kind !== "carrier-module") {
      throw new Error("expected carrier modules");
    }
    expect(client.id).not.toBe(server.id);
    expect(client.request.nodeKind).toEqual(server.request.nodeKind);
  });

  it("the catalog serves the id that identify mints for a published module", () => {
    const session = bind();
    session.catalog.publish(
      recordOf({
        authoredStyles: [
          { blockToken: "a", content: ".a{}", lang: "css", scoped: false, module: false },
        ],
      }),
    );
    const result = session.identify("/app/A.vue?vue&type=style&index=0&lang.css", "client-prod");
    if (result.kind !== "carrier-module") throw new Error("expected a carrier module");
    expect(session.catalog.module(result.id)?.role).toEqual({ kind: "style", index: 0 });
  });

  it("identifying does not read a file", () => {
    const readFile = vi.spyOn(nativeModule.Workspace.prototype, "readFile");
    const fileExists = vi.spyOn(nativeModule.Workspace.prototype, "fileExists");
    const session = bind();
    for (let index = 0; index < 20; index += 1) {
      session.identify(`/a/A${index}.vue?vue&type=style&index=0&lang.css`, "client-prod");
    }
    expect(readFile).not.toHaveBeenCalled();
    expect(fileExists).not.toHaveBeenCalled();
  });
});

describe("BundlerHostSession graphFor", () => {
  it("the hook's environment decides the graph before the bound default", () => {
    const session = bind({ defaultTarget: "client", mode: "prod" });
    expect(session.graphFor({ environment: { name: "ssr", config: { consumer: "server" } } })).toBe(
      "ssr-prod",
    );
    expect(
      session.graphFor({ environment: { name: "client", config: { consumer: "client" } } }),
    ).toBe("client-prod");
    expect(session.graphFor({ ssrFlag: true })).toBe("ssr-prod");
    expect(session.graphFor({ ssrFlag: false })).toBe("client-prod");
    expect(session.graphFor({})).toBe("client-prod");
  });

  it("an environment without a consumer falls back to the per-call flag, then the default", () => {
    const session = bind({ defaultTarget: "ssr", mode: "dev" });
    expect(session.graphFor({ environment: { name: "x" }, ssrFlag: false })).toBe("client-dev");
    expect(session.graphFor({ environment: { name: "x" }, ssrFlag: null })).toBe("ssr-dev");
  });
});

describe("BundlerHostSession invalidate", () => {
  it("retires the carrier's records in every graph", () => {
    const session = bind();
    session.catalog.publish(recordOf({ graph: "client-prod" }));
    session.catalog.publish(recordOf({ graph: "ssr-prod" }));
    const result = session.invalidate("/app/A.vue");
    expect(result.retired).toEqual(["/app/A.vue"]);
    expect(result.removedFromHost).toBe(false);
    expect(session.catalog.size()).toBe(0);
    expect(session.peekHost()).toBeNull();
  });

  it("invalidating a carrier removes it from the native host", () => {
    const session = bind();
    const host = session.host();
    host.upsert({ inputId: "/app/A.vue", source: "<template><div/></template>" } as never);
    const remove = vi.spyOn(host as VerterHost, "remove");
    session.invalidate("/app/A.vue");
    expect(remove).toHaveBeenCalledTimes(1);
    expect(remove).toHaveBeenCalledWith("/app/A.vue");
  });

  it("does not call the host for a path outside the include filter", () => {
    const session = bind();
    const host = session.host();
    const remove = vi.spyOn(host, "remove");
    expect(session.invalidate("/app/readme.md").removedFromHost).toBe(false);
    expect(remove).not.toHaveBeenCalled();
  });

  it("retires records that read the changed file, by the host's spelling of it", () => {
    const session = bind();
    const host = session.host();
    host.upsert({
      inputId: "/app/types.ts",
      source: "export type T = 1",
      fileKind: "text",
    } as never);
    const hostSpelling = host.resolve("/app/types.ts")!.canonicalId as HostCanonicalId;
    session.catalog.publish(recordOf({ closure: new Set([hostSpelling]) }));
    const result = session.invalidate("/app/types.ts");
    expect(result.retired).toEqual(["/app/A.vue"]);
    expect(session.catalog.size()).toBe(0);
  });

  it("never creates a host", () => {
    const session = bind();
    session.invalidate("/app/A.vue");
    expect(session.peekHost()).toBeNull();
  });
});
