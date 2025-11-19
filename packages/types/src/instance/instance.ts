import {
  CreateMacroReturn,
  ExtractMacroProps,
  ExtractPropsFromMacro,
  ExtractTemplateRef,
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

export type PublicInstanceFromMacro<
  T,
  Attrs = {},
  El = Element,
  MakeDefaultsOptional extends boolean = false,
  DEV extends boolean = false
> = NormaliseMacroReturn<T> extends infer NT extends NormalisedMacroReturn<any>
  ? //import("vue").ComponentPublicInstance &
    {
      $data: DEV extends true ? OmitMacroReturn<T> : {};
      $props: ToInstanceProps<NT["props"], MakeDefaultsOptional> &
        ModelToProps<MacroToModelRecord<NT["model"]>>;

      $attrs: Attrs;
      $refs: NT["templateRef"];
    //   $slots: SlotsToSlotType<NT["slots"]>;
      $root: import("vue").ComponentPublicInstance | null;
      $parent: import("vue").ComponentPublicInstance | null;

      // TODO check this value and see if we can resolve the type better
      host: Element | null;

      $emit: MacroToEmitValue<NT["emits"]> &
        ModelToEmits<MacroToModelRecord<NT["model"]>>;

      $el: El | null;

      $option: NT["options"];

      m1: MacroToModelRecord<NT["model"]>;
      m: ModelToProps<MacroToModelRecord<NT["model"]>>;
      mm: ModelToModelInfo<NT["model"]>;
      //   $props: ExtractMacroProps<T> extends infer PP? PP : MakeInternalProps<T>;
      /* &
        ModelToProps<ToModelType<M>>;
      $emit: ToEmitValue<E> & ModelToEmits<ToModelType<M>>;
      $slots: S; */
    }
  : //   P["props"],
    //   {},
    //   {},
    //   {},
    //   {},
    //   {},
    //   {},
    //   P["defaults"],
    //   Public,
    //   O["value"],
    //   {},
    //   SlotsToSlotType<S>,
    //   ExposeToVueExposeKey<X>,
    //   // todo check what's typeRef
    //   {},
    //   //todo check typeEl
    //   any
    // > & { $emits: E["value"] }
    false;

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

TT.m1.modelValue;
TT.m.modelValue;
TT.mm.modelValue;

TT.$data.someData;

TT.$refs.someRef.value?.focus();

TT.$slots.default()?.toLowerCase();
TT.$slots

type UU = import("vue").ComponentInstance;
