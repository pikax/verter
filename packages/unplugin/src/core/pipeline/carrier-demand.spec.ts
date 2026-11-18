/**
 * The carrier pipeline against a real native host: one compile per demand
 * whatever the hook order, nothing published from a failed, superseded or
 * stale flight, and native failures that keep their kind.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mkdirSync, mkdtempSync, promises as fsPromises, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import type { HostMissingInput } from "@verter/native";
import type { ResolveHook } from "../bundler-resolve";
import type { CarrierRecord } from "../host-session/catalog";
import { canonicalizeId } from "@verter/native";
import type { HostCanonicalId } from "../host-session/identity";
import { BundlerHostSession } from "../host-session/session";
import { PipelineRefusalError } from "./admission";
import { CarrierPipeline, type CarrierDemandKey, type DemandContext } from "./carrier-demand";
import type { PipelineCounters } from "./counters";
import { executeLoad, executeTransform, planLoad, planTransform } from "./plan";

const TWO_STYLES = `<script setup>
const n = 1
</script>
<template><p class="a b">{{ n }}</p></template>
<style scoped>.a { color: red; }</style>
<style>.b { color: blue; }</style>`;

const WITH_IMPORT = `<script setup lang="ts">
import { label } from "./dep"
</script>
<template><p>{{ label }}</p></template>`;

const WITHOUT_IMPORT = `<script setup lang="ts">
const label = "two"
</script>
<template><p>{{ label }}</p></template>`;

/** The host refuses to compile an external non-native style it was given no processed bytes for. */
const BROKEN = `<template><div>ok</div></template>
<style src="./external.css" lang="postcss"></style>`;

let root: string;
const sessions: BundlerHostSession[] = [];

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "verter-carrier-demand-")).replace(/\\/g, "/");
  writeFileSync(join(root, "dep.ts"), `export const label = "one"\n`);
  writeFileSync(join(root, "external.css"), `.from-disk { color: red; }\n`);
});

afterEach(() => {
  vi.restoreAllMocks();
  for (const session of sessions.splice(0)) session.close();
  rmSync(root, { recursive: true, force: true });
});

function open(): { session: BundlerHostSession; pipeline: CarrierPipeline } {
  const session = BundlerHostSession.bind({
    adapter: "rollup",
    root,
    mode: "dev",
    defaultTarget: "client",
    frameworkSelection: "vue",
    options: {},
    hostCssPipeline: false,
    hostStripsTypeScript: false,
    hostConfig: null,
    nuxtFileRoles: false,
  });
  sessions.push(session);
  return { session, pipeline: new CarrierPipeline(session) };
}

function carrierFile(name: string, source: string | null): string {
  const path = `${root}/${name}`;
  if (source !== null) writeFileSync(path, source);
  return path;
}

function keyOf(session: BundlerHostSession, path: string): CarrierDemandKey {
  return {
    carrier: canonicalizeId(path) as HostCanonicalId,
    rawPath: path,
    graph: session.graphFor({}),
  };
}

function contextOf(session: BundlerHostSession, resolve: ResolveHook | null = null): DemandContext {
  return { invocation: session.nextInvocation(), resolve, warn: () => {}, addWatchFile: null };
}

function deferred(): { promise: Promise<void>; release: () => void } {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}

/** A resolver that answers `./dep` only once `gate` is released. */
function gatedResolver(gate: Promise<void>): ResolveHook {
  return async (source) => {
    await gate;
    return source === "./dep" ? { id: `${root}/dep.ts` } : null;
  };
}

/** Every counter except the hook count, which a new hook call always raises. */
function workOf(counters: Readonly<PipelineCounters>): Omit<PipelineCounters, "hookInvocations"> {
  const { hookInvocations: _hooks, ...work } = counters;
  return work;
}

function permutations<T>(items: readonly T[]): T[][] {
  if (items.length <= 1) return [items.slice()];
  return items.flatMap((item, index) =>
    permutations([...items.slice(0, index), ...items.slice(index + 1)]).map((rest) => [
      item,
      ...rest,
    ]),
  );
}

type Operation = "transform" | "script" | "style0" | "style1";

