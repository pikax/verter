import {
  defineComponent,
  DeclareComponent,
  ComponentData,
  DeclareEmits,
  EmitsToProps,
  ComponentProps,
  useModel as _useModel,
} from "vue";

type __COMP__ = DeclareComponent<{test: { foo: string }

  default: false,
: any} & EmitsToProps<DeclareEmits<{['update:test']: [{ foo: string }]
['update:
  default: false,
']: [any]}>>,ComponentData<typeof ComponentOptions>, DeclareEmits<{['update:test']: [{ foo: string }]
['update:
  default: false,
']: [any]}>, {}, typeof ComponentOptions>; const ComponentOptions = defineComponent(({
  __name: 'Comp',
  props: {
    "test": { type: Object },
    "testModifiers": {},
    "modelValue": {
  default: false,
},
    "modelModifiers": {},
  },
  emits: ["update:test", "update:modelValue"],
  setup(__props, { expose: __expose }) {
  __expose();

const testModel = _useModel<{ foo: string }>(__props, "test");
const model = _useModel(__props, "modelValue");

const __returned__ = { testModel, model }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}));
declare const Comp: __COMP__;

expectType<{
  (event: "update:modelValue", value: boolean): void;
  (event: "update:test", value: { foo: string }): void;
}>(getComponentEmits(Comp));

expectType<{
  (event: "onUpdate:modelValue"): void;
  // @ts-expect-error not any
}>(getComponentEmits(Comp));

expectType<{
  modelValue: { foo: string };
  test?: boolean;
}>(getComponentProps(Comp));

expectType<{
  onRandom?: () => void;
  // @ts-expect-error not any
}>(getComponentProps(Comp));
