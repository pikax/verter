import {
  defineComponent,
  useSlots as _useSlots,
  DeclareComponent,
  ComponentData,
  SlotsType,
} from "vue";

type __COMP__ = DeclareComponent<{},ComponentData<typeof ComponentOptions>, {}, SlotsType<{ foo: () => any }>, typeof ComponentOptions>; const ComponentOptions = defineComponent(({
  __name: 'Comp',
  setup(__props, { expose: __expose }) {
  __expose();

const slots = _useSlots();

const __returned__ = { slots }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}));
declare const Comp: __COMP__;

expectType<{
  foo: () => any;
}>(getComponentSlots(Comp));

expectType<{
  bar: () => any;
  // @ts-expect-error not any
}>(getComponentSlots(Comp));
