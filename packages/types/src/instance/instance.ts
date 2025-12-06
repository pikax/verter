import {
  createMacroReturn,
  CreateMacroReturn,
  ExtractPropsFromMacro,
  MacroOptionsToOptions,
  MacroToEmitValue,
  MacroToModelRecord,
  NormalisedMacroReturn,
  NormaliseMacroReturn,
  Prettify,
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
  AttrsProps extends boolean = false,
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
          AttrsProps,
          DEV,
          MakeInternalInstanceFromNormalisedMacro<T, Attrs, AttrsProps, DEV>
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
  ? // Check if PP has no keys (truly empty) rather than {} extends PP
    // which would be true for all-optional props like { message?: string }
    keyof PP extends never
    ? {}
    : PP extends Record<string, any>
    ? MakeDefaultsOptional extends true
      ? MakePublicProps<PP>
      : MakeInternalProps<PP>
    : {}
  : {};

export type CreateTypedPublicInstanceFromNormalisedMacro<
  T extends NormalisedMacroReturn<any>,
  Attrs = {},
  AttrsProps extends boolean = false,
  MakeDefaultsOptional extends boolean = false,
  DEV extends boolean = false,
  InternalInstance = false
> = {
  $: InternalInstance extends false
    ? MakeInternalInstanceFromNormalisedMacro<T, Attrs, DEV>
    : InternalInstance;

  $data: DEV extends true ? T["$data"] : {};
  $props: Prettify<
    ToInstanceProps<T["props"], MakeDefaultsOptional> &
      ModelToProps<MacroToModelRecord<T["model"]>> &
      EmitsToProps<MacroToEmitValue<T["emits"]>>
  > &
    (AttrsProps extends true ? Attrs : {});

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
  AttrsProps extends boolean = false,
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
      AttrsProps,
      DEV,
      InternalInstance
    >
  : {};

export type PublicInstanceFromNormalisedMacro<
  T extends NormalisedMacroReturn<any>,
  Attrs,
  El extends Element,
  MakeDefaultsOptional extends boolean,
  AttrsProps extends boolean = false,
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
    AttrsProps,
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

/**
 * Creates a public instance type for use within the component's SFC template.
 *
 * This type is used internally when generating template bindings. It differs from
 * the external public instance because:
 * - `MakeDefaultsOptional` is `false`: Props with defaults are required (always defined internally)
 * - `AttrsProps` is `false`: Attrs are NOT included in `$props` (attrs accessed separately via `$attrs`)
 *
 * Use this type when you need the instance type as seen from inside the component template,
 * where all props (including those with defaults) are guaranteed to be present.
 *
 * @template T - The macro return type from `createMacroReturn`
 * @template Attrs - The component's attrs type (fallthrough attributes)
 * @template El - The root element type (defaults to Element)
 *
 * @example
 * ```ts
 * const setup = () => createMacroReturn({
 *   props: { value: { count: number }, type: { count: number } }
 * });
 * type Instance = SFCPublicInstanceFromMacro<ReturnType<typeof setup>, {}, HTMLDivElement>;
 * // Instance.$props.count is number (not optional)
 * // Instance.$attrs does NOT appear in Instance.$props
 * ```
 */
export type SFCPublicInstanceFromMacro<
  T,
  Attrs,
  El extends Element
> = PublicInstanceFromMacro<T, Attrs, El, false, false>;

/**
 * Creates a public instance type for external/consumer usage of the component.
 *
 * This type is used when consuming a component from outside (e.g., in parent templates
 * or when using `ref` on a component). It differs from the internal SFC instance because:
 * - `MakeDefaultsOptional` is `true`: Props with defaults are optional (can be omitted by consumer)
 * - `AttrsProps` is `false`: Attrs are NOT included in `$props`
 *
 * Use this type when you need the instance type as seen by component consumers,
 * where props with defaults can be omitted.
 *
 * @template T - The macro return type from `createMacroReturn`
 * @template Attrs - The component's attrs type (fallthrough attributes)
 * @template El - The root element type (defaults to Element)
 *
 * @example
 * ```ts
 * const setup = () => createMacroReturn({
 *   props: { value: { title: string }, type: { title: string } },
 *   withDefaults: { value: { title: 'Default' }, type: { title: string } }
 * });
 * type Instance = ExternalPublicInstanceFromMacro<ReturnType<typeof setup>, {}, HTMLDivElement>;
 * // Instance.$props.title is string | undefined (optional for consumers)
 * ```
 */
export type ExternalPublicInstanceFromMacro<
  T,
  Attrs,
  El extends Element
> = PublicInstanceFromMacro<T, Attrs, El, true, false>;

/**
 * Creates a public instance type for testing purposes with attrs included in props.
 *
 * This type is specifically designed for use in Vitest tests where you need to verify
 * that attrs are properly merged into `$props`. It includes:
 * - `MakeDefaultsOptional` is `true`: Props with defaults are optional
 * - `AttrsProps` is `true`: Attrs ARE included in `$props` (for testing attr merging)
 *
 * **Note:** This type should only be used in test files. For production code, use
 * `SFCPublicInstanceFromMacro` or `ExternalPublicInstanceFromMacro` instead.
 *
 * @template T - The macro return type from `createMacroReturn`
 * @template Attrs - The component's attrs type (fallthrough attributes)
 * @template El - The root element type (defaults to Element)
 *
 * @example
 * ```ts
 * // In a test file:
 * type CustomAttrs = { class?: string; 'data-testid'?: string };
 * const setup = () => createMacroReturn({
 *   props: { value: { id: number }, type: { id: number } }
 * });
 * type Instance = TestExternalPublicInstanceFromMacro<ReturnType<typeof setup>, CustomAttrs, HTMLDivElement>;
 * // Instance.$props includes both { id?: number } AND { class?: string; 'data-testid'?: string }
 * ```
 */
export type TestExternalPublicInstanceFromMacro<
  T,
  Attrs,
  El extends Element
> = PublicInstanceFromMacro<T, Attrs, El, true, true>;
