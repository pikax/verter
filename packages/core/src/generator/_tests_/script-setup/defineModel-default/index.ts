import {
  defineComponent,
  DeclareComponent,
  ComponentData,
  DeclareEmits,
  EmitsToProps,
  ComponentProps,
  useModel as _useModel,
} from "vue";

type __COMP__ = DeclareComponent<{modelValue: { foo: string }} & EmitsToProps<DeclareEmits<{['update:modelValue']: [{ foo: string }]}>>,ComponentData<typeof ComponentOptions>, DeclareEmits<{['update:modelValue']: [{ foo: string }]}>, {}, typeof ComponentOptions>; const ComponentOptions = defineComponent(({
  __name: 'Comp',
  props: {
    "modelValue": { type: Object },
    "modelModifiers": {},
  },
  emits: ["update:modelValue"],
  setup(__props, { expose: __expose }) {
  __expose();

const model = _useModel<{ foo: string }>(__props, "modelValue");

const __returned__ = { model }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}));
declare const Comp: __COMP__;

expectType<{
  (event: "update:modelValue", value: { foo: string }): void;
}>(getComponentEmits(Comp));

expectType<{
  (event: "onUpdate:modelValue", value: number): void;
  // @ts-expect-error not any
}>(getComponentEmits(Comp));

expectType<{
  modelValue: { foo: string };
}>(getComponentProps(Comp));

expectType<{
  onRandom?: () => void;
  // @ts-expect-error not any
}>(getComponentProps(Comp));
