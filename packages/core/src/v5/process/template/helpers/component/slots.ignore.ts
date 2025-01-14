import { defineComponent, SlotsType } from "vue";
import { StrictRenderSlot } from "./slots.js";

const Comp = defineComponent({
  props: {
    foo: String,
    type: String,
  },

  slots: Object as SlotsType<{
    default: () => any[];
    foo: (props: { foo: string }) => any[];
    single: (props: { type: string }) => HTMLInputElement;
    multiple: (props: { type: string }) => HTMLInputElement[];

    two: (props: { type: string }) => [HTMLMediaElement, HTMLAnchorElement];
  }>,
});

// any
StrictRenderSlot(new Comp().$slots.default, [null]);

// foo
StrictRenderSlot(new Comp().$slots.foo, [{ foo: "foo" }]);

// single
StrictRenderSlot(new Comp().$slots.single, [new HTMLInputElement()]);

// @ts-expect-error
StrictRenderSlot(new Comp().$slots.single, [
  new HTMLInputElement(),
  new HTMLInputElement(),
]);

// multiple
StrictRenderSlot(new Comp().$slots.multiple, [new HTMLInputElement()]);

// @ts-expect-error
StrictRenderSlot(new Comp().$slots.multiple, [new HTMLElement()]);

// two
StrictRenderSlot(new Comp().$slots.two, [
  new HTMLMediaElement(),
  new HTMLAnchorElement(),
]);

// @ts-expect-error
StrictRenderSlot(new Comp().$slots.two, [
  new HTMLMediaElement(),
  new HTMLAnchorElement(),
  new HTMLAnchorElement(),
]);

StrictRenderSlot(new Comp().$slots.two, [
  new HTMLMediaElement(),
  // @ts-expect-error
  new HTMLMediaElement(),
]);
