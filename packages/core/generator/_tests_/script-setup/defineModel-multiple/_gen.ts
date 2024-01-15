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

const props = getComponentProps(Comp);

props.modelValue;
props.test;

// @ts-expect-error does not exist
props.randomProp;

expectType<{
  modelValue: boolean;
  test?: { foo: string };
  "onUpdate:modelValue"?: (v: boolean) => void;
  "onUpdate:test"?: (v: { foo: string }) => void;
}>(props);

expectType<{
  onRandom?: () => void;
  // @ts-expect-error not any
}>(props);
