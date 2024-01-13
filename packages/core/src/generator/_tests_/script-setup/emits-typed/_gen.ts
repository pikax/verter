import {
  defineComponent,
  DeclareComponent,
  ComponentData,
  DeclareEmits,
  EmitsToProps,
  ComponentProps,
} from "vue";

type __COMP__ = {};
declare const Comp: __COMP__;

expectType<{
  (event: "foo", test: string): void;
}>(getComponentEmits(Comp));

expectType<{
  (event: "bar", test: number): void;
  // @ts-expect-error not any
}>(getComponentEmits(Comp));

expectType<{
  onFoo?: (test: string) => void;
}>(getComponentProps(Comp));

expectType<{
  onRandom?: () => void;
  // @ts-expect-error not any
}>(getComponentProps(Comp));
