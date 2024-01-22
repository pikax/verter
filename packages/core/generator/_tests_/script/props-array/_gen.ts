type __COMP__ = {};
declare const Comp: __COMP__;

const Render = new Comp();

Render.$props.foo;
Render.$props.bar;

// @ts-expect-error not defined
Render.$props.random;

const props = getComponentProps(Comp);

expectType<{
  foo?: any;
  bar?: any;
}>(props);

// @ts-expect-error doesn't exist
props.random

expectType<Array<"foo" | "bar">>(Comp.props);

// @ts-expect-error doesn't exist
expectType<Array<'somthing-else'>>(Comp.props);
