/**
 * @ai-generated - Verifies that the canonical local and CI VS Code E2E routes
 * are derived from one inventory and remain complete across all provider rails.
 */
import { describe, expect, it } from "vitest";

import {
  EDITOR_ACCEPTANCE_ROUTES,
  EXTENSION_ACCEPTANCE_ROUTES,
  NON_REQUIRED_E2E_ROUTES,
  SHARED_TSGO_INAPPLICABLE_FIXTURES,
  STANDARD_E2E_FIXTURES,
  TYPE_PROVIDER_ROUTES,
  buildE2eRouteInventory,
  buildGitHubActionsMatrix,
  buildRequiredE2eRouteInventory,
  parseE2eRouteLabel,
  resolveE2eFixtureSelection,
  selectE2eRoutes,
} from "./routeInventory";

describe("VS Code E2E route inventory", () => {
  it("derives the local runner and CI matrix from the same unique route inventory", () => {
    const routes = buildE2eRouteInventory();
    const labels = routes.map(({ fixture, typeProvider }) => `${fixture}@${typeProvider}`);
    const matrix = buildGitHubActionsMatrix();

    expect(routes).toHaveLength(49);
    expect(routes.filter((route) => route.typeProvider === "tsserver")).toHaveLength(16);
    expect(routes.filter((route) => route.typeProvider === "tsgo")).toHaveLength(16);
    expect(routes.filter((route) => route.typeProvider === "shared-tsgo")).toHaveLength(15);
    // The editor-owned tier is never selected automatically, so it appears
    // exactly once — on the fixture that exists to exercise it.
    expect(routes.filter((route) => route.typeProvider === "editor-tsserver")).toHaveLength(1);
    // Same for the extension-hosted tier: one acceptance route, on the
    // out-of-tree workspace that is the only layout able to discriminate its
    // project-bound TypeScript resolution.
    expect(routes.filter((route) => route.typeProvider === "extension")).toHaveLength(1);
    expect(new Set(labels).size).toBe(labels.length);
    expect(matrix.include).toEqual(
      buildRequiredE2eRouteInventory().map(({ fixture, typeProvider }) => ({
        fixture,
        type_provider: typeProvider,
      })),
    );
  });

  it("keeps a deselected route selectable while excluding it from the required matrix", () => {
    const required = buildRequiredE2eRouteInventory();
    const inventory = buildE2eRouteInventory();

    expect(NON_REQUIRED_E2E_ROUTES).toHaveLength(1);
    const [deselected] = NON_REQUIRED_E2E_ROUTES;
    expect(deselected.route).toEqual({
      fixture: "out-of-tree-monorepo",
      typeProvider: "extension",
    });
    expect(deselected.reason).toMatch(/TypeProviderKind::Tsserver/);
    // A deselected route must not read as merely broken: the reason states the
    // interim containment the product ships, so the next reader knows the
    // setting warns instead of silently answering nothing.
    expect(deselected.reason).toMatch(/contained/i);
    expect(deselected.reason).toMatch(/warn/i);

    // Every deselected route is a declared route: the required set is the
    // inventory minus exactly those, never a set that drops a route silently.
    for (const { route } of NON_REQUIRED_E2E_ROUTES) {
      expect(inventory).toContainEqual(route);
      expect(required).not.toContainEqual(route);
    }
    expect(required).toHaveLength(inventory.length - NON_REQUIRED_E2E_ROUTES.length);

    // The default (unselected) run is the required matrix.
    expect(selectE2eRoutes({})).toEqual(required);
    // An explicit selector still reaches it, by label and by fixture.
    expect(parseE2eRouteLabel("out-of-tree-monorepo@extension")).toEqual(deselected.route);
    expect(selectE2eRoutes({ fixture: "out-of-tree-monorepo" })).toEqual([deselected.route]);
    expect(selectE2eRoutes({ typeProvider: "extension" })).toEqual([deselected.route]);
  });

  it("runs every project-bound standard fixture on all three provider routes", () => {
    const routes = buildE2eRouteInventory();

    for (const fixture of STANDARD_E2E_FIXTURES) {
      const providers = routes
        .filter((route) => route.fixture === fixture)
        .map((route) => route.typeProvider)
        .sort();
      if (SHARED_TSGO_INAPPLICABLE_FIXTURES.includes(fixture as never)) {
        expect(providers).toEqual(["tsgo", "tsserver"]);
      } else {
        expect(providers).toEqual([...TYPE_PROVIDER_ROUTES].sort());
      }
    }
  });

  it("retains the explicit editor-owned acceptance routes without inventing managed coverage", () => {
    const routes = buildE2eRouteInventory();
    for (const required of EDITOR_ACCEPTANCE_ROUTES) {
      expect(routes).toContainEqual(required);
    }
    expect(routes).not.toContainEqual({ fixture: "editor-owned-project", typeProvider: "tsgo" });
  });

  it("retains the extension-hosted acceptance route on its out-of-tree workspace", () => {
    const routes = buildE2eRouteInventory();
    for (const required of EXTENSION_ACCEPTANCE_ROUTES) {
      expect(routes).toContainEqual(required);
    }
    // The extension provider is never selected by the standard matrix: its
    // resolution model needs a workspace outside this repository, and an in-repo
    // fixture would pass against the very defect the route exists to catch.
    expect(routes).not.toContainEqual({ fixture: "monorepo", typeProvider: "extension" });
    expect(routes).not.toContainEqual({
      fixture: "out-of-tree-monorepo",
      typeProvider: "tsserver",
    });
  });

  it("expands fixture-only selectors and fails closed when a selector matches no route", () => {
    expect(selectE2eRoutes({ fixture: "svelte-contract" })).toEqual(
      TYPE_PROVIDER_ROUTES.map((typeProvider) => ({
        fixture: "svelte-contract",
        typeProvider,
      })),
    );
    expect(() => selectE2eRoutes({ fixture: "sveltte-contract" })).toThrow(/matched nothing/i);
    expect(() => selectE2eRoutes({ typeProvider: "unsupported" })).toThrow(
      /unsupported.*provider/i,
    );
    expect(() => parseE2eRouteLabel("sveltte-contract@tsgo")).toThrow(/matched nothing/i);
    expect(() => parseE2eRouteLabel("no-config@shared-tsgo")).toThrow(/matched nothing/i);
  });
});

