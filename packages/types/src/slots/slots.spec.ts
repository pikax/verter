/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for slot type helpers including:
 * - StrictRenderSlot: type-safe slot rendering with required props
 * - extractArgumentsFromRenderSlot: extracts slot props from component instances
 * - Validates slot prop typing and VNode return types
 */
import "vue/jsx";
import { assertType, describe, it } from "vitest";
import { defineComponent, SlotsType } from "vue";
import { strictRenderSlot, extractArgumentsFromRenderSlot } from "./slots";
import { instantiateComponent } from "../components/components";

describe("StrictRenderSlot", () => {
  const TabItem = defineComponent({
    props: {
      id: { type: String, required: true },
    },
  });

  const Tab = defineComponent({
    slots: {} as SlotsType<{
      default: () => (typeof TabItem)[];
      single: () => typeof TabItem;
      notEmpty: () => [typeof TabItem, ...Array<typeof TabItem>];

      tuple: () => [typeof TabItem, typeof TabItem];

      string: () => string;
      stringMixed: () => string | typeof TabItem;

      literal: () => "foo";
      literalMixed: () => "foo" | "bar";
      literalMultiple: () => ("foo" | "bar")[];
      literalTuple: () => ["foo", typeof TabItem];

      spanElement: () => HTMLSpanElement;
      spanElements: () => HTMLSpanElement[];
    }>,
  });

  const UnpatchedTabItems = defineComponent({
    props: {
      catsAreTheBest: Boolean,
    },
  });

  const c = new Tab();

  it("accepts array children for array-returning slot", () => {
    strictRenderSlot(c.$slots.default, [TabItem, TabItem]);
    strictRenderSlot(c.$slots.default, []);
    strictRenderSlot(c.$slots.default, [TabItem]);
    // @ts-expect-error wrong child type
    strictRenderSlot(c.$slots.default, [UnpatchedTabItems]);
    // additional sanity: mixing non-matching type still errors
    // @ts-expect-error wrong child type in array
    strictRenderSlot(c.$slots.default, [TabItem, UnpatchedTabItems]);
  });

  it("requires single-element tuple for single-returning slot", () => {
    strictRenderSlot(c.$slots.single, [TabItem]);
    // @ts-expect-error multiple children not allowed for single slot
    strictRenderSlot(c.$slots.single, [TabItem, TabItem]);
    // @ts-expect-error wrong child type
    strictRenderSlot(c.$slots.single, [UnpatchedTabItems]);
  });

  it("requires non-empty array for notEmpty-returning slot", () => {
    strictRenderSlot(c.$slots.notEmpty, [TabItem]);
    strictRenderSlot(c.$slots.notEmpty, [TabItem, TabItem]);
    // @ts-expect-error empty array not allowed
    strictRenderSlot(c.$slots.notEmpty, []);
    // @ts-expect-error wrong child type
    strictRenderSlot(c.$slots.notEmpty, [UnpatchedTabItems]);
  });

  it("supports tuple-returning slot", () => {
    strictRenderSlot(c.$slots.tuple, [TabItem, TabItem]);
    // @ts-expect-error wrong number of children
    strictRenderSlot(c.$slots.tuple, [TabItem]);
    // @ts-expect-error wrong child type
    strictRenderSlot(c.$slots.tuple, [TabItem, UnpatchedTabItems]);
  });

  it("support for string", () => {
    strictRenderSlot(c.$slots.string, ["just a string"]);
    // @ts-expect-error multiple strings allowed
    strictRenderSlot(c.$slots.string, ["one", "two"]);
    // @ts-expect-error wrong type
    strictRenderSlot(c.$slots.string, [TabItem]);
  });
  it("support for string | TabItem", () => {
    strictRenderSlot(c.$slots.stringMixed, ["just a string"]);
    strictRenderSlot(c.$slots.stringMixed, [TabItem]);
    // @ts-expect-error wrong type
    strictRenderSlot(c.$slots.stringMixed, [UnpatchedTabItems]);
  });
  it("support for named slot returning literal", () => {
    strictRenderSlot(c.$slots.literal, ["foo"]);
    // @ts-expect-error wrong literal
    strictRenderSlot(c.$slots.literal, ["bar"]);
    // @ts-expect-error wrong type
    strictRenderSlot(c.$slots.literal, [TabItem]);
    // @ts-expect-error wrong number of literals
    strictRenderSlot(c.$slots.literal, ["foo", "foo"]);
  });

  it("support for named slot returning literal union", () => {
    strictRenderSlot(c.$slots.literalMixed, ["foo"]);
    strictRenderSlot(c.$slots.literalMixed, ["bar"]);

    // @ts-expect-error wrong literal
    strictRenderSlot(c.$slots.literalMixed, ["baz"]);

    // @ts-expect-error wrong type
    strictRenderSlot(c.$slots.literalMixed, [TabItem]);
  });

  it("support for named slot returning literal union array", () => {
    strictRenderSlot(c.$slots.literalMultiple, ["foo", "bar", "foo"]);
    strictRenderSlot(c.$slots.literalMultiple, []);
    strictRenderSlot(c.$slots.literalMultiple, ["bar"]);
    // Note: ["baz"] cannot be rejected because TypeScript widens it to string[]
    // To get strict literal checking, users must use `as const`: ["baz"] as const
    // @ts-expect-error wrong type
    strictRenderSlot(c.$slots.literalMultiple, ["baz" as const]);
    // @ts-expect-error wrong type
    strictRenderSlot(c.$slots.literalMultiple, [TabItem]);
  });

  it("support for named slot returning tuple of literal and component", () => {
    strictRenderSlot(c.$slots.literalTuple, ["foo", TabItem]);
    // @ts-expect-error wrong literal
    strictRenderSlot(c.$slots.literalTuple, ["bar", TabItem]);
    // @ts-expect-error wrong type
    strictRenderSlot(c.$slots.literalTuple, ["foo", UnpatchedTabItems]);
  });

  it("support for named slot returning HTMLSpanElement", () => {
    strictRenderSlot(c.$slots.spanElement, [document.createElement("span") as HTMLSpanElement]);

    // @ts-expect-error wrong type
    strictRenderSlot(c.$slots.spanElement, [document.createElement("div") as HTMLDivElement]);
  });

  it("support for named slot returning HTMLSpanElement[]", () => {
    strictRenderSlot(c.$slots.spanElements, [document.createElement("span") as HTMLSpanElement]);

    // @ts-expect-error wrong type
    strictRenderSlot(c.$slots.spanElements, [document.createElement("div") as HTMLDivElement]);
  });
});

