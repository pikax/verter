import { describe, expect, it } from "vitest";

import {
  FRAMEWORK_ASSERTED_COMPLETIONS,
  FRAMEWORK_CONTRACT_CAPABILITIES,
  FRAMEWORK_CONTRACT_PRODUCT_GAP_ROUTE_KEYS,
  knownFrameworkContractGapsForRoute,
  requiredFrameworkContractIds,
} from "./frameworkContractManifest";
import { TYPE_PROVIDER_ROUTES } from "./routeInventory";

describe("framework contract manifest", () => {
  it("keeps the exact-definition stability contracts mandatory for both frameworks", () => {
    expect(FRAMEWORK_CONTRACT_CAPABILITIES).toHaveLength(33);
    expect(new Set(FRAMEWORK_CONTRACT_CAPABILITIES).size).toBe(
      FRAMEWORK_CONTRACT_CAPABILITIES.length,
    );

    for (const framework of ["vue", "svelte"] as const) {
      const ids = requiredFrameworkContractIds(framework);
      expect(ids).toHaveLength(
        FRAMEWORK_CONTRACT_CAPABILITIES.length + FRAMEWORK_ASSERTED_COMPLETIONS[framework].length,
      );
      expect(new Set(ids).size).toBe(ids.length);
      // Completion is the per-keystroke surface; the survey control and the script-region
      // control must stay required for both frameworks.
      expect(ids).toContain(`${framework}.completion.answers-every-gesture`);
      expect(ids).toContain(`${framework}.completion.script-member`);
      expect(ids).toContain(`${framework}.ts.definition.markup-to-script.exact-stable-warm`);
      expect(ids).toContain(`${framework}.js.definition.markup-to-script.exact-stable-warm`);
      expect(ids).toContain(`${framework}.import.direct.public-prop-definition.exact-stable-warm`);
      expect(ids).toContain(
        `${framework}.import.deep-barrel.public-prop-definition.exact-stable-warm`,
      );
      // The event-handler expression region must stay required for BOTH frameworks: it is
      // the only anchor set whose markup use is outside an interpolation.
      expect(ids).toContain(`${framework}.ts.definition.event-handler-to-script`);
      expect(ids).toContain(`${framework}.ts.references.event-handler`);
      expect(ids).toContain(`${framework}.ts.rename.from-event-handler`);
      expect(ids).toContain(`${framework}.ts.hover.event-handler`);
      expect(ids).toContain(`${framework}.ctrl-click.event-handler-to-script`);
    }
  });

  it("allows only main-branch-reproduced framework contract gaps", () => {
    expect(FRAMEWORK_CONTRACT_PRODUCT_GAP_ROUTE_KEYS).toEqual(
      (["vue", "svelte"] as const)
        .flatMap((framework) => TYPE_PROVIDER_ROUTES.map((provider) => `${framework}@${provider}`))
        .sort(),
    );

    for (const framework of ["vue", "svelte"] as const) {
      const required = new Set(requiredFrameworkContractIds(framework));
      for (const provider of TYPE_PROVIDER_ROUTES) {
        const gaps = knownFrameworkContractGapsForRoute(framework, provider);
        for (const [id, issue] of Object.entries(gaps)) {
          expect(required.has(id), `${framework}@${provider}: ${id}`).toBe(true);
          expect(issue).toMatch(/^ISSUE-[A-Za-z0-9_-]+$/);
        }
        const expectedGapCount =
          framework !== "vue" ? 0 : provider === "tsgo" ? 11 : provider === "tsserver" ? 5 : 0;
        expect(Object.keys(gaps)).toHaveLength(expectedGapCount);
      }
    }
  });
});