describe("resolveE2eFixtureSelection", () => {
  it("resolves the ordinary selections a launcher is given", () => {
    expect(resolveE2eFixtureSelection({})).toEqual({
      fixture: "single-project",
      typeProvider: "",
    });
    expect(resolveE2eFixtureSelection({ rawFixture: "barrel-exports" })).toEqual({
      fixture: "barrel-exports",
      typeProvider: "",
    });
    expect(resolveE2eFixtureSelection({ rawFixture: "no-config", typeProvider: "tsgo" })).toEqual({
      fixture: "no-config",
      typeProvider: "tsgo",
    });
    // The `<fixture>@<provider>` spelling, and it beats the separate variable.
    expect(
      resolveE2eFixtureSelection({ rawFixture: "monorepo@tsgo", typeProvider: "tsserver" }),
    ).toEqual({ fixture: "monorepo", typeProvider: "tsgo" });
  });

  it("refuses a selection that names no route, before anyone builds a path from it", () => {
    // A launcher joins this onto `e2e/fixtures/`, so an unchecked value escapes
    // the fixture directory: `../..` is `packages/vue-vscode`, which has a
    // `package.json` and a pnpm-managed `node_modules`. Deciding to replace a
    // dependency tree is now a real action, so the value that selects the tree
    // has to name a route that exists.
    expect(() => resolveE2eFixtureSelection({ rawFixture: "../.." })).toThrow(/matched nothing/i);
    expect(() => resolveE2eFixtureSelection({ rawFixture: ".." })).toThrow(/matched nothing/i);
    expect(() => resolveE2eFixtureSelection({ rawFixture: "/etc" })).toThrow(/matched nothing/i);
    expect(() =>
      resolveE2eFixtureSelection({ rawFixture: "../../../e2e/fixtures/single-project" }),
    ).toThrow(/matched nothing/i);
    // A real fixture directory that is not a route is still not selectable here.
    expect(() => resolveE2eFixtureSelection({ rawFixture: "external-ts-dx" })).toThrow(
      /matched nothing/i,
    );
    expect(() => resolveE2eFixtureSelection({ rawFixture: "single-project@nope" })).toThrow(
      /unsupported.*provider/i,
    );
  });
});
