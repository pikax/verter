type __COMP__ = {};
declare const Comp: __COMP__;


const slots = getComponentSlots(Comp);
expectType<{
  foo: () => any;
}>(slots);

expectType<{
  bar: () => any;
  // @ts-expect-error not any
}>(slots);
