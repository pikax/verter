import {
  defineComponent,
  DeclareComponent,
  ComponentData,
  DeclareEmits,
  EmitsToProps,
  ComponentProps,
} from "vue";

type __COMP__ = DeclareComponent<EmitsToProps<DeclareEmits<{
  (event: "foo", test: string): void;
}>>,ComponentData<typeof ComponentOptions>, DeclareEmits<{
  (event: "foo", test: string): void;
}>, {}, typeof ComponentOptions>; const ComponentOptions = defineComponent(({
  __name: 'Comp',
  emits: ["foo"],
  setup(__props, { expose: __expose, emit: __emit }) {
  __expose();

const emits = __emit;

const __returned__ = { emits }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}));
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