/** Start one hook's work on `file` through plan and execute, without awaiting it. */
function startOperation(
  session: BundlerHostSession,
  pipeline: CarrierPipeline,
  file: string,
  operation: Operation,
): Promise<string | undefined> {
  const graph = session.graphFor({});
  const binding = session.binding;
  const ctx = contextOf(session);
  if (operation === "transform") {
    const plan = planTransform(session.identify(file, graph), binding);
    return executeTransform(plan, TWO_STYLES, pipeline, binding, ctx).then((out) => out?.code);
  }
  const id =
    operation === "script"
      ? `${file}?vue&type=script&lang.js`
      : `${file}?vue&type=style&index=${operation === "style0" ? 0 : 1}&lang.css`;
  return executeLoad(planLoad(session.identify(id, graph), binding), pipeline, ctx).then(
    (out) => out?.code,
  );
}

describe("carrier demand", () => {
  it("a main transform, a script load and two style loads cause one compile, in every order", async () => {
    const operations: Operation[] = ["transform", "script", "style0", "style1"];

    const sequential = open();
    const file = carrierFile("Ordered.vue", TWO_STYLES);
    const expected: Record<string, string | undefined> = {};
    for (const operation of operations) {
      expected[operation] = await startOperation(
        sequential.session,
        sequential.pipeline,
        file,
        operation,
      );
    }
    expect(expected.style0).toMatch(/\[data-v-[0-9a-f]+\]/);

    const orders = permutations(operations);
    expect(orders).toHaveLength(24);
    for (const order of orders) {
      const { session, pipeline } = open();
      const started = order.map((operation) =>
        startOperation(session, pipeline, file, operation).then(
          (code) => [operation, code] as const,
        ),
      );
      const results = Object.fromEntries(await Promise.all(started));
      expect(pipeline.counters().compileRequests, order.join(",")).toBe(1);
      expect(pipeline.counters().registrations, order.join(",")).toBe(1);
      expect(results, order.join(",")).toEqual(expected);
      session.close();
    }
  });

  it("a second demand with the same bytes does no work", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Warm.vue", TWO_STYLES);
    const key = keyOf(session, file);

    const first = await pipeline.demand(
      key,
      { kind: "bundler", code: TWO_STYLES },
      contextOf(session),
    );
    const before = workOf(pipeline.counters());
    const host = session.host();
    const upsert = vi.spyOn(host, "upsert");
    const compile = vi.spyOn(host, "compileRequest");
    const readFile = vi.spyOn(session.workspace(), "readFile");
    const second = await pipeline.demand(
      key,
      { kind: "bundler", code: TWO_STYLES },
      contextOf(session),
    );

    expect(second).toBe(first);
    // The host answers the one registration with `changed: false`.
    expect(upsert).toHaveBeenCalledTimes(1);
    expect(upsert.mock.results[0]?.value).toMatchObject({ changed: false });
    expect(compile).not.toHaveBeenCalled();
    expect(readFile).not.toHaveBeenCalled();
    expect(workOf(pipeline.counters())).toEqual({
      ...before,
      registrations: before.registrations + 1,
    });
  });

  it("new bytes supersede a running flight", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Superseded.vue", null);
    const key = keyOf(session, file);
    const gate = deferred();
    const publish = vi.spyOn(session.catalog, "publish");

    const older = pipeline.demand(
      key,
      { kind: "bundler", code: WITH_IMPORT },
      contextOf(session, gatedResolver(gate.promise)),
    );
    const newer = pipeline.demand(
      key,
      { kind: "bundler", code: WITHOUT_IMPORT },
      contextOf(session),
    );
    const newerRecord = await newer;
    gate.release();

    expect(await older).toBe(newerRecord);
    expect(pipeline.counters().supersededFlights).toBe(1);
    expect(publish).toHaveBeenCalledTimes(1);
    expect(publish.mock.calls[0][0].revision).toBe(newerRecord.revision);
    expect(session.catalog.record(key.carrier, key.graph)).toBe(newerRecord);
  });

  it("a disk demand joins a running bundler flight instead of reading the file", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Joined.vue", WITH_IMPORT);
    const key = keyOf(session, file);
    const gate = deferred();
    const readFile = vi.spyOn(fsPromises, "readFile");

    const fromBundler = pipeline.demand(
      key,
      { kind: "bundler", code: WITH_IMPORT },
      contextOf(session, gatedResolver(gate.promise)),
    );
    const fromDisk = pipeline.demand(key, { kind: "disk" }, contextOf(session));
    expect(pipeline.inFlight(key)).toBe(true);
    gate.release();

    expect(await fromDisk).toBe(await fromBundler);
    expect(readFile.mock.calls.some((call: unknown[]) => String(call[0]) === file)).toBe(false);
  });

  it("a disk demand after a bundler record returns that record even if the file differs", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("BundlerWins.vue", WITHOUT_IMPORT);
    const key = keyOf(session, file);

    const fromBundler = await pipeline.demand(
      key,
      { kind: "bundler", code: TWO_STYLES },
      contextOf(session),
    );
    const fromDisk = await pipeline.demand(key, { kind: "disk" }, contextOf(session));

    expect(fromDisk).toBe(fromBundler);
    expect(fromDisk.compiledStyles).toHaveLength(2);
  });

  it("nothing is published when the compile fails", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Broken.vue", null);
    const key = keyOf(session, file);

    await expect(
      pipeline.demand(key, { kind: "bundler", code: BROKEN }, contextOf(session)),
    ).rejects.toThrow(/ProcessedContentRequired/);
    expect(session.catalog.record(key.carrier, key.graph)).toBeUndefined();

    const fixed = await pipeline.demand(
      key,
      { kind: "bundler", code: TWO_STYLES },
      contextOf(session),
    );
    expect(session.catalog.record(key.carrier, key.graph)).toBe(fixed);
  });

  it("every joiner of a failed flight sees the same error", async () => {
    const { session, pipeline } = open();
    const key = keyOf(session, carrierFile("BrokenJoined.vue", null));

    const [first, second] = await Promise.allSettled([
      pipeline.demand(key, { kind: "bundler", code: BROKEN }, contextOf(session)),
      pipeline.demand(key, { kind: "bundler", code: BROKEN }, contextOf(session)),
    ]);

    expect(first.status).toBe("rejected");
    expect(second.status).toBe("rejected");
    expect((first as PromiseRejectedResult).reason).toBe((second as PromiseRejectedResult).reason);
    expect(pipeline.counters().compileRequests).toBe(1);
  });

  it("a flight that finishes after a restart publishes nothing", async () => {
    const { session, pipeline } = open();
    const key = keyOf(session, carrierFile("Restarted.vue", null));
    const gate = deferred();

    const flight = pipeline.demand(
      key,
      { kind: "bundler", code: WITH_IMPORT },
      contextOf(session, gatedResolver(gate.promise)),
    );
    session.restart();
    gate.release();

    const error = await flight.catch((reason: unknown) => reason);
    expect(error).toBeInstanceOf(PipelineRefusalError);
    expect((error as PipelineRefusalError).refusal.kind).toBe("stale");
    expect(session.catalog.size()).toBe(0);
    expect(pipeline.counters().staleDiscards).toBe(1);
  });

  it("edit then revert equals a fresh session", async () => {
    const edited = open();
    const file = carrierFile("Reverted.vue", null);
    const editedKey = keyOf(edited.session, file);
    for (const code of [TWO_STYLES, WITHOUT_IMPORT, TWO_STYLES]) {
      await edited.pipeline.demand(editedKey, { kind: "bundler", code }, contextOf(edited.session));
    }

    const fresh = open();
    const freshRecord = await fresh.pipeline.demand(
      keyOf(fresh.session, file),
      { kind: "bundler", code: TWO_STYLES },
      contextOf(fresh.session),
    );

    const revertedRecord = edited.session.catalog.record(editedKey.carrier, editedKey.graph)!;
    const withoutEpoch = (record: CarrierRecord) => ({ ...record, epoch: 0 });
    expect(withoutEpoch(revertedRecord)).toEqual(withoutEpoch(freshRecord));
  });

  it("a missing carrier is refused by name", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Absent.vue", null);

    await expect(
      pipeline.demand(keyOf(session, file), { kind: "disk" }, contextOf(session)),
    ).rejects.toMatchObject({ refusal: { kind: "carrier-unavailable", carrier: file } });
    expect(pipeline.counters().registrations).toBe(0);
  });

  it("a native compile failure keeps its kind", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Refused.vue", null);
    const original = Object.assign(new Error("x"), { kind: "refused" });
    vi.spyOn(session.host(), "compileRequest").mockImplementation(() => {
      throw original;
    });

    const error = (await pipeline
      .demand(keyOf(session, file), { kind: "bundler", code: TWO_STYLES }, contextOf(session))
      .catch((reason: unknown) => reason)) as Error & { kind?: string; cause?: unknown };

    expect(error).not.toBeInstanceOf(PipelineRefusalError);
    expect(error.kind).toBe("refused");
    expect(error.message).toBe(`[verter] ${file}: x`);
    expect(error.cause).toBe(original);
  });

  /** A compile that names each of `rounds` in turn, then compiles for real. */
  function naming(session: BundlerHostSession, rounds: HostMissingInput[][]) {
    const compile = vi.spyOn(session.host(), "compileRequest");
    for (const missingInputs of rounds) {
      compile.mockImplementationOnce(() => {
        throw Object.assign(new Error("dependencies incomplete"), {
          kind: "dependenciesIncomplete",
          missingInputs,
        });
      });
    }
    return compile;
  }

  const demandOf = (pipeline: CarrierPipeline, session: BundlerHostSession, file: string) =>
    pipeline.demand(
      keyOf(session, file),
      { kind: "bundler", code: TWO_STYLES },
      contextOf(session),
    );

  const failureOf = async (attempt: Promise<unknown>) =>
    (await attempt.catch((reason: unknown) => reason)) as Error & { kind?: string };

  const recordOf = (session: BundlerHostSession, file: string) =>
    session.catalog.record(canonicalizeId(file) as HostCanonicalId, session.graphFor({}));

  it("an external template source is part of the record's closure", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("External.vue", null);
    const view = root + "/view.html";
    writeFileSync(view, "<p>external</p>\n");

    const record = await pipeline.demand(
      keyOf(session, file),
      { kind: "bundler", code: '<template src="./view.html"></template>' },
      contextOf(session),
    );

    expect(record.closure.has(canonicalizeId(view) as HostCanonicalId)).toBe(true);
  });

  it("a file the host names is supplied and the compile repeats once", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Needs.vue", null);
    const wanted = root + "/wanted.d.ts";
    writeFileSync(wanted, "export type Wanted = 1\n");
    const compile = naming(session, [[{ kind: "file", canonicalId: wanted }]]);
    const upsert = vi.spyOn(session.host(), "upsert");

    const record = await demandOf(pipeline, session, file);

    expect(compile).toHaveBeenCalledTimes(2);
    expect(pipeline.counters().inputRounds).toBe(1);
    expect(upsert.mock.calls.some(([request]) => request.inputId === canonicalizeId(wanted))).toBe(
      true,
    );
    expect(record.closure.has(canonicalizeId(wanted) as HostCanonicalId)).toBe(true);
    expect(recordOf(session, file)).toBe(record);
  });

  it("a route the host names is resolved, hydrated and contributed under its importer", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Routed.vue", null);
    const target = root + "/video.d.ts";
    writeFileSync(target, "export type Video = 1\n");
    const importer = canonicalizeId(file);
    naming(session, [[{ kind: "route", importer, specifier: "./video.js", refused: false }]]);
    const setRoutes = vi.spyOn(session.host(), "setImportDependencies");

    const record = await demandOf(pipeline, session, file);

    const pushed = setRoutes.mock.calls.filter(([owner]) => owner === importer).at(-1)?.[1];
    expect(pushed?.map((route) => [route.specifier, route.resolvedCanonicalId])).toContainEqual([
      "./video.js",
      canonicalizeId(target),
    ]);
    expect(record.closure.has(canonicalizeId(target) as HostCanonicalId)).toBe(true);
  });

  it("a route inside a package is supplied like any other", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Packaged.vue", null);
    const importer = root + "/node_modules/pkg/dist/index.d.ts";
    mkdirSync(dirname(importer), { recursive: true });
    writeFileSync(importer, 'export * from "other-pkg";\n');
    mkdirSync(root + "/node_modules/other-pkg", { recursive: true });
    writeFileSync(
      root + "/node_modules/other-pkg/package.json",
      JSON.stringify({ name: "other-pkg", version: "0.0.0", types: "./index.d.ts" }),
    );
    writeFileSync(root + "/node_modules/other-pkg/index.d.ts", "export type Other = 1\n");
    naming(session, [
      [
        {
          kind: "route",
          importer: canonicalizeId(importer),
          specifier: "other-pkg",
          refused: false,
        },
      ],
    ]);
    const setRoutes = vi.spyOn(session.host(), "setImportDependencies");

    const record = await demandOf(pipeline, session, file);

    expect(pipeline.counters().inputRounds).toBe(1);
    expect(
      setRoutes.mock.calls.some(
        ([owner, routes]) =>
          owner === canonicalizeId(importer) &&
          routes.some((route) => route.specifier === "other-pkg"),
      ),
    ).toBe(true);
    expect(recordOf(session, file)).toBe(record);
  });

  it("gaps that grow are followed", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Grows.vue", null);
    const first = root + "/first.d.ts";
    const second = root + "/second.d.ts";
    writeFileSync(first, "export type A = 1\n");
    writeFileSync(second, "export type B = 1\n");
    naming(session, [
      [{ kind: "file", canonicalId: first }],
      [{ kind: "file", canonicalId: second }],
    ]);

    const record = await demandOf(pipeline, session, file);

    expect(pipeline.counters().inputRounds).toBe(2);
    expect(record.closure.has(canonicalizeId(first) as HostCanonicalId)).toBe(true);
    expect(record.closure.has(canonicalizeId(second) as HostCanonicalId)).toBe(true);
  });

  it("the same gap twice is terminal", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Twice.vue", null);
    const wanted = root + "/wanted.d.ts";
    writeFileSync(wanted, "export type Wanted = 1\n");
    const gap: HostMissingInput[] = [{ kind: "file", canonicalId: wanted }];
    naming(session, [gap, gap]);

    const error = await failureOf(demandOf(pipeline, session, file));

    expect(error).not.toBeInstanceOf(PipelineRefusalError);
    expect(error.kind).toBe("dependenciesIncomplete");
    expect(pipeline.counters().inputRounds).toBe(1);
    expect(recordOf(session, file)).toBeUndefined();
  });

  it("a report of only lookups or refused routes is terminal at once", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Refused.vue", null);
    naming(session, [
      [
        { kind: "lookup", importSource: "./x", typeName: "X" },
        { kind: "route", importer: canonicalizeId(file), specifier: "./y", refused: true },
      ],
    ]);

    const error = await failureOf(demandOf(pipeline, session, file));

    expect(error.kind).toBe("dependenciesIncomplete");
    expect(pipeline.counters().inputRounds).toBe(0);
  });

  it("a named file that cannot be read is terminal", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Missing.vue", null);
    naming(session, [[{ kind: "file", canonicalId: root + "/missing.d.ts" }]]);

    const error = await failureOf(demandOf(pipeline, session, file));

    expect(error.kind).toBe("dependenciesIncomplete");
    expect(recordOf(session, file)).toBeUndefined();
  });

  it("an incomplete-dependencies failure with nothing to supply keeps the native kind", async () => {
    const { session, pipeline } = open();
    const file = carrierFile("Incomplete.vue", null);
    vi.spyOn(session.host(), "compileRequest").mockImplementation(() => {
      throw Object.assign(new Error("dependencies incomplete"), { kind: "dependenciesIncomplete" });
    });

    const error = await failureOf(demandOf(pipeline, session, file));

    expect(error.kind).toBe("dependenciesIncomplete");
    expect(recordOf(session, file)).toBeUndefined();
  });
});
