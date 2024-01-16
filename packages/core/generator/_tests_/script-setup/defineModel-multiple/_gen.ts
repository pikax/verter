type __COMP__ = {};
declare const Comp: __COMP__;


const emits = getComponentEmits(Comp);
const props = getComponentProps(Comp);

expectType<{
  (event: "update:modelValue", value: boolean): void;
  (event: "update:test", value: { foo: string }): void;
}>(emits);

expectType<{
  (event: "onUpdate:modelValue"): void;
  // @ts-expect-error not any
}>(emits);


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
