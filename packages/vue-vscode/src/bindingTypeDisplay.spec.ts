/**
 * Q1 acceptance fixture for the `$/verter/getBindingTypes` wire: tooltip and
 * description rendering of the provider's `displaySignature`.
 *
 * Written against the pure helper module the tree providers CALL
 * (`bindingTypeDisplay.ts`) — never an inline re-implementation (the
 * `analysis-response.spec.ts:234` mirror pattern does not discriminate and is
 * explicitly not acceptable here).
 *
 * The wire value is display-only: consumers render it VERBATIM and never
 * split, trim to a right-hand side, or otherwise recover structure from it.
 */
import { describe, it, expect } from "vitest";
import {
  bindingLeafDescription,
  bindingLeafTooltip,
  bindingSignature,
  bindingTooltipLine,
  propSignatureDescription,
  type BindingTypeEntry,
} from "./bindingTypeDisplay";

const count: BindingTypeEntry = { displaySignature: "const count: Ref<number>" };
const stale: BindingTypeEntry = null;

describe("bindingTypeDisplay", () => {
  it("renders the leaf tooltip as the signature VERBATIM", () => {
    expect(bindingLeafTooltip("count", count)).toBe("const count: Ref<number>");
  });

  it("does not duplicate the binding name (no `${name}: ` prefix)", () => {
    // The load-bearing negative: the deleted defect rendered
    // `count: const count: Ref<number>`.
    expect(bindingLeafTooltip("count", count)).not.toContain("count: const ");
  });

  it("never renders markdown artifacts", () => {
    const rendered = [
      bindingLeafTooltip("count", count),
      bindingLeafDescription(count),
      bindingTooltipLine(count),
      propSignatureDescription(bindingSignature(count), "const"),
    ];
    for (const text of rendered) {
      expect(text).not.toContain("```");
      expect(text).not.toContain("typescript");
    }
  });

  it("renders null as absent — never the strings 'null'/'undefined'", () => {
    expect(bindingSignature(stale)).toBeNull();
    expect(bindingLeafDescription(stale)).toBe("");
    expect(bindingTooltipLine(stale)).toBe("");
    const tooltip = bindingLeafTooltip("count", stale);
    expect(tooltip).not.toContain("null");
    expect(tooltip).not.toContain("undefined");
  });

  it("renders the component-tree prop description from the same helper", () => {
    expect(propSignatureDescription(bindingSignature(count), "const")).toBe(
      "(const count: Ref<number>, const)",
    );
  });

  it("labels the binding tooltip line as a signature, not a type", () => {
    // The `TSGO type:` label carried a full display signature; the line is now
    // labelled for what it is.
    const line = bindingTooltipLine(count);
    expect(line).toBe("Signature: const count: Ref<number>");
    expect(line).not.toContain("TSGO type:");
  });
});