// Tests for strictRenderSlot using the Verter-generated pattern:
// function Comp() { return instantiateComponent(Component, {}); }
// strictRenderSlot({} as NonNullable<ReturnType<typeof Comp>['$slots']['default']>, [...])
describe("StrictRenderSlot with Verter codegen pattern", () => {
  const TabItem = defineComponent({
    props: {
      id: { type: String, required: true },
    },
  });

  const OtherComp = defineComponent({
    props: {
      label: { type: String, required: true },
    },
  });

  const Tabs = defineComponent({
    slots: {} as SlotsType<{
      default: () => (typeof TabItem)[];
      header: () => HTMLSpanElement[];
    }>,
  });

  // This simulates the Verter-generated Comp function pattern
  function ___VERTER___Comp42() {
    return instantiateComponent(Tabs, {});
  }

  // Type aliases to keep @ts-expect-error on single-line calls
  type DefaultSlot = NonNullable<ReturnType<typeof ___VERTER___Comp42>["$slots"]["default"]>;
  type HeaderSlot = NonNullable<ReturnType<typeof ___VERTER___Comp42>["$slots"]["header"]>;

  it("accepts correct children via Comp function ReturnType pattern", () => {
    // This is the exact pattern Verter generates
    strictRenderSlot({} as DefaultSlot, [TabItem]);
    strictRenderSlot({} as DefaultSlot, [TabItem, TabItem]);
    // @ts-expect-error wrong child type via Comp function pattern
    strictRenderSlot({} as DefaultSlot, [OtherComp]);
  });

  it("works with HTML element slot types via Comp function pattern", () => {
    strictRenderSlot({} as HeaderSlot, [{} as HTMLSpanElement]);
    // @ts-expect-error wrong HTML element type
    strictRenderSlot({} as HeaderSlot, [{} as HTMLDivElement]);
  });

  it("works with HTMLElementTagNameMap pattern (what Verter emits for HTML children)", () => {
    // Verter generates: {} as HTMLElementTagNameMap["span"] for <span/> children
    strictRenderSlot({} as HeaderSlot, [{} as HTMLElementTagNameMap["span"]]);
    // @ts-expect-error wrong element tag
    strictRenderSlot({} as HeaderSlot, [{} as HTMLElementTagNameMap["div"]]);
  });
});

