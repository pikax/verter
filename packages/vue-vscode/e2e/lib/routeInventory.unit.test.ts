/**
 * @ai-generated - Verifies that the canonical local and CI VS Code E2E routes
 * are derived from one inventory and remain complete across all provider rails.
 */
import { describe, expect, it } from "vitest";

import {
  EDITOR_ACCEPTANCE_ROUTES,
  SHARED_TSGO_INAPPLICABLE_FIXTURES,
  STANDARD_E2E_FIXTURES,
  TYPE_PROVIDER_ROUTES,
  buildE2eRouteInventory,
  buildGitHubActionsMatrix,
  parseE2eRouteLabel,
  selectE2eRoutes,
} from "./routeInventory";

describe("VS Code E2E route inventory", () => {
  it("derives the local runner and CI matrix from the same unique route inventory", () => {
    const routes = buildE2eRouteInventory();
    const labels = routes.map(({ fixture, typeProvider }) => `${fixture}@${typeProvider}`);
    const matrix = buildGitHubActionsMatrix();

    expect(routes).toHaveLength(48);
    expect(routes.filter((route) => route.typeProvider === "tsserver")).toHaveLength(17);
    expect(routes.filter((route) => route.typeProvider === "tsgo")).toHaveLength(16);
    expect(routes.filter((route) => route.typeProvider === "shared-tsgo")).toHaveLength(15);
    expect(new Set(labels).size).toBe(labels.length);
    expect(matrix.include).toEqual(
      routes.map(({ fixture, typeProvider }) => ({
        fixture,
        type_provider: typeProvider,
      })),
    );
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
