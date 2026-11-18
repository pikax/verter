import { describe, expect, it } from "vitest";
import type { HostDependencyResolution, VerterHost } from "@verter/native";
import type { HostCanonicalId } from "../host-session/identity";
import { createPipelineCounters } from "./counters";
import { DependencyRouteTable } from "./route-table";

const OWNER = "/proj/src/app.vue" as HostCanonicalId;

function makeTable() {
  const pushes: { owner: string; routes: HostDependencyResolution[] }[] = [];
  const host = {
    setImportDependencies(owner: string, routes: HostDependencyResolution[]): void {
      pushes.push({ owner, routes });
    },
  } as unknown as VerterHost;
  const counters = createPipelineCounters();
  return { table: new DependencyRouteTable(host, counters), pushes, counters };
}

const route = (specifier: string, target: string): HostDependencyResolution => ({
  specifier,
  resolvedCanonicalId: target,
});

describe("DependencyRouteTable", () => {
  it("hands the host the union of every contribution", () => {
    const { table, pushes, counters } = makeTable();

    table.contribute(OWNER, [route("a", "/proj/a.ts")], "module");
    table.contribute(OWNER, [route("b", "/proj/b.d.ts")], "declaration");

    expect(pushes.at(-1)?.routes).toEqual([route("a", "/proj/a.ts"), route("b", "/proj/b.d.ts")]);
    expect(pushes.every((push) => push.owner === OWNER)).toBe(true);
    expect(counters.routePushes).toBe(2);
  });

  it.each([
    ["module first", ["module", "declaration"] as const],
    ["declaration first", ["declaration", "module"] as const],
  ])("lets the declaration route win for a specifier both tiers name (%s)", (_name, order) => {
    const { table, pushes } = makeTable();
    const contributions = {
      module: route("pkg", "/proj/node_modules/pkg/dist/index.js"),
      declaration: route("pkg", "/proj/node_modules/pkg/dist/index.d.ts"),
    };

    for (const tier of order) table.contribute(OWNER, [contributions[tier]], tier);

    expect(pushes.at(-1)?.routes).toEqual([contributions.declaration]);
    expect(table.routesOf(OWNER)).toEqual([contributions.declaration]);
  });

  it("starts an owner's table again after forgetting it", () => {
    const { table, pushes } = makeTable();
    table.contribute(OWNER, [route("a", "/proj/a.ts")], "module");

    table.forget(OWNER);
    table.contribute(OWNER, [route("c", "/proj/c.ts")], "module");

    expect(pushes.at(-1)?.routes).toEqual([route("c", "/proj/c.ts")]);
  });

  it("replaces an earlier route for the same specifier in the same tier", () => {
    const { table, pushes } = makeTable();

    table.contribute(OWNER, [route("a", "/proj/old.ts")], "module");
    table.contribute(OWNER, [route("a", "/proj/new.ts")], "module");

    expect(pushes.at(-1)?.routes).toEqual([route("a", "/proj/new.ts")]);
  });

  it("keeps owners apart", () => {
    const { table } = makeTable();
    const other = "/proj/src/other.vue" as HostCanonicalId;

    table.contribute(OWNER, [route("a", "/proj/a.ts")], "module");
    table.contribute(other, [route("b", "/proj/b.ts")], "module");
    table.forget(OWNER);

    expect(table.routesOf(OWNER)).toEqual([]);
    expect(table.routesOf(other)).toEqual([route("b", "/proj/b.ts")]);
  });

  it("pushes an empty contribution only when asked", () => {
    const { table, pushes } = makeTable();

    table.contribute(OWNER, [], "module");
    expect(pushes).toEqual([]);

    table.contribute(OWNER, [], "module", { always: true });
    expect(pushes).toEqual([{ owner: OWNER, routes: [] }]);
  });
});
