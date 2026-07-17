/**
 * @ai-generated - Verifies that component-hover safety rejects Verter's
 * historical opaque carriers without rejecting framework-native type detail.
 */
import { describe, expect, it } from "vitest";

import { assertSafeComponentHoverCarrier } from "./componentHoverContract";

describe("component hover public carrier safety", () => {
  it.each([
    "const BarrelChild: new (...args: any[]) => __VerterPublicInstance",
    "const Child: Component & { $props: any }",
    "const Child: Component & { $props: unknown }",
  ])("rejects opaque or erased public carriers: %s", (hover) => {
    expect(() => assertSafeComponentHoverCarrier(hover)).toThrow(/unsafe|generated/i);
  });

  it("accepts framework-native implementation detail when the public prop surface is concrete", () => {
    const vue = `const DirectChild: DefineComponent<{}, {}, {}, {}, {}, ComponentOptionsMixin,
      ComponentOptionsMixin, {}, string, PublicProps, ToResolvedProps<{}, {}>, {}, {}, {}, {},
      string, ComponentProvideOptions, true, {}, any> & (new (props?: PublicProps & {
        contractProp: string;
      }) => { $props: PublicProps & { contractProp: string } })`;
    const svelte = `**Props:**\n- \`barrelProp\`: string\n\n**Usage:**\n- ? \`:barrelProp\` — *unknown*`;

    expect(() => assertSafeComponentHoverCarrier(vue)).not.toThrow();
    expect(() => assertSafeComponentHoverCarrier(svelte)).not.toThrow();
  });
});
