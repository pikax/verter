import { describe, expect, it } from "vitest";

import {
  FRAMEWORK_ASSERTED_COMPLETIONS,
  FRAMEWORK_CONTRACT_CAPABILITIES,
  requiredFrameworkContractIds,
} from "./frameworkContractManifest";

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
});
