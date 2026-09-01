/**
 * @ai-generated - Verifies that the canonical local and CI VS Code E2E routes
 * are derived from one inventory and remain complete across all provider rails.
 */
import { describe, expect, it } from "vitest";

import { FIXTURE_SUITE_GLOBS } from "./fixtureSuiteMap";
import {
  BARREL_REGRESSION_LOADED_FILES,
  BARREL_REGRESSION_SUITE_GLOB,
  BARREL_REGRESSION_TEST_IDS,
} from "./barrelRegressionManifest";
import {
  PROJECTLESS_CONTRACT_LOADED_FILES,
  PROJECTLESS_CONTRACT_TEST_IDS,
} from "./projectlessContractManifest";

import {
  EDITOR_ACCEPTANCE_ROUTES,
  EXTENSION_ACCEPTANCE_ROUTES,
  NON_REQUIRED_E2E_ROUTES,
  STANDARD_E2E_FIXTURES,
  TYPE_PROVIDER_ROUTES,
  buildE2eRouteInventory,
  buildGitHubActionsMatrix,
  buildRequiredE2eRouteInventory,
  e2eRouteLabel,
  parseE2eRouteLabel,
  resolveE2eFixtureSelection,
  selectE2eRoutes,
} from "./routeInventory";

describe("VS Code E2E route inventory", () => {
  it("derives the local runner and CI matrix from the same unique route inventory", () => {
    const routes = buildE2eRouteInventory();
    const labels = routes.map(({ fixture, typeProvider }) => `${fixture}@${typeProvider}`);
    const matrix = buildGitHubActionsMatrix();

    expect(routes).toHaveLength(47);
    expect(routes.filter((route) => route.typeProvider === "tsserver")).toHaveLength(14);
    expect(routes.filter((route) => route.typeProvider === "tsgo")).toHaveLength(14);
    expect(routes.filter((route) => route.typeProvider === "shared-tsgo")).toHaveLength(15);
    // The editor-owned tier is never selected automatically, so it appears
    // exactly once — on the fixture that exists to exercise it.
    expect(routes.filter((route) => route.typeProvider === "editor-tsserver")).toHaveLength(1);
    // Same for the extension-hosted tier: one acceptance route, on the
    // out-of-tree workspace that is the only layout able to discriminate its
    // project-bound TypeScript resolution.
    expect(routes.filter((route) => route.typeProvider === "extension")).toHaveLength(1);
    expect(routes.filter((route) => route.typeProvider === "off")).toHaveLength(2);
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

    expect(NON_REQUIRED_E2E_ROUTES).toHaveLength(15);
    expect(NON_REQUIRED_E2E_ROUTES.map(({ route }) => e2eRouteLabel(route)).sort()).toEqual(
      [
        "barrel-exports@shared-tsgo",
        "composite-paths@shared-tsgo",
        "editor-owned-project@shared-tsgo",
        "monorepo@shared-tsgo",
        "out-of-tree-monorepo@extension",
        "path-aliases@shared-tsgo",
        "single-project@shared-tsgo",
        "single-project@tsgo",
        "single-project@tsserver",
        "svelte-contract@shared-tsgo",
        "svelte-parity@shared-tsgo",
        "tsconfig-extends@shared-tsgo",
        "tsconfig-references@shared-tsgo",
        "vue-contract@shared-tsgo",
        "vue-parity@shared-tsgo",
      ].sort(),
    );
    const extensionRoute = NON_REQUIRED_E2E_ROUTES.find(
      ({ route }) => route.typeProvider === "extension",
    );
    expect(extensionRoute?.route).toEqual({
      fixture: "out-of-tree-monorepo",
      typeProvider: "extension",
    });
    expect(extensionRoute?.reason).toMatch(/TypeProviderKind::Tsserver/);
    // A deselected route must not read as merely broken: the reason states the
    // interim containment the product ships, so the next reader knows the
    // setting warns instead of silently answering nothing.
    expect(extensionRoute?.reason).toMatch(/contained/i);
    expect(extensionRoute?.reason).toMatch(/warn/i);

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
    expect(parseE2eRouteLabel("out-of-tree-monorepo@extension")).toEqual(extensionRoute?.route);
    expect(selectE2eRoutes({ fixture: "out-of-tree-monorepo" })).toEqual([extensionRoute?.route]);
    expect(selectE2eRoutes({ typeProvider: "extension" })).toEqual([extensionRoute?.route]);

    for (const fixture of ["vue-parity", "svelte-parity"] as const) {
      for (const typeProvider of ["tsserver", "tsgo"] as const) {
        expect(required).toContainEqual({ fixture, typeProvider });
      }
      expect(required).not.toContainEqual({ fixture, typeProvider: "shared-tsgo" });
    }
    for (const fixture of ["mixed-parity", "multi-root-parity", "ecosystem-parity"] as const) {
      for (const typeProvider of TYPE_PROVIDER_ROUTES) {
        expect(required).toContainEqual({ fixture, typeProvider });
      }
    }
  });

  it("runs every configured standard fixture on all three provider routes", () => {
    const routes = buildE2eRouteInventory();

    for (const fixture of STANDARD_E2E_FIXTURES) {
      const providers = routes
        .filter((route) => route.fixture === fixture)
        .map((route) => route.typeProvider)
        .sort();
      expect(providers).toEqual([...TYPE_PROVIDER_ROUTES].sort());
    }
  });

  it("runs projectless fixtures only on the explicit provider-off route", () => {
    const routes = buildE2eRouteInventory();
    for (const fixture of ["no-config", "single-file"] as const) {
      expect(routes.filter((route) => route.fixture === fixture)).toEqual([
        { fixture, typeProvider: "off" },
      ]);
      expect(FIXTURE_SUITE_GLOBS[fixture]).toEqual(["projectless-contract.test"]);
    }
    expect(PROJECTLESS_CONTRACT_LOADED_FILES).toEqual(["projectless-contract.test.js"]);
    expect(PROJECTLESS_CONTRACT_TEST_IDS).toHaveLength(4);
    expect(new Set(PROJECTLESS_CONTRACT_TEST_IDS).size).toBe(PROJECTLESS_CONTRACT_TEST_IDS.length);
  });

  it("keeps barrel CI focused on the exact typed public-surface regression contract", () => {
    expect(FIXTURE_SUITE_GLOBS["barrel-exports"]).toEqual([BARREL_REGRESSION_SUITE_GLOB]);
    expect(BARREL_REGRESSION_LOADED_FILES).toEqual(["barrel-type-integrity.test.js"]);
    expect(BARREL_REGRESSION_TEST_IDS).toHaveLength(9);
    expect(new Set(BARREL_REGRESSION_TEST_IDS).size).toBe(BARREL_REGRESSION_TEST_IDS.length);
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
    expect(() => parseE2eRouteLabel("no-config@tsgo")).toThrow(/matched nothing/i);
    expect(parseE2eRouteLabel("no-config@off")).toEqual({
      fixture: "no-config",
      typeProvider: "off",
    });
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
    expect(resolveE2eFixtureSelection({ rawFixture: "no-config", typeProvider: "off" })).toEqual({
      fixture: "no-config",
      typeProvider: "off",
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
