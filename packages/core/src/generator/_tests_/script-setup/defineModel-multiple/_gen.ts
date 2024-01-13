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
  (event: "update:modelValue", value: boolean): void;
  (event: "update:test", value: { foo: string }): void;
}>(getComponentEmits(Comp));

expectType<{
  (event: "onUpdate:modelValue"): void;
  // @ts-expect-error not any
}>(getComponentEmits(Comp));

expectType<{
  modelValue: { foo: string };
  test?: boolean;
}>(getComponentProps(Comp));

expectType<{
  onRandom?: () => void;
  // @ts-expect-error not any
}>(getComponentProps(Comp));
