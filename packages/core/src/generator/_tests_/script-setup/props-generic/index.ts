import {
  defineComponent,
  DeclareComponent,
  ExtractPropTypes,
  ComponentData,
  ObjectToComponentProps,
  PropType,
} from "vue";

type __COMP__ = DeclareComponent<{ new<T extends string>(): { $props: { foo: T }, $emit:  , $children: {}  } },ComponentData<typeof ComponentOptions>, {}, {}, typeof ComponentOptions>; const ComponentOptions = defineComponent(({
  __name: 'Comp',
  props: {
    foo: { type: null, required: true }
  },
  setup(__props: any, { expose: __expose }) {
  __expose();



const __returned__ = {  }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}));
declare const Comp: __COMP__;
