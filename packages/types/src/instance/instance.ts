import {
  createMacroReturn,
  CreateMacroReturn,
  ExtractMacroProps,
  ExtractPropsFromMacro,
  ExtractTemplateRef,
  MacroOptionsToOptions,
  MacroToEmitValue,
  MacroToModelRecord,
  MacroToModelType,
  NormalisedMacroReturn,
  NormaliseMacroReturn,
  OmitMacroReturn,
  SlotsToSlotType,
} from "../setup";
import { MakePublicProps, MakeInternalProps } from "../props";
import { ModelToEmits, ModelToModelInfo, ModelToProps } from "../model";
import { ModelRef, useTemplateRef } from "vue";
import { EmitsToProps } from "../emits";

export type CreateTypedInternalInstanceFromNormalisedMacro<
  T extends NormalisedMacroReturn<any>,
  Attrs = {},
  DEV extends boolean = false
> = {
  // proxy: {};
  // TODO fill in internal instance properties
  data: DEV extends true ? T["$data"] : {};
  props: ExtractPropsFromMacro<T["props"]> &
    ModelToProps<MacroToModelRecord<T["model"]>>;
  attrs: Attrs;
  refs: T["templateRef"];
  emit: MacroToEmitValue<T["emits"]> &
    ModelToEmits<MacroToModelRecord<T["model"]>>;
  slots: SlotsToSlotType<T["slots"]>;
  //   // TODO add more properties, it needs name etc.
  //   options: MacroOptionsToOptions<T["options"]>;

  // exposed: {}
  // exposedProxy: {}
};

type MakeInternalInstanceFromNormalisedMacro<
  T extends NormalisedMacroReturn<any>,
  Attrs = {},
  DEV extends boolean = false
> = CreateTypedInternalInstanceFromNormalisedMacro<
  T,
  Attrs,
  DEV
> extends infer I
  ? Omit<import("vue").ComponentInternalInstance, keyof I> & I extends infer FI
    ? Omit<FI, "proxy"> & {
        proxy: PublicInstanceFromNormalisedMacro<
          T,
          Attrs,
          Element,
          false,
          DEV,
          MakeInternalInstanceFromNormalisedMacro<T, Attrs, DEV>
        >;
      }
    : I
  : never;

export type ToInstanceProps<
  T,
  MakeDefaultsOptional extends boolean
> = ExtractPropsFromMacro<T> extends infer PP extends Record<string, any>
  ? MakeDefaultsOptional extends true
    ? MakePublicProps<PP>
    : MakeInternalProps<PP>
  : T extends Record<string, any>
  ? MakeInternalProps<T>
  : {};

export type CreateTypedPublicInstanceFromNormalisedMacro<
  T extends NormalisedMacroReturn<any>,
  Attrs = {},
  MakeDefaultsOptional extends boolean = false,
  DEV extends boolean = false,
  InternalInstance = false
> = {
  $: InternalInstance extends false
    ? MakeInternalInstanceFromNormalisedMacro<T, Attrs, DEV>
    : InternalInstance;

  $data: DEV extends true ? T["$data"] : {};
  $props: ToInstanceProps<T["props"], MakeDefaultsOptional> &
    ModelToProps<MacroToModelRecord<T["model"]>> &
    EmitsToProps<MacroToEmitValue<T["emits"]>>;

  $attrs: Attrs;
  $refs: T["templateRef"];

  $emit: MacroToEmitValue<T["emits"]> &
    ModelToEmits<MacroToModelRecord<T["model"]>>;
};

export type PublicInstanceFromMacro<
  T,
  Attrs,
  El extends Element,
  MakeDefaultsOptional extends boolean,
  DEV extends boolean,
  InternalInstance extends
    | false
    | MakeInternalInstanceFromNormalisedMacro<any, any> = false
> = NormaliseMacroReturn<T> extends infer NT extends NormalisedMacroReturn<any>
  ? PublicInstanceFromNormalisedMacro<
      NT,
      Attrs,
      El,
      MakeDefaultsOptional,
      DEV,
      InternalInstance
    >
  : {};

export type PublicInstanceFromNormalisedMacro<
  T extends NormalisedMacroReturn<any>,
  Attrs,
  El extends Element,
  MakeDefaultsOptional extends boolean,
  DEV extends boolean,
  InternalInstance = false
> = Omit<
  import("vue").ComponentPublicInstance<
    {},
    {},
    {},
    {},
    {},
    {},
    {},
    {},
    MakeDefaultsOptional,
    MacroOptionsToOptions<T["options"]>,
    {},
    SlotsToSlotType<T["slots"]>,
    "",
    {},
    El
  >,
  keyof CreateTypedPublicInstanceFromNormalisedMacro<any>
> &
  CreateTypedPublicInstanceFromNormalisedMacro<
    T,
    Attrs,
    MakeDefaultsOptional,
    DEV,
    InternalInstance
  >;

const model = defineModel<{
  a: number;
}>({});
type T = PublicInstanceFromMacro<
  CreateMacroReturn<{
    props: { value: { foo: string }; type: { foo: string } };
    emits: {
      value: (e: "update", val: number) => void;
    };
    slots: { value: { default: () => string } };
    options: { value: { customOption: boolean } };
    model: { modelValue: { value: typeof model; type: typeof model } };
    expose: { value: { focus: () => void }; type: { focus: () => void } };
    withDefaults: { value: { foo: "default foo" }; type: { foo: string } };

    templateRef: {
      someRef: ReturnType<typeof useTemplateRef<HTMLDivElement>>;
    };
  }> & {
    someData: string;
  },
  {},
  Element,
  true,
  true
>;

declare const TT: T;

TT.$props.foo = undefined;
TT.$props.foo = "";
// @ts-expect-error wrong type
TT.$props.foo = 123;
TT.$emit("update", 123);
// @ts-expect-error wrong type
TT.$emit("update:modelValue", 123);

TT.$data.someData;
TT.$data.foo;
TT.$data.a;

TT.$refs.someRef.value?.focus();

TT.$slots.default()?.toLowerCase();
TT.$slots;

TT.$options.customOption;
TT.$options.beforeUpdate;

TT.$.attrs;
TT.$.uid;

TT.$.proxy.$.proxy?.$data.someData;
TT.$.proxy.$data.someData;

// very very deep access while maintaining types
TT.$.proxy.$props.foo;
TT.$.proxy.$.props.foo;
TT.$.proxy.$.proxy.$props.foo;
TT.$.proxy.$.proxy.$.props.foo;
TT.$.proxy.$.proxy.$.proxy.$props.foo;

type UU = import("vue").ComponentInstance;

const a = TT.kk;

// if(a === 'host')
