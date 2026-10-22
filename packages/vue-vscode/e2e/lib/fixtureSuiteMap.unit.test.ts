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

  it("loads the teardown recording on every server-owning specialty fixture", () => {
    // The launcher requires a recording only where this predicate holds, so a fixture
    // that omits the suite would pass with an orphaned server undetectable.
    for (const fixture of [
      "vue-parity",
      "svelte-parity",
      "mixed-parity",
      "multi-root-parity",
      "ecosystem-parity",
      "vue-contract",
      "svelte-contract",
    ]) {
      expect(suiteAllowedForFixture(fixture, "_teardown.test.js"), fixture).toBe(true);
    }
  });
});