// @ai-generated - Tests for extractArgumentsFromRenderSlot helper
describe("extractArgumentsFromRenderSlot", () => {
  const MyCompSlots = defineComponent({
    slots: {} as SlotsType<{
      default: (props: { msg: string }) => any;
      header: (props: { title: string; subtitle?: string }) => any;
      footer: () => any;
      complex: (props: { items: { id: number; name: string }[] }) => any;
    }>,
  });

  // Simulate how component-type plugin creates component instances
  function Comp1() {
    return new MyCompSlots();
  }

  it("extracts props from slot with single prop", () => {
    const result = extractArgumentsFromRenderSlot(Comp1(), "default");
    type Result = typeof result;
    type Expected = { msg: string };

    assertType<Result>({} as Expected);
    assertType<Expected>({} as Result);

    // @ts-expect-error - Result is not any/unknown/never
    assertType<{ unrelated: true }>({} as Result);

    // Verify prop access
    result.msg satisfies string;
  });

  it("extracts props from slot with multiple props", () => {
    const result = extractArgumentsFromRenderSlot(Comp1(), "header");
    type Result = typeof result;
    type Expected = { title: string; subtitle?: string };

    assertType<Result>({} as Expected);
    assertType<Expected>({} as Result);

    // @ts-expect-error - Result is not any/unknown/never
    assertType<{ unrelated: true }>({} as Result);

    // Verify prop access
    result.title satisfies string;
    result.subtitle satisfies string | undefined;
  });

  it("handles slot without props (returns undefined)", () => {
    const result = extractArgumentsFromRenderSlot(Comp1(), "footer");
    type Result = typeof result;

    // Footer slot has no props, so Parameters<() => any>[0] is undefined
    // The result type should be undefined when there are no slot props
    result satisfies undefined;
  });

  it("extracts complex nested props from slot", () => {
    const result = extractArgumentsFromRenderSlot(Comp1(), "complex");
    type Result = typeof result;
    type Expected = { items: { id: number; name: string }[] };

    assertType<Result>({} as Expected);
    assertType<Expected>({} as Result);

    // @ts-expect-error - Result is not any/unknown/never
    assertType<{ unrelated: true }>({} as Result);

    // Verify nested access
    result.items[0].id satisfies number;
    result.items[0].name satisfies string;
  });

  it("works with component instance directly", () => {
    const instance = new MyCompSlots();
    const result = extractArgumentsFromRenderSlot(instance, "default");

    result.msg satisfies string;

    // @ts-expect-error - wrong prop name
    result.wrongProp;
  });

  it("provides type safety for slot names", () => {
    // Valid slot names compile
    extractArgumentsFromRenderSlot(Comp1(), "default");
    extractArgumentsFromRenderSlot(Comp1(), "header");
    extractArgumentsFromRenderSlot(Comp1(), "footer");
    extractArgumentsFromRenderSlot(Comp1(), "complex");

    // @ts-expect-error - invalid slot name
    extractArgumentsFromRenderSlot(Comp1(), "nonexistent");
  });

  it("preserves slot props for public-api-style constructor components", () => {
    type PublicApiComponent = {
      new (props?: {}): {
        $props: {};
        $slots: {
          default(props: {
            slotItem: { id: number; name: string };
            slotIndex: number;
            slotTotal: number;
          }): any;
        };
      };
    };

    const PublicApiComp = {} as PublicApiComponent;
    const result = extractArgumentsFromRenderSlot(
      instantiateComponent(PublicApiComp, {}),
      "default",
    );

    result.slotItem.id satisfies number;
    result.slotItem.name satisfies string;
    result.slotIndex satisfies number;
    result.slotTotal satisfies number;

    // @ts-expect-error - slot item should not widen to any
    result.slotItem.id satisfies string;
  });

  it("preserves slot props for generated public-api intersections", () => {
    type OmitNew<T> = { [K in keyof T]: T[K] };
    const BaseComp = defineComponent({});

    type GeneratedPublicApiComponent = OmitNew<typeof BaseComp> & {
      new (props?: {}): {
        $props: {};
        $slots: {
          default(props: {
            slotItem: { id: number; name: string };
            slotIndex: number;
            slotTotal: number;
          }): any;
        };
      };
    };

    const PublicApiComp = {} as GeneratedPublicApiComponent;
    const result = extractArgumentsFromRenderSlot(
      instantiateComponent(PublicApiComp, {}),
      "default",
    );

    result.slotItem.id satisfies number;
    result.slotItem.name satisfies string;
    result.slotIndex satisfies number;
    result.slotTotal satisfies number;

    // @ts-expect-error - slot item should not widen to any
    result.slotItem.id satisfies string;
  });

  // Test with generic component types
  describe("with generic components", () => {
    const GenericComp = defineComponent({
      slots: {} as SlotsType<{
        item: (props: { value: number; index: number }) => any;
      }>,
    });

    function GenericCompFn() {
      return new GenericComp();
    }

    it("extracts props from generic component slots", () => {
      const result = extractArgumentsFromRenderSlot(GenericCompFn(), "item");

      result.value satisfies number;
      result.index satisfies number;

      // @ts-expect-error - wrong prop type
      result.value satisfies string;
    });
  });

  // Test with union props
  describe("with union type props", () => {
    const UnionComp = defineComponent({
      slots: {} as SlotsType<{
        status: (props: { state: "loading" | "success" | "error" }) => any;
      }>,
    });

    function UnionCompFn() {
      return new UnionComp();
    }

    it("extracts union type props correctly", () => {
      const result = extractArgumentsFromRenderSlot(UnionCompFn(), "status");

      result.state satisfies "loading" | "success" | "error";

      // @ts-expect-error - wrong union value
      result.state satisfies "pending";
    });
  });
});
