/**
 * Completion tracking of `TypeDependencyHydrator`, against a mock host and
 * file access. Promise gates, not timers, fix the interleavings.
 *
 * The mock host spells every id in lower case, as the native host spells a
 * drive letter, so a test can reach one file through two spellings.
 */
import { dirname, resolve } from "node:path";
import { describe, expect, it, vi } from "vitest";
import type { HostDependencyResolution, VerterHost } from "@verter/native";
import type { BundlerSourceRevision, HostCanonicalId } from "../host-session/identity";
import { createPipelineCounters } from "./counters";
import { DependencyRouteTable } from "./route-table";
import { TypeDependencyHydrator } from "./type-hydration";

interface MockFile {
  source: string;
  analysis?: object;
}

const canon = (path: string) => path.toLowerCase();
/** An absolute, forward-slash path on any platform. The mock host spells it in lower case. */
const p = (path: string) => resolve(path).replace(/\\/g, "/");
/** The id the mock host mints for a path. */
const id = (path: string) => canon(p(path)) as HostCanonicalId;
/** One recorded event of the mock host or file access. */
const ev = (kind: string, path: string) => `${kind}:${canon(p(path))}`;
const ids = (...paths: string[]) => paths.map((path) => canon(p(path))).sort();

function deferred<T = void>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

function carrier(...specifiers: string[]): MockFile {
  return {
    source: "<template/>",
    analysis: {
      macroTypeDeps: specifiers.map((importSource) => ({
        typeName: "T",
        importSource,
        macroKind: "defineModel",
      })),
    },
  };
}

function declarations(analysis: object = {}): MockFile {
  return { source: "export {};", analysis };
}

function reexports(...sources: string[]): MockFile {
  return declarations({
    exportSignatures: sources.map((reexportSource) => ({
      name: "*",
      isType: false,
      reexportSource,
    })),
  });
}

function makeWorld(initial: Record<string, MockFile>) {
  const files = new Map<string, MockFile>();
  for (const [path, file] of Object.entries(initial)) files.set(canon(path), file);
  const events: string[] = [];
  const tracked = new Set<string>();
  const pushes: { owner: string; routes: HostDependencyResolution[] }[] = [];
  const resolveCalls = new Map<string, number>();
  const resolveListeners = new Map<string, (calls: number) => void>();
  /** What the workspace resolver answers for a specifier, whatever the importer. */
  const targets = new Map<string, string>();

  const host = {
    getAnalysis(path: string): string | null {
      const analysis = files.get(canon(path))?.analysis;
      return analysis ? JSON.stringify(analysis) : null;
    },
    resolve(path: string) {
      resolveCalls.set(canon(path), (resolveCalls.get(canon(path)) ?? 0) + 1);
      resolveListeners.get(canon(path))?.(resolveCalls.get(canon(path)) ?? 0);
      return { canonicalId: canon(path), existsInHost: tracked.has(canon(path)) };
    },
    upsert(entry: { inputId: string; source: string; fileKind?: string }) {
      events.push(`upsert:${canon(entry.inputId)}`);
      tracked.add(canon(entry.inputId));
      return { canonicalId: canon(entry.inputId) };
    },
    remove(path: string): void {
      events.push(`remove:${canon(path)}`);
      tracked.delete(canon(path));
    },
    setImportDependencies(owner: string, routes: HostDependencyResolution[]): void {
      events.push(`routes:${owner}`);
      pushes.push({ owner, routes });
    },
  } as unknown as VerterHost;

  // Like the native workspace, file access is asynchronous. A read can be held.
  const readGates = new Map<string, Promise<void>>();
  const access = {
    readFile: async (path: string) => {
      events.push(`read:${canon(path)}`);
      await readGates.get(canon(path));
      return files.get(canon(path))?.source ?? null;
    },
    // The workspace resolver answers a file that exists: a mapped specifier, the
    // path the bundler named, or a relative specifier read from the importer's directory.
    resolveImport: async (importer: string, specifier: string) => {
      const mapped = targets.get(specifier);
      if (mapped !== undefined) return files.has(canon(mapped)) ? mapped : null;
      if (specifier.startsWith("/")) return files.has(canon(specifier)) ? specifier : null;
      const joined = resolve(dirname(importer), specifier).replace(/\\/g, "/");
      return (
        [joined, `${joined}.ts`, `${joined}.d.ts`].find((path) => files.has(canon(path))) ?? null
      );
    },
    isPackageBacked: (canonicalId: string) => canonicalId.includes("/node_modules/"),
  };

  const counters = createPipelineCounters();
  const routes = new DependencyRouteTable(host, counters);
  const hydrator = new TypeDependencyHydrator(host, access, routes, counters);

  return {
    hydrator,
    routes,
    counters,
    events,
    pushes,
    add: (path: string, file: MockFile) => files.set(canon(path), file),
    holdRead: (path: string, until: Promise<void>) => readGates.set(canon(path), until),
    track: (path: string) => tracked.add(canon(path)),
    targets,
    /** Calls `listener` with the call count whenever the host is asked for `path`'s id. */
    onResolve: (path: string, listener: (calls: number) => void) =>
      resolveListeners.set(canon(path), listener),
    count: (event: string) => events.filter((entry) => entry === event).length,
  };
}

