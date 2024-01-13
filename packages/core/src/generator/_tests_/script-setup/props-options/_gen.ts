import {
  defineComponent,
  DeclareComponent,
  ExtractPropTypes,
  ComponentData,
  ObjectToComponentProps,
  PropType,
} from "vue";

type __COMP__ = {};
declare const Comp: __COMP__;

expectType<{
  foo?: string;
  bar?: string;
}>(getComponentProps(Comp));

expectType<{
  foo: StringConstructor;
  bar: {
    type: StringConstructor;
    default: string;
  };
}>(Comp.props);
