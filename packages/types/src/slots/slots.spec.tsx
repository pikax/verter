import "vue/jsx";
import { describe, it } from "vitest";
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
    // @ts-expect-error wrong child shape mixed in array
    StrictRenderSlot(c.$slots.default, [TabItem, UnpatchedTabItems]);
    // @ts-expect-error wrong child type
    StrictRenderSlot(c.$slots.default, [TabItem, HTMLElement]);
  });

  it("requires single-element tuple for single-returning slot", () => {
    StrictRenderSlot(c.$slots.single, [TabItem]);
    // @ts-expect-error multiple children not allowed for single slot
    StrictRenderSlot(c.$slots.single, [TabItem, TabItem]);
    // @ts-expect-error wrong child type
    StrictRenderSlot(c.$slots.single, [UnpatchedTabItems]);
  });
});
