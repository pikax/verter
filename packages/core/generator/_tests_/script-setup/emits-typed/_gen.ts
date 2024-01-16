type __COMP__ = {};
declare const Comp: __COMP__;

const emits = getComponentEmits(Comp);
const props = getComponentProps(Comp);

expectType<{
  (event: "foo", test: string): void;
}>(emits);

expectType<{
  (event: "bar", test: number): void;
  // @ts-expect-error not any
}>(emits);

expectType<{
  onFoo?: (test: string) => void;
}>(props);

expectType<{
  onRandom?: () => void;
  // @ts-expect-error not any
}>(props);
