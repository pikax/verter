import { describe, expect, it } from "vitest";

import {
  FRAMEWORK_CONTRACT_CAPABILITIES,
  requiredFrameworkContractIds,
} from "./frameworkContractManifest";

describe("framework contract manifest", () => {
  it("keeps the exact-definition stability contracts mandatory for both frameworks", () => {
    expect(FRAMEWORK_CONTRACT_CAPABILITIES).toHaveLength(26);
    expect(new Set(FRAMEWORK_CONTRACT_CAPABILITIES).size).toBe(
      FRAMEWORK_CONTRACT_CAPABILITIES.length,
    );

    for (const framework of ["vue", "svelte"] as const) {
      const ids = requiredFrameworkContractIds(framework);
      expect(ids).toHaveLength(FRAMEWORK_CONTRACT_CAPABILITIES.length);
      expect(ids).toContain(`${framework}.ts.definition.markup-to-script.exact-stable-warm`);
      expect(ids).toContain(`${framework}.js.definition.markup-to-script.exact-stable-warm`);
      expect(ids).toContain(`${framework}.import.direct.public-prop-definition.exact-stable-warm`);
      expect(ids).toContain(
        `${framework}.import.deep-barrel.public-prop-definition.exact-stable-warm`,
      );
    }
  });
});
