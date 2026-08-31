import { describe, expect, it } from "vitest";

import { productGapsForFixtureRoute } from "./productGapRoute";

describe("product-gap route selection", () => {
  it("selects exact parity debt without absorbing an unapproved regression", () => {
    const gaps = productGapsForFixtureRoute("svelte-parity", "tsserver");
    expect(gaps["strict.svelte.rest-props-opt-in"]).toBe("ISSUE-svelte-strict-rest-props");
    expect(gaps["svelte.references.script-and-markup"]).toBeUndefined();
  });

  it("selects focused framework-contract debt", () => {
    expect(
      productGapsForFixtureRoute("vue-contract", "tsserver")["vue.js.rename.from-script"],
    ).toBe("ISSUE-vue-contract-rename");
    expect(productGapsForFixtureRoute("svelte-contract", "tsserver")).toEqual({});
  });

  it("does not skip tests outside an exact product-gap fixture route", () => {
    expect(productGapsForFixtureRoute("single-project", "tsserver")).toEqual({});
    expect(productGapsForFixtureRoute("vue-parity", "extension")).toEqual({});
  });
});
