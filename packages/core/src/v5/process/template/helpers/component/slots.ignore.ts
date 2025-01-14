import { defineComponent, SlotsType } from "vue";
import { $V_StrictRenderSlot } from "./slots.js";

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
$V_StrictRenderSlot(new Comp().$slots.default, [null]);

// foo
$V_StrictRenderSlot(new Comp().$slots.foo, [{ foo: "foo" }]);

// single
$V_StrictRenderSlot(new Comp().$slots.single, [new HTMLInputElement()]);

// @ts-expect-error
$V_StrictRenderSlot(new Comp().$slots.single, [
  new HTMLInputElement(),
  new HTMLInputElement(),
]);

// multiple
$V_StrictRenderSlot(new Comp().$slots.multiple, [new HTMLInputElement()]);

// @ts-expect-error
$V_StrictRenderSlot(new Comp().$slots.multiple, [new HTMLElement()]);

// two
$V_StrictRenderSlot(new Comp().$slots.two, [
  new HTMLMediaElement(),
  new HTMLAnchorElement(),
]);

// @ts-expect-error
$V_StrictRenderSlot(new Comp().$slots.two, [
  new HTMLMediaElement(),
  new HTMLAnchorElement(),
  new HTMLAnchorElement(),
]);

$V_StrictRenderSlot(new Comp().$slots.two, [
  new HTMLMediaElement(),
  // @ts-expect-error
  new HTMLMediaElement(),
]);
