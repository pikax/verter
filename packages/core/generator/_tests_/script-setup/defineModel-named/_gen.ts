type __COMP__ = {};
declare const Comp: __COMP__;

const emits = getComponentEmits(Comp);
const props = getComponentProps(Comp);

expectType<{
  (event: "update:test", value: { foo: string }): void;
}>(emits);

expectType<{
  (event: "onUpdate:test", value: number): void;
  // @ts-expect-error not any
}>(emits);

expectType<{
  test: { foo: string };
}>(props);

expectType<{
  onRandom?: () => void;
  // @ts-expect-error not any
}>(props);
