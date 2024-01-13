import {
  defineComponent,
  DeclareComponent,
  ComponentData,
  DeclareEmits,
  EmitsToProps,
  ComponentProps,
  useModel as _useModel,
} from "vue";

type __COMP__ = {};
declare const Comp: __COMP__;

expectType<{
  (event: "update:modelValue", value: { foo: string }): void;
}>(getComponentEmits(Comp));

expectType<{
  (event: "onUpdate:modelValue", value: number): void;
  // @ts-expect-error not any
}>(getComponentEmits(Comp));

expectType<{
  modelValue: { foo: string };
}>(getComponentProps(Comp));

expectType<{
  onRandom?: () => void;
  // @ts-expect-error not any
}>(getComponentProps(Comp));
