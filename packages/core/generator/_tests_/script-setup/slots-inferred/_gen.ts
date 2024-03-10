type __COMP__ = {};
declare const Comp: __COMP__;

const slots = getComponentSlots(Comp);
expectType<{
  header: (o: { title: string }) => any;
  default: () => any;
  footer: (arg: { foo: any }) => any;
}>(slots);

expectType<{
  bar: () => any;
  // @ts-expect-error not any
}>(slots);