const BARREL = p("/pkg/index.d.ts");
const VIDEO = p("/pkg/video.d.ts");
const SIMULATION = p("/pkg/simulation.d.ts");

/** A package whose declaration entry re-exports two modules through their runtime names. */
function barrelFiles(extra: Record<string, MockFile> = {}): Record<string, MockFile> {
  return {
    [p("/proj/first.vue")]: carrier("pkg"),
    [p("/proj/second.vue")]: carrier("pkg"),
    [BARREL]: reexports("./video.js", "./simulation.js"),
    [VIDEO]: declarations(),
    [SIMULATION]: declarations(),
    [p("/pkg/video.js")]: { source: "export {};" },
    [p("/pkg/simulation.js")]: { source: "export {};" },
    ...extra,
  };
}

describe("TypeDependencyHydrator", () => {
  it("makes a second carrier wait for a barrel another carrier is still walking", async () => {
    const world = makeWorld(barrelFiles());
    const gate = deferred();
    const secondReachedBarrel = deferred();
    // The first call for the barrel's id is the first carrier's walk, the second is the second carrier reaching the memo.
    world.onResolve(BARREL, (calls) => {
      if (calls === 2) secondReachedBarrel.resolve();
    });
    const resolve = async (source: string) => {
      if (source === "pkg") return BARREL;
      if (source === "./video.js") {
        await gate.promise;
        return p("/pkg/video.js");
      }
      if (source === "./simulation.js") return p("/pkg/simulation.js");
      return null;
    };
    world.targets.set(p("/pkg/video.js"), VIDEO);
    world.targets.set(p("/pkg/simulation.js"), SIMULATION);
    const ensured = vi.spyOn(world.hydrator, "ensureFile");

    const first = world.hydrator.hydrateCarrier(id("/proj/first.vue"), resolve).then((closure) => {
      world.events.push("settled:first");
      return closure;
    });
    const second = world.hydrator
      .hydrateCarrier(id("/proj/second.vue"), resolve)
      .then((closure) => {
        world.events.push("settled:second");
        return closure;
      });
    await secondReachedBarrel.promise;

    const barrelWalks = ensured.mock.calls
      .map((call, index) => ({ path: call[0], walk: ensured.mock.results[index]!.value }))
      .filter((call) => canon(call.path) === canon(BARREL));
    expect(barrelWalks).toHaveLength(2);
    expect(barrelWalks[1]!.walk).toBe(barrelWalks[0]!.walk);

    gate.resolve();
    await Promise.all([first, second]);

    for (const path of [BARREL, VIDEO, SIMULATION]) {
      expect(world.count(ev("read", path))).toBe(1);
      expect(world.count(ev("upsert", path))).toBe(1);
    }
    const routesPushed = world.events.indexOf(ev("routes", BARREL));
    expect(routesPushed).toBeGreaterThanOrEqual(0);
    expect(routesPushed).toBeLessThan(world.events.indexOf("settled:first"));
    expect(routesPushed).toBeLessThan(world.events.indexOf("settled:second"));
  });

  it("makes a second carrier wait for a file whose read is still in flight", async () => {
    const world = makeWorld(barrelFiles());
    const gate = deferred();
    const secondReachedBarrel = deferred();
    world.holdRead(BARREL, gate.promise);
    world.onResolve(BARREL, (calls) => {
      if (calls === 2) secondReachedBarrel.resolve();
    });
    world.targets.set(p("/pkg/video.js"), VIDEO);
    world.targets.set(p("/pkg/simulation.js"), SIMULATION);
    const resolve = async (source: string) =>
      source === "pkg"
        ? BARREL
        : source === "./video.js"
          ? p("/pkg/video.js")
          : source === "./simulation.js"
            ? p("/pkg/simulation.js")
            : null;

    const first = world.hydrator.hydrateCarrier(id("/proj/first.vue"), resolve).then((closure) => {
      world.events.push("settled:first");
      return closure;
    });
    const second = world.hydrator
      .hydrateCarrier(id("/proj/second.vue"), resolve)
      .then((closure) => {
        world.events.push("settled:second");
        return closure;
      });
    await secondReachedBarrel.promise;

    expect(world.count(ev("read", BARREL))).toBe(1);
    expect(world.events).not.toContain("settled:first");
    expect(world.events).not.toContain("settled:second");
    expect(world.events).not.toContain(ev("upsert", BARREL));

    gate.resolve();
    const [firstClosure, secondClosure] = await Promise.all([first, second]);

    expect([...firstClosure].sort()).toEqual(ids(BARREL, SIMULATION, VIDEO));
    expect([...secondClosure].sort()).toEqual(ids(BARREL, SIMULATION, VIDEO));
    for (const path of [BARREL, VIDEO, SIMULATION]) {
      expect(world.count(ev("read", path))).toBe(1);
      expect(world.count(ev("upsert", path))).toBe(1);
    }
  });

  it("names every file of the barrel in each carrier's closure", async () => {
    const world = makeWorld(barrelFiles());
    world.targets.set(p("/pkg/video.js"), VIDEO);
    world.targets.set(p("/pkg/simulation.js"), SIMULATION);
    const resolve = async (source: string) =>
      source === "pkg"
        ? BARREL
        : source === "./video.js"
          ? p("/pkg/video.js")
          : source === "./simulation.js"
            ? p("/pkg/simulation.js")
            : null;

    const [first, second] = await Promise.all([
      world.hydrator.hydrateCarrier(id("/proj/first.vue"), resolve),
      world.hydrator.hydrateCarrier(id("/proj/second.vue"), resolve),
    ]);

    for (const closure of [first, second]) {
      expect([...closure].sort()).toEqual(ids(BARREL, SIMULATION, VIDEO));
    }
  });

  it("retries a file whose resolver failed when the next carrier asks", async () => {
    const world = makeWorld({
      [p("/proj/first.vue")]: carrier("pkg"),
      [p("/proj/second.vue")]: carrier("pkg"),
      [BARREL]: reexports("@/video"),
      [VIDEO]: declarations(),
    });
    const failure = new Error("resolver failed");
    let failing = true;
    const resolve = async (source: string) => {
      if (source === "pkg") return BARREL;
      if (source === "@/video") {
        if (failing) throw failure;
        return VIDEO;
      }
      return null;
    };

    await expect(world.hydrator.hydrateCarrier(id("/proj/first.vue"), resolve)).rejects.toBe(
      failure,
    );
    expect(world.hydrator.hydratedPaths().has(id(BARREL))).toBe(false);

    failing = false;
    const closure = await world.hydrator.hydrateCarrier(id("/proj/second.vue"), resolve);

    expect([...closure].sort()).toEqual(ids(BARREL, VIDEO));
    expect(world.count(ev("read", BARREL))).toBe(2);
    expect(world.hydrator.hydratedPaths().has(id(BARREL))).toBe(true);
  });

  it("completes a cyclic import graph, reading each file once", async () => {
    const world = makeWorld({
      [p("/proj/app.vue")]: carrier("./a"),
      [p("/proj/a.d.ts")]: declarations({ imports: [{ source: "./b" }] }),
      [p("/proj/b.d.ts")]: declarations({ imports: [{ source: "./a" }] }),
    });

    const closure = await world.hydrator.hydrateCarrier(id("/proj/app.vue"), null);

    expect([...closure].sort()).toEqual(ids("/proj/a.d.ts", "/proj/b.d.ts"));
    expect(world.count(ev("read", p("/proj/a.d.ts")))).toBe(1);
    expect(world.count(ev("read", p("/proj/b.d.ts")))).toBe(1);
  });

  it("upserts the file the workspace resolver returned for a specifier", async () => {
    const world = makeWorld({
      [p("/lib/app.vue")]: carrier("./index"),
      [p("/lib/index.d.ts")]: reexports("./video.js"),
      [p("/lib/video.js")]: { source: "export const CHOICES = [0, 30, 60];" },
      [p("/lib/video.d.ts")]: declarations(),
    });

    // The bundler has external modules to report: no resolution at all.
    world.targets.set("./video.js", p("/lib/video.d.ts"));
    await world.hydrator.hydrateCarrier(id("/lib/app.vue"), async () => null);

    expect(world.events).toContain(ev("upsert", p("/lib/video.d.ts")));
    expect(world.events).not.toContain(ev("upsert", p("/lib/video.js")));
  });

  it("reads nothing for a carrier with no macro type dependency", async () => {
    const world = makeWorld({
      [p("/proj/app.vue")]: {
        source: "<template/>",
        analysis: { imports: [{ source: "./helpers" }], macroTypeDeps: [] },
      },
      [p("/proj/helpers.ts")]: declarations(),
    });

    const closure = await world.hydrator.hydrateCarrier(id("/proj/app.vue"), null);

    expect(closure.size).toBe(0);
    expect(world.events).toEqual([]);
  });

  it("reads a dependency once across many carriers", async () => {
    const files: Record<string, MockFile> = { [p("/proj/shared.d.ts")]: declarations() };
    const carriers = ["a", "b", "c", "d", "e"].map((name) => `/proj/${name}.vue`);
    for (const path of carriers) files[p(path)] = carrier("./shared");
    const world = makeWorld(files);

    await Promise.all(carriers.map((path) => world.hydrator.hydrateCarrier(id(path), null)));

    expect(world.count(ev("read", p("/proj/shared.d.ts")))).toBe(1);
    expect(world.count(ev("upsert", p("/proj/shared.d.ts")))).toBe(1);
    expect(world.counters.hydrationReads).toBe(1);
  });

  it("reads an invalidated file again and its importer not", async () => {
    const world = makeWorld(barrelFiles());
    world.targets.set(p("/pkg/video.js"), VIDEO);
    world.targets.set(p("/pkg/simulation.js"), SIMULATION);
    const resolve = async (source: string) =>
      source === "pkg"
        ? BARREL
        : source === "./video.js"
          ? p("/pkg/video.js")
          : source === "./simulation.js"
            ? p("/pkg/simulation.js")
            : null;
    await world.hydrator.hydrateCarrier(id("/proj/first.vue"), resolve);

    expect(world.hydrator.invalidate(VIDEO)).toBe(true);
    // The host's registered copy goes, so the next read starts from the new bytes.
    expect(world.count(ev("remove", VIDEO))).toBe(1);
    await world.hydrator.hydrateCarrier(id("/proj/second.vue"), resolve);

    expect(world.count(ev("read", VIDEO))).toBe(2);
    expect(world.count(ev("read", BARREL))).toBe(1);
    expect(world.count(ev("read", SIMULATION))).toBe(1);
  });

  it("reports an untracked path as unknown on invalidation", () => {
    const world = makeWorld({});

    expect(world.hydrator.invalidate(p("/proj/never-seen.d.ts"))).toBe(false);
  });

  it("shares one entry between two spellings of a file", async () => {
    const world = makeWorld({
      [p("/lib/index.d.ts")]: declarations({ imports: [{ source: "./x" }] }),
      [p("/lib/x.d.ts")]: declarations(),
    });

    const first = world.hydrator.ensureFile(p("/Lib/Index.d.ts"), null);
    const second = world.hydrator.ensureFile(p("/lib/index.d.ts"), null);
    await first;

    expect(second).toBe(first);
    expect(world.count(ev("read", p("/lib/index.d.ts")))).toBe(1);
    expect(world.count(ev("upsert", p("/lib/index.d.ts")))).toBe(1);
    expect(world.pushes.map((push) => push.owner)).toEqual([id("/lib/index.d.ts")]);
  });

  it("gives a carrier's routes and a dependency's routes for one file a single owner", async () => {
    const world = makeWorld({
      [p("/proj/main.vue")]: carrier("./Dep.vue"),
      [p("/proj/dep.vue")]: {
        source: "<template/>",
        analysis: { imports: [{ source: "./types" }] },
      },
      [p("/proj/types.d.ts")]: declarations(),
    });
    await world.hydrator.hydrateCarrier(id("/proj/main.vue"), null);

    // The dependency is registered under the host's spelling, whatever spelling named it.
    const moduleRoute = { specifier: "vue", resolvedCanonicalId: p("/proj/vue.js") };
    world.routes.contribute(id("/proj/dep.vue"), [moduleRoute], "module", { always: true });

    const pushedForDep = world.pushes.filter(
      (push) => push.owner.toLowerCase() === id("/proj/dep.vue"),
    );
    expect(pushedForDep.length).toBeGreaterThan(0);
    expect(pushedForDep.every((push) => push.owner === id("/proj/dep.vue"))).toBe(true);
    const lastRoutes = pushedForDep.at(-1)?.routes ?? [];
    expect(
      lastRoutes.map((route) => [route.specifier, route.resolvedCanonicalId?.toLowerCase()]),
    ).toEqual([
      ["vue", id("/proj/vue.js")],
      ["./types", id("/proj/types.d.ts")],
    ]);
  });

  it("asks again for an unreadable file instead of remembering the miss", async () => {
    const world = makeWorld({});

    const missing = await world.hydrator.ensureFile(p("/proj/late.d.ts"), null);
    expect(missing).toEqual({ id: null, registered: false, children: [] });
    expect(world.hydrator.hydratedPaths().size).toBe(0);

    world.add(p("/proj/late.d.ts"), declarations());
    const found = await world.hydrator.ensureFile(p("/proj/late.d.ts"), null);

    expect(found.registered).toBe(true);
    expect(world.count(ev("read", p("/proj/late.d.ts")))).toBe(2);
    expect(world.counters.hydrationReads).toBe(2);
  });

  it("does not read a carrier registered with bundler bytes as a dependency", async () => {
    const world = makeWorld({
      [p("/proj/main.vue")]: carrier("./Dep.vue"),
      [p("/proj/dep.vue")]: { source: "<template/>" },
    });
    world.track(p("/proj/dep.vue"));
    world.hydrator.noteCarrier(id("/proj/dep.vue"), "sha256:one" as BundlerSourceRevision);

    const closure = await world.hydrator.hydrateCarrier(id("/proj/main.vue"), null);

    expect([...closure]).toEqual([id("/proj/dep.vue")]);
    expect(world.events).not.toContain(ev("read", p("/proj/dep.vue")));
    expect(world.events).not.toContain(ev("upsert", p("/proj/dep.vue")));
  });

  it("forgets a carrier's completion when its revision changes", async () => {
    const world = makeWorld({
      [p("/proj/main.vue")]: carrier("./Dep.vue"),
      [p("/proj/dep.vue")]: { source: "<template/>" },
    });
    world.track(p("/proj/dep.vue"));
    world.hydrator.noteCarrier(id("/proj/dep.vue"), "sha256:one" as BundlerSourceRevision);
    await world.hydrator.hydrateCarrier(id("/proj/main.vue"), null);
    expect(world.hydrator.hydratedPaths().has(id("/proj/dep.vue"))).toBe(true);

    world.hydrator.noteCarrier(id("/proj/dep.vue"), "sha256:one" as BundlerSourceRevision);
    expect(world.hydrator.hydratedPaths().has(id("/proj/dep.vue"))).toBe(true);

    world.hydrator.noteCarrier(id("/proj/dep.vue"), "sha256:two" as BundlerSourceRevision);
    expect(world.hydrator.hydratedPaths().has(id("/proj/dep.vue"))).toBe(false);
  });
});
