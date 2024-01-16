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

expectType<{
  getItemAtIndex: (index: number) => Item;
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
