import {
    defineComponent,
    DeclareComponent,
    ComponentData,
    DeclareEmits,
    EmitsToProps,
    ComponentProps,
    useModel as _useModel,
  } from "vue";

type __COMP__ = DeclareComponent<{test: { foo: string }} & EmitsToProps<DeclareEmits<{['update:test']: [{ foo: string }]}>>,ComponentData<typeof ComponentOptions>, DeclareEmits<{['update:test']: [{ foo: string }]}>, {}, typeof ComponentOptions>; const ComponentOptions = defineComponent(({
  __name: 'Comp',
  props: {
    "test": { type: Object },
    "testModifiers": {},
  },
  emits: ["update:test"],
  setup(__props, { expose: __expose }) {
  __expose();

const model = _useModel<{ foo: string }>(__props, "test");

const __returned__ = { model }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}));
declare const Comp: __COMP__;


expectType<{
    (event: "update:test", value: { foo: string }): void;
  }>(getComponentEmits(Comp));
  
  expectType<{
    (event: "onUpdate:test", value: number): void;
    // @ts-expect-error not any
  }>(getComponentEmits(Comp));
  
  expectType<{
    test: { foo: string };
  }>(getComponentProps(Comp));
  
  expectType<{
    onRandom?: () => void;
    // @ts-expect-error not any
  }>(getComponentProps(Comp));
  