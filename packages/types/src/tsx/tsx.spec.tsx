import { assertType, describe, expect, it } from "vitest";
import { defineComponent, SlotsType } from "vue";
import "./tsx";

describe("TSX type augmentations", () => {
  describe("v-slot on HTML elements", () => {
    it("supports v-slot on div elements", () => {
      <div
        v-slot={(el) => {
          assertType<HTMLElement>(el);
        }}
      />;
    });

    it("supports v-slot on input elements", () => {
      <input
        v-slot={(el) => {
          assertType<HTMLInputElement>(el);
          assertType<string>(el.value);
        }}
      />;
    });

    it("supports v-slot on select elements", () => {
      // HTMLSelectElement instance through HTMLAttributes
      <select
        v-slot={(el) => {
          assertType<HTMLSelectElement>(el);
          assertType<number>(el.selectedIndex);
        }}
      />;
    });
  });

  describe("v-slot on Vue components", () => {
    const TabItem = defineComponent({
      props: {
        id: { type: String, required: true },
      },
    });

    const Tabs = defineComponent({
      slots: {} as SlotsType<{
        default: (arg: { foo: 1 }) => (typeof TabItem)[];
        header: () => string;
      }>,
      setup() {
        return { tabsId: "tabs-123" };
      },
    });

    type TabsInstance = InstanceType<typeof Tabs>;

    it("infers component instance type from v-slot function parameter", () => {
      <Tabs
        v-slot={(c) => {
          assertType<TabsInstance>(c);
          assertType<string>(c.tabsId);

          c.$slots.default({ foo: 1 });
          // @ts-expect-error missing property
          c.$slots.default();

          c.$slots.header();
          // @ts-expect-error wrong arg
          c.$slots.header({ foo: 1 });

          return {} as any;
        }}
      />;
    });
  });
});
