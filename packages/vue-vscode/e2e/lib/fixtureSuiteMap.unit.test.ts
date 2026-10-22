import { describe, expect, it } from "vitest";

import { suiteAllowedForFixture } from "./fixtureSuiteMap";

describe("fixture suite selection", () => {
  it("loads the server lifecycle suite on a legacy fixture without admitting a parity suite that merely shares its words", () => {
    expect(suiteAllowedForFixture("monorepo", "server-lifecycle.test.js")).toBe(true);
    // Globs match by substring, so a legacy glob must not occur inside a parity path:
    // that suite throws in its before-all hook on a fixture it does not apply to.
    expect(suiteAllowedForFixture("monorepo", "parity/shared/product-lifecycle.test.js")).toBe(
      false,
    );
    expect(suiteAllowedForFixture("tsconfig-extends", "parity/lsp-extras.test.js")).toBe(false);
  });
});
