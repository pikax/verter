import "vue/jsx";
import { assertType, describe, it } from "vitest";
import { defineComponent, SlotsType } from "vue";
import { StrictRenderSlot } from "./slots";

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

      tuple: () => [typeof TabItem, typeof TabItem];

      string: () => string;
      stringMixed: () => string | typeof TabItem;

      literal: () => "foo";
      literalMixed: () => "foo" | "bar";
      literalMultiple: () => ("foo" | "bar")[];
      literalTuple: () => ["foo", typeof TabItem];

      spanElement: () => HTMLSpanElement;
    }>,
  });

  const UnpatchedTabItems = defineComponent({
    props: {
      catsAreTheBest: Boolean,
    },
  });

  const c = new Tab();

  it("accepts array children for array-returning slot", () => {
    StrictRenderSlot(c.$slots.default, [TabItem, TabItem]);
    StrictRenderSlot(c.$slots.default, []);
    StrictRenderSlot(c.$slots.default, [TabItem]);
    // @ts-expect-error wrong child type
    StrictRenderSlot(c.$slots.default, [UnpatchedTabItems]);
    // additional sanity: mixing non-matching type still errors
    // @ts-expect-error wrong child type in array
    StrictRenderSlot(c.$slots.default, [TabItem, UnpatchedTabItems]);
  });

  it("requires single-element tuple for single-returning slot", () => {
    StrictRenderSlot(c.$slots.single, [TabItem]);
    // @ts-expect-error multiple children not allowed for single slot
    StrictRenderSlot(c.$slots.single, [TabItem, TabItem]);
    // @ts-expect-error wrong child type
    StrictRenderSlot(c.$slots.single, [UnpatchedTabItems]);
  });
  it("supports tuple-returning slot", () => {
    StrictRenderSlot(c.$slots.tuple, [TabItem, TabItem]);
    // @ts-expect-error wrong number of children
    StrictRenderSlot(c.$slots.tuple, [TabItem]);
    // @ts-expect-error wrong child type
    StrictRenderSlot(c.$slots.tuple, [TabItem, UnpatchedTabItems]);
  });

  it("support for string", () => {
    StrictRenderSlot(c.$slots.string, ["just a string"]);
    // @ts-expect-error multiple strings allowed
    StrictRenderSlot(c.$slots.string, ["one", "two"]);
    // @ts-expect-error wrong type
    StrictRenderSlot(c.$slots.string, [TabItem]);
  });
  it("support for string | TabItem", () => {
    StrictRenderSlot(c.$slots.stringMixed, ["just a string"]);
    StrictRenderSlot(c.$slots.stringMixed, [TabItem]);
    // @ts-expect-error wrong type
    StrictRenderSlot(c.$slots.stringMixed, [UnpatchedTabItems]);
  });
  it("support for named slot returning literal", () => {
    StrictRenderSlot(c.$slots.literal, ["foo"]);
    // @ts-expect-error wrong literal
    StrictRenderSlot(c.$slots.literal, ["bar"]);
    // @ts-expect-error wrong type
    StrictRenderSlot(c.$slots.literal, [TabItem]);
    // @ts-expect-error wrong number of literals
    StrictRenderSlot(c.$slots.literal, ["foo", "foo"]);
  });

  it("support for named slot returning literal union", () => {
    StrictRenderSlot(c.$slots.literalMixed, ["foo"]);
    StrictRenderSlot(c.$slots.literalMixed, ["bar"]);

    // @ts-expect-error wrong literal
    StrictRenderSlot(c.$slots.literalMixed, ["baz"]);

    // @ts-expect-error wrong type
    StrictRenderSlot(c.$slots.literalMixed, [TabItem]);
  });

  it("support for named slot returning literal union array", () => {
    StrictRenderSlot(c.$slots.literalMultiple, ["foo", "bar", "foo"]);
    StrictRenderSlot(c.$slots.literalMultiple, []);
    StrictRenderSlot(c.$slots.literalMultiple, ["bar"]);
    // @ts-expect-error wrong literal
    StrictRenderSlot(c.$slots.literalMultiple, ["baz"]);
    // @ts-expect-error wrong type
    StrictRenderSlot(c.$slots.literalMultiple, [TabItem]);
  });

  it("support for named slot returning tuple of literal and component", () => {
    StrictRenderSlot(c.$slots.literalTuple, ["foo", TabItem]);
    // @ts-expect-error wrong literal
    StrictRenderSlot(c.$slots.literalTuple, ["bar", TabItem]);
    // @ts-expect-error wrong type
    StrictRenderSlot(c.$slots.literalTuple, ["foo", UnpatchedTabItems]);
  });

  it("support for named slot returning HTMLSpanElement", () => {
    StrictRenderSlot(c.$slots.spanElement, [
      document.createElement("span") as HTMLSpanElement,
    ]);

    // @ts-expect-error wrong type
    StrictRenderSlot(c.$slots.spanElement, [
      document.createElement("div") as HTMLDivElement,
    ]);
  });
});
