import {
  createMacroReturn,
  CreateMacroReturn,
  ExtractPropsFromMacro,
  MacroOptionsToOptions,
  MacroToEmitValue,
  MacroToModelRecord,
  NormalisedMacroReturn,
  NormaliseMacroReturn,
  SlotsToSlotType,
} from "../setup";
import { MakePublicProps, MakeInternalProps } from "../props";
import { ModelToEmits, ModelToProps } from "../model";
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

  exposed: T["expose"];
  exposedProxy: T["expose"];
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

export type InternalInstanceFromMacro<
  T,
  Attrs = {},
  DEV extends boolean = false
> = NormaliseMacroReturn<T> extends infer NT extends NormalisedMacroReturn<any>
  ? MakeInternalInstanceFromNormalisedMacro<NT, Attrs, DEV>
  : {};

export type ToInstanceProps<
  T,
  MakeDefaultsOptional extends boolean
> = ExtractPropsFromMacro<T> extends infer PP
  ? {} extends PP
    ? T extends Record<string, any>
      ? MakeInternalProps<T>
      : {}
    : PP extends Record<string, any>
    ? MakeDefaultsOptional extends true
      ? MakePublicProps<PP> & { xx: PP }
      : MakeInternalProps<PP> & { xy: PP }
    : T extends Record<string, any>
    ? MakeInternalProps<T>
    : {}
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
    EmitsToProps<MacroToEmitValue<T["emits"]>> &
    Attrs;

  $attrs: Attrs;
  $refs: T["templateRef"];
  $options: MacroOptionsToOptions<T["options"]>;

  $emit: MacroToEmitValue<T["emits"]> &
    ModelToEmits<MacroToModelRecord<T["model"]>>;
} & (T["expose"] extends { object: infer V }
  ? unknown extends V
    ? ToInstanceProps<T["props"], true> &
        ModelToProps<MacroToModelRecord<T["model"]>>
    : V
  : ToInstanceProps<T["props"], true> &
      ModelToProps<MacroToModelRecord<T["model"]>>);
export type PublicInstanceFromMacro<
  T,
  Attrs,
  El extends Element,
  MakeDefaultsOptional extends boolean,
  DEV extends boolean = false,
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
  DEV extends boolean = false,
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
    T["expose"] extends { object: infer V }
      ? unknown extends V
        ? ""
        : keyof V
      : "",
    {},
    El
  >,
  "$props" | "$emit" | "$data" | "$attrs" | "$refs" | "$"
> &
  CreateTypedPublicInstanceFromNormalisedMacro<
    T,
    Attrs,
    MakeDefaultsOptional,
    DEV,
    InternalInstance
  >;

export type CreateExportedInstanceFromNormalisedMacro<
  T extends NormalisedMacroReturn<any>,
  Attrs = {},
  El extends Element = Element,
  DEV extends boolean = false
> = PublicInstanceFromNormalisedMacro<T, Attrs, El, true, DEV> extends infer I
  ? I
  : {};

export type CreateExportedInstanceFromMacro<
  T,
  Attrs = {},
  El extends Element = Element,
  DEV extends boolean = false
> = NormaliseMacroReturn<T> extends infer NT extends NormalisedMacroReturn<any>
  ? CreateExportedInstanceFromNormalisedMacro<NT, Attrs, El, DEV>
  : {};
