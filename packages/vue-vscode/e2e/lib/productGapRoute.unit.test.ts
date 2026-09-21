import { describe, expect, it } from "vitest";

import { productGapCanariesForFixtureRoute, productGapsForFixtureRoute } from "./productGapRoute";

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

  it("selects canaries for an exact parity route and nowhere else", () => {
    expect(
      productGapCanariesForFixtureRoute("vue-parity", "shared-tsgo")[
        "ide.complete.import-path-carrier"
      ],
    ).toBe("ISSUE-shared-tsgo-plain-ts-consumer");
    expect(productGapCanariesForFixtureRoute("vue-parity", "tsgo")).toEqual({});
    expect(productGapCanariesForFixtureRoute("vue-contract", "shared-tsgo")).toEqual({});
    expect(productGapCanariesForFixtureRoute("single-project", "shared-tsgo")).toEqual({});
    expect(productGapCanariesForFixtureRoute("vue-parity", undefined)).toEqual({});
  });
});
