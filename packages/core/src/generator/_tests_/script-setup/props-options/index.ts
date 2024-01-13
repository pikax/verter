import {
  defineComponent,
  DeclareComponent,
  ExtractPropTypes,
  ComponentData,
  ObjectToComponentProps,
  PropType,
} from "vue";

type __COMP__ = DeclareComponent<typeof ComponentOptions['props'],ComponentData<typeof ComponentOptions>, {}, {}, typeof ComponentOptions>; const ComponentOptions = defineComponent({
  __name: 'Comp',
  props: {
  foo: String,
  bar: {
    type: String,
    default: "bar",
  },
},
  setup(__props, { expose: __expose }) {
  __expose();

const props = __props;

const __returned__ = { props }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

});
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
