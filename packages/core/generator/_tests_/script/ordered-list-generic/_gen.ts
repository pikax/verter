type __COMP__ = {};
declare const Comp: __COMP__;

const data = getComponentData(Comp);

expectType<{
  getItemAtIndex: (index: number) => unknown;
}>(data);

// check types

interface Item {
  foo: "foo";
  bar: {
    a: 1;
  };
}

declare const CompItem: __COMP__<Item>;

const propsItem = getComponentProps(CompItem);
const dataItem = getComponentData(CompItem);

expectType<"foo">(new Comp<Item>().$props.items[0].foo);

expectType<"foo">(dataItem.getItemAtIndex(0)!.foo);

expectType<{
  getItemAtIndex: (index: number) => Item | undefined;
}>(dataItem);

expectType<{
  items: Item[];
  getKey: (item: Item) => string | number;
  getLabel: (item: Item) => string;
}>(propsItem);

expectType<{
  items: number[];
  // @ts-expect-error should have correct type
}>(propsItem);
