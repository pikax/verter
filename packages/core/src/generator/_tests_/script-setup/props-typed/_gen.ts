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
  foo: string;
}>(getComponentProps(Comp));

expectType<{
  foo: {
    type: PropType<string>;
    required: true;
  };
}>(Comp.props);
