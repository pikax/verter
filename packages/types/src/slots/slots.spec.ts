/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for slot type helpers including:
 * - StrictRenderSlot: type-safe slot rendering with required props
 * - Validates slot prop typing and VNode return types
 */
import "vue/jsx";
import { assertType, describe, it } from "vitest";
import { defineComponent, SlotsType } from "vue";
import { strictRenderSlot } from "./slots";

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
    strictRenderSlot(c.$slots.spanElement, [
      document.createElement("span") as HTMLSpanElement,
    ]);

    // @ts-expect-error wrong type
    strictRenderSlot(c.$slots.spanElement, [
      document.createElement("div") as HTMLDivElement,
    ]);
  });

  it("support for named slot returning HTMLSpanElement[]", () => {
    strictRenderSlot(c.$slots.spanElements, [
      document.createElement("span") as HTMLSpanElement,
    ]);

    // @ts-expect-error wrong type
    strictRenderSlot(c.$slots.spanElements, [
      document.createElement("div") as HTMLDivElement,
    ]);
  });
});
