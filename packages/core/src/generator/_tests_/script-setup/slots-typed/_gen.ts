import {
  defineComponent,
  useSlots as _useSlots,
  DeclareComponent,
  ComponentData,
  SlotsType,
} from "vue";

type __COMP__ = {};
declare const Comp: __COMP__;

expectType<{
  foo: () => any;
}>(getComponentSlots(Comp));

expectType<{
  bar: () => any;
  // @ts-expect-error not any
}>(getComponentSlots(Comp));
