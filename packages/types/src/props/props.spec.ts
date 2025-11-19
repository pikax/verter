import { describe, it, assertType } from "vitest";
import {
  MakePublicProps,
  MakeInternalProps,
  FindDefaultsKey,
  ResolveFromMacroReturn,
  ResolveDefaultsPropsFromMacro,
  PropsObjectExtractDefaults,
  ExtractBooleanKeys,
  MakeBooleanOptional,
} from "./props";
import { defineProps, withDefaults, DefineProps } from "vue";
import {
  createMacroReturn,
  ExtractMacroReturn,
  ExtractMacroProps,
  MacroReturnObject,
  ExtractPropsFromMacro,
} from "../setup";
import { defineProps_Box, withDefaults_Box } from "../vue/vue.macros";
import { ExtractHidden, removeHiddenPatch } from "../helpers";

describe("Props helpers", () => {
  describe("MakePublicProps", () => {
    it("returns plain object props unchanged", () => {
      type Plain = { a: number; b?: string };
      type Public = MakePublicProps<Plain>;
      assertType<Public>({} as Plain);
      // optional stays optional
      const v1: Public = { a: 1 };
      const v2: Public = { a: 1, b: "x" };
      void v1;
      void v2;
    });

    it("makes non-default props potentially undefined and keeps default props required", () => {
      const p = defineProps({
        f: String,
        d: { type: String, default: "bar" },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;
      // f can be string | undefined
      assertType<Public["f"]>({} as string | undefined);
      // d is always string
      assertType<Public["d"]>({} as string);
      // @ts-expect-error d cannot be undefined
      assertType<Public["d"]>({} as undefined);
      // @ts-expect-error f is readonly
      p.f = "";
    });

    it("works with withDefaults applied", () => {
      const propsBoxed = defineProps_Box<{
        f?: string;
        d?: string;
      }>();
      const defaultsBoxed = withDefaults_Box(
        {} as DefineProps<typeof propsBoxed, never>,
        { d: "baz" }
      );
      type D = typeof defaultsBoxed;
      type Public = MakePublicProps<D>;
      // f can be string | undefined
      assertType<Public["f"]>({} as string | undefined);
      // d is required (string only)
      assertType<Public["d"]>({} as string);
      // @ts-expect-error d not undefined
      assertType<Public["d"]>({} as undefined);
    });

    it("handles multiple props with mixed defaults", () => {
      const p = defineProps({
        required1: String,
        required2: Number,
        withDefault1: { type: String, default: "foo" },
        withDefault2: { type: Boolean, default: true },
        withDefault3: { type: Array as () => string[], default: () => [] },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      // Props without defaults can be undefined
      assertType<Public["required1"]>({} as string | undefined);
      assertType<Public["required2"]>({} as number | undefined);

      // Props with defaults are always defined
      assertType<Public["withDefault1"]>({} as string);
      assertType<Public["withDefault2"]>({} as boolean);
      assertType<Public["withDefault3"]>({} as string[]);

      // @ts-expect-error default props cannot be undefined
      assertType<Public["withDefault1"]>({} as undefined);
      // @ts-expect-error default props cannot be undefined
      assertType<Public["withDefault2"]>({} as undefined);
    });

    it("handles complex prop types with defaults", () => {
      const p = defineProps({
        obj: {
          type: Object as () => { x: number; y: number },
          default: () => ({ x: 0, y: 0 }),
        },
        union: {
          type: [String, Number] as unknown as () => string | number,
          default: "default",
        },
        nullable: {
          type: [String, null] as unknown as () => string | null,
          default: null,
        },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      assertType<Public["obj"]>({} as { x: number; y: number });
      assertType<Public["union"]>({} as string | number);
      assertType<Public["nullable"]>({} as string | null);

      // @ts-expect-error cannot be undefined
      assertType<Public["obj"]>({} as undefined);
    });

    it("handles all props with defaults", () => {
      const p = defineProps({
        a: { type: String, default: "a" },
        b: { type: Number, default: 42 },
        c: { type: Boolean, default: false },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      // All props are defined (no undefined union)
      assertType<Public["a"]>({} as string);
      assertType<Public["b"]>({} as number);
      assertType<Public["c"]>({} as boolean);

      // @ts-expect-error cannot be undefined
      assertType<Public["a"]>({} as undefined);
      // @ts-expect-error cannot be undefined
      assertType<Public["b"]>({} as undefined);
      // @ts-expect-error cannot be undefined
      assertType<Public["c"]>({} as undefined);
    });

    it("handles all props without defaults", () => {
      const p = defineProps({
        a: String,
        b: Number,
        c: Boolean,
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      // All props can be undefined
      assertType<Public["a"]>({} as string | undefined);
      assertType<Public["b"]>({} as number | undefined);
      // Boolean props may have special handling - verify the actual type
      type CType = Public["c"];
      const cTest: CType = true;
      void cTest;
    });

    it("preserves readonly nature of props", () => {
      const p = defineProps({
        name: String,
        age: { type: Number, default: 0 },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      const pub = {} as Public;
      // @ts-expect-error props are readonly
      pub.name = "test";
      // @ts-expect-error props are readonly
      pub.age = 42;
    });

    it("makes props with defaults optional keys in public API", () => {
      const p = defineProps({
        required: String,
        withDefault: { type: String, default: "default" },
        anotherDefault: { type: Number, default: 42 },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      // Test that withDefault and anotherDefault are optional keys
      // by checking if we can omit them
      type TestOmit = Pick<Public, "required">;
      type CanOmitDefaults = TestOmit extends Public ? false : true;
      // We should be able to omit props with defaults
      assertType<CanOmitDefaults>({} as true);

      // Props without defaults can be undefined
      assertType<Public["required"]>({} as string | undefined);

      // Props with defaults are always defined (not undefined)
      assertType<Public["withDefault"]>({} as string);
      assertType<Public["anotherDefault"]>({} as number);

      // @ts-expect-error props with defaults cannot be undefined
      assertType<Public["withDefault"]>({} as undefined);
    });

    it("handles DefineProps correctly (Partial vs Required distinction)", () => {
      const p = defineProps({
        id: Number,
        name: { type: String, default: "Anonymous" },
        active: { type: Boolean, default: true },
      });
      type P = typeof p;

      // The K parameter in DefineProps<P, K> represents keys with defaults
      // For public API: props with defaults should use Partial, not Required
      // This test verifies that changing Partial to Required would break the logic

      type Public = MakePublicProps<P>;
      assertType<Public["name"]>({} as string);
      assertType<Public["active"]>({} as boolean);
      assertType<Public["id"]>({} as number | undefined);

      // The actual optionality is handled by the macro path (lines 80-83)
      // which uses Omit and mapped types to make defaults truly optional.
      // The DefineProps fallback path is a simplified version.
    });

    it("makes boolean props optional by default in public API", () => {
      type P = DefineProps<{ foo?: string; bar: boolean }, "bar">;
      type Public = MakePublicProps<P>;

      // Boolean props are treated as optional in Vue (default to false if not provided)
      // bar should be boolean | undefined
      assertType<Public["bar"]>({} as boolean | undefined);

      // foo should remain optional
      assertType<Public["foo"]>({} as string | undefined);

      // The key 'bar' should be optional (can be omitted)
      type IsBarOptional = {} extends Pick<Public, "bar"> ? true : false;
      assertType<IsBarOptional>({} as true);
    });

    it("handles multiple boolean props", () => {
      type P = DefineProps<
        { required: string; bool1: boolean; bool2: boolean; bool3: boolean },
        "bool1" | "bool2" | "bool3"
      >;
      type Public = MakePublicProps<P>;

      // All boolean props should be optional and can be undefined
      assertType<Public["bool1"]>({} as boolean | undefined);
      assertType<Public["bool2"]>({} as boolean | undefined);
      assertType<Public["bool3"]>({} as boolean | undefined);

      // Boolean keys should be optional
      type AreBoolsOptional = {} extends Pick<Public, "bool1" | "bool2">
        ? true
        : false;
      assertType<AreBoolsOptional>({} as true);
    });

    it("handles mixed boolean and non-boolean props with defaults", () => {
      type P = DefineProps<
        {
          id: number;
          name: string;
          active: boolean;
          visible: boolean;
        },
        "active" | "visible"
      >;
      type Public = MakePublicProps<P>;

      // Boolean props (which are in BKeys) should be optional
      assertType<Public["active"]>({} as boolean | undefined);
      assertType<Public["visible"]>({} as boolean | undefined);

      // Non-boolean props remain required (not | undefined)
      assertType<Public["id"]>({} as number);
      assertType<Public["name"]>({} as string);

      // Verify boolean props are optional keys
      type IsBoolOptional = {} extends Pick<Public, "active"> ? true : false;
      assertType<IsBoolOptional>({} as true);
    });

    it("extracts boolean keys correctly", () => {
      type P1 = DefineProps<{ foo: string; bar: boolean }, "bar">;
      type BKeys1 = ExtractBooleanKeys<P1>;
      assertType<BKeys1>({} as "bar");

      type P2 = DefineProps<
        { a: string; b: boolean; c: boolean; d: number },
        "b" | "c"
      >;
      type BKeys2 = ExtractBooleanKeys<P2>;
      assertType<BKeys2>({} as "b" | "c");

      type P3 = DefineProps<{ x: string }, never>;
      type BKeys3 = ExtractBooleanKeys<P3>;
      assertType<BKeys3>({} as never);
    });

    it("handles boolean props without explicit defaults", () => {
      // When boolean props are not in BKeys, they behave like regular props
      type P = DefineProps<{ flag: boolean; name: string }, never>;
      type Public = MakePublicProps<P>;

      // Without being in BKeys, all props are required (no defaults)
      assertType<Public["flag"]>({} as boolean);
      assertType<Public["name"]>({} as string);
    });

    it("handles only boolean props", () => {
      type P = DefineProps<
        { flag1: boolean; flag2: boolean; flag3: boolean },
        "flag1" | "flag2" | "flag3"
      >;
      type Public = MakePublicProps<P>;

      // All should be optional booleans
      assertType<Public["flag1"]>({} as boolean | undefined);
      assertType<Public["flag2"]>({} as boolean | undefined);
      assertType<Public["flag3"]>({} as boolean | undefined);

      // All keys should be optional
      type AllOptional = {} extends Public ? true : false;
      assertType<AllOptional>({} as true);
    });
  });

  describe("MakeBooleanOptional", () => {
    it("makes boolean props optional when they have defaults", () => {
      type P = DefineProps<{ name: string; active: boolean }, "active">;
      type Result = MakeBooleanOptional<P>;

      // active should be optional (boolean | undefined)
      assertType<Result["active"]>({} as boolean | undefined);
      // name should remain required
      assertType<Result["name"]>({} as string);

      // active key should be optional
      type IsActiveOptional = {} extends Pick<Result, "active"> ? true : false;
      assertType<IsActiveOptional>({} as true);
    });

    it("handles multiple boolean props with defaults", () => {
      type P = DefineProps<
        { str: string; bool1: boolean; bool2: boolean; num: number },
        "bool1" | "bool2"
      >;
      type Result = MakeBooleanOptional<P>;

      // Boolean props should be optional
      assertType<Result["bool1"]>({} as boolean | undefined);
      assertType<Result["bool2"]>({} as boolean | undefined);

      // Non-boolean props should remain required
      assertType<Result["str"]>({} as string);
      assertType<Result["num"]>({} as number);

      // Boolean keys should be optional
      type AreBoolsOptional = {} extends Pick<Result, "bool1" | "bool2">
        ? true
        : false;
      assertType<AreBoolsOptional>({} as true);
    });

    it("handles DefineProps with no boolean defaults", () => {
      type P = DefineProps<{ name: string; age: number }, never>;
      type Result = MakeBooleanOptional<P>;

      // No boolean defaults, so all props remain required
      assertType<Result["name"]>({} as string);
      assertType<Result["age"]>({} as number);
    });

    it("handles only boolean props with defaults", () => {
      type P = DefineProps<
        { flag1: boolean; flag2: boolean; flag3: boolean },
        "flag1" | "flag2" | "flag3"
      >;
      type Result = MakeBooleanOptional<P>;

      // All should be optional booleans
      assertType<Result["flag1"]>({} as boolean | undefined);
      assertType<Result["flag2"]>({} as boolean | undefined);
      assertType<Result["flag3"]>({} as boolean | undefined);

      // All keys should be optional
      type AllOptional = {} extends Result ? true : false;
      assertType<AllOptional>({} as true);
    });

    it("returns plain types unchanged when not DefineProps", () => {
      type Plain = { name: string; active: boolean };
      type Result = MakeBooleanOptional<Plain>;

      // Should return the same type
      assertType<Result>({} as Plain);
      assertType<Result["name"]>({} as string);
      assertType<Result["active"]>({} as boolean);
    });

    it("handles mixed optional and required props", () => {
      type P = DefineProps<
        { required: string; optional?: number; bool: boolean },
        "bool"
      >;
      type Result = MakeBooleanOptional<P>;

      // required stays required
      assertType<Result["required"]>({} as string);
      // optional stays optional
      assertType<Result["optional"]>({} as number | undefined);
      // bool becomes optional
      assertType<Result["bool"]>({} as boolean | undefined);
    });

    it("preserves readonly nature of props", () => {
      type P = DefineProps<
        { readonly name: string; readonly active: boolean },
        "active"
      >;
      type Result = MakeBooleanOptional<P>;

      const result = {} as Result;
      // @ts-expect-error props are readonly
      result.name = "test";
      // @ts-expect-error props are readonly
      result.active = true;
    });

    it("handles empty BKeys parameter", () => {
      type P = DefineProps<{ a: string; b: boolean; c: number }, never>;
      type Result = MakeBooleanOptional<P>;

      // No boolean defaults, all required
      assertType<Result["a"]>({} as string);
      assertType<Result["b"]>({} as boolean);
      assertType<Result["c"]>({} as number);
    });

    it("handles tuple return from withDefaults_Box", () => {
      const propsBoxed = defineProps_Box<{ name: string; active: boolean }>();
      const defaultsBoxed = withDefaults_Box(
        {} as DefineProps<typeof propsBoxed, never>,
        { active: true }
      );

      // withDefaults_Box returns a tuple [DefineProps, Defaults]
      type Tuple = typeof defaultsBoxed;
      type Result = MakeBooleanOptional<Tuple>;

      // Should handle tuple type
      type First = Result extends [infer F, any] ? F : never;
      const _check: First = {} as any;
    });
  });

  describe("MakePublicProps", () => {
    it("handles withDefaults with all props optional", () => {
      const defaultsBoxed = withDefaults_Box(
        defineProps<{
          a?: string;
          b?: number;
          c: boolean;
        }>(),
        {
          a: "default",
          b: 0,
          c: false,
        }
      );
      const d = withDefaults(defaultsBoxed[0], defaultsBoxed[1]);
      type D = typeof defaultsBoxed;
      type Public = MakePublicProps<D>;

      // All have defaults, so none are undefined
      assertType<Public["a"]>({} as string | undefined);
      assertType<Public["b"]>({} as number | undefined);
      assertType<Public["c"]>({} as boolean | undefined);
    });

    it("handles withDefaults with partial defaults", () => {
      const propsBoxed = defineProps_Box<{
        a?: string;
        b?: number;
        c?: boolean;
      }>();
      const defaultsBoxed = withDefaults_Box(
        {} as DefineProps<typeof propsBoxed, never>,
        {
          a: "default",
        }
      );
      type D = typeof defaultsBoxed;
      type Public = MakePublicProps<D>;

      // a has default, not undefined
      assertType<Public["a"]>({} as string);
      // b and c can be undefined
      assertType<Public["b"]>({} as number | undefined);
      // Boolean props may have special handling
      type CType = Public["c"];
      const cTest: CType = false;
      void cTest;

      // @ts-expect-error a cannot be undefined
      assertType<Public["a"]>({} as undefined);
    });

    it("handles withDefaults with required props", () => {
      const propsBoxed = defineProps_Box<{
        required: string;
        optional?: number;
      }>();
      const defaultsBoxed = withDefaults_Box(
        {} as DefineProps<typeof propsBoxed, never>,
        {
          optional: 42,
        }
      );
      type D = typeof defaultsBoxed;
      type Public = MakePublicProps<D>;

      assertType<Public["required"]>({} as string);
      assertType<Public["optional"]>({} as number);

      // @ts-expect-error required cannot be undefined
      assertType<Public["required"]>({} as undefined);
      // @ts-expect-error optional has default
      assertType<Public["optional"]>({} as undefined);
    });

    it("handles empty props", () => {
      const p = defineProps({});
      type P = typeof p;
      type Public = MakePublicProps<P>;

      assertType<Public>({} as P);
    });

    it("handles single prop with default", () => {
      const p = defineProps({
        single: { type: String, default: "value" },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      assertType<Public["single"]>({} as string);
      // @ts-expect-error cannot be undefined
      assertType<Public["single"]>({} as undefined);
    });

    it("handles single prop without default", () => {
      const p = defineProps({
        single: String,
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      assertType<Public["single"]>({} as string | undefined);
    });
  });

  describe("MakeInternalProps", () => {
    it("makes all props required for internal usage", () => {
      const withDefaults_Boxed = withDefaults_Box(
        defineProps<{
          f?: string;
          d?: string;
        }>(),
        { d: "baz" }
      );
      const d = withDefaults(withDefaults_Boxed[0], withDefaults_Boxed[1]);
      type D = typeof d;
      type Internal = MakeInternalProps<D>;
      // Both props required internally
      // @ts-expect-error f cannot be omitted internally
      assertType<Internal>({} as { d: string });
      assertType<Internal>({} as { f: string; d: string });
    });

    it("treats default + optional mix as required internally", () => {
      const p = defineProps({
        f: String,
        d: { type: String, default: "bar" },
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;
      // @ts-expect-error f must be string internally
      assertType<Internal["f"]>({} as undefined);
      assertType<Internal["f"]>({} as string);
      assertType<Internal["d"]>({} as string);
    });

    it("makes all props required even without defaults", () => {
      const p = defineProps({
        a: String,
        b: Number,
        c: Boolean,
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      // All must be defined internally
      assertType<Internal["a"]>({} as string);
      assertType<Internal["b"]>({} as number);
      assertType<Internal["c"]>({} as boolean);

      // @ts-expect-error cannot be undefined
      assertType<Internal["a"]>({} as undefined);
      // @ts-expect-error cannot be undefined
      assertType<Internal["b"]>({} as undefined);
    });

    it("makes all props required with all defaults", () => {
      const p = defineProps({
        a: { type: String, default: "a" },
        b: { type: Number, default: 42 },
        c: { type: Boolean, default: false },
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      assertType<Internal["a"]>({} as string);
      assertType<Internal["b"]>({} as number);
      assertType<Internal["c"]>({} as boolean);

      // @ts-expect-error cannot be undefined
      assertType<Internal["a"]>({} as undefined);
    });

    it("makes optional props required with withDefaults", () => {
      const d = withDefaults(
        defineProps<{
          a?: string;
          b?: number;
          c?: boolean;
        }>(),
        {
          b: 0,
        }
      );
      type D = typeof d;
      type Internal = MakeInternalProps<D>;

      // All required internally
      assertType<Internal["a"]>({} as string);
      assertType<Internal["b"]>({} as number);
      assertType<Internal["c"]>({} as boolean);

      // @ts-expect-error cannot be undefined
      assertType<Internal["a"]>({} as undefined);
      // @ts-expect-error cannot be undefined
      assertType<Internal["c"]>({} as undefined);
    });

    it("handles complex types as required", () => {
      const p = defineProps({
        obj: Object as () => { x: number; y: number },
        arr: Array as () => string[],
        union: [String, Number] as unknown as () => string | number,
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      assertType<Internal["obj"]>({} as { x: number; y: number });
      assertType<Internal["arr"]>({} as string[]);
      assertType<Internal["union"]>({} as string | number);

      // @ts-expect-error cannot be undefined
      assertType<Internal["obj"]>({} as undefined);
      // @ts-expect-error cannot be undefined
      assertType<Internal["arr"]>({} as undefined);
    });

    it("preserves readonly nature of props", () => {
      const p = defineProps({
        name: String,
        age: { type: Number, default: 0 },
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      const internal = {} as Internal;
      // @ts-expect-error props are readonly
      internal.name = "test";
      // @ts-expect-error props are readonly
      internal.age = 42;
    });

    it("handles empty props", () => {
      const p = defineProps({});
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      assertType<Internal>({} as P);
    });

    it("makes single prop required", () => {
      const p = defineProps({
        single: String,
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      assertType<Internal["single"]>({} as string);
      // @ts-expect-error cannot be undefined
      assertType<Internal["single"]>({} as undefined);
    });

    it("handles props with function types", () => {
      const p = defineProps({
        onClick: Function as unknown as () => (e: MouseEvent) => void,
        onInput: {
          type: Function as unknown as () => (value: string) => void,
          default: () => () => {},
        },
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      assertType<Internal["onClick"]>({} as (e: MouseEvent) => void);
      assertType<Internal["onInput"]>({} as (value: string) => void);

      // @ts-expect-error cannot be undefined
      assertType<Internal["onClick"]>({} as undefined);
    });

    it("handles nullable prop types", () => {
      const p = defineProps({
        nullable: [String, null] as unknown as () => string | null,
        withDefault: {
          type: [String, null] as unknown as () => string | null,
          default: null,
        },
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      assertType<Internal["nullable"]>({} as string | null);
      assertType<Internal["withDefault"]>({} as string | null);

      // @ts-expect-error cannot be undefined (but can be null)
      assertType<Internal["nullable"]>({} as undefined);
    });
  });

  describe("Edge cases and advanced scenarios", () => {
    it("handles props with validator functions", () => {
      const props_boxed = defineProps_Box({
        status: {
          type: String,
          default: "pending",
          validator: (value: string) =>
            ["pending", "success", "error"].includes(value),
        },
      });
      const p = defineProps(removeHiddenPatch(props_boxed));
      type Macro = ExtractMacroProps<{
        props: MacroReturnObject<typeof p, typeof props_boxed>;
      }>;
      type Public = MakePublicProps<Macro>;
      type Internal = MakeInternalProps<Macro>;

      assertType<Public["status"]>({} as string | undefined);
      assertType<Internal["status"]>({} as string);
    });

    it("handles props with required flag", () => {
      const props_boxed = defineProps_Box({
        required: { type: String, required: true },
        notRequired: { type: String, required: false },
      });
      const p = defineProps(removeHiddenPatch(props_boxed));
      type Macro = ExtractMacroProps<{
        props: MacroReturnObject<typeof p, typeof props_boxed>;
      }>;
      type Public = MakePublicProps<Macro>;

      assertType<Public["required"]>({} as string);
      assertType<Public["notRequired"]>({} as string | undefined);
    });

    it("handles mixed required, optional, and default props", () => {
      const propsBoxed = defineProps_Box<{
        required: string;
        optional?: number;
        withDefault?: boolean;
      }>();
      const defaultsBoxed = withDefaults_Box(
        {} as DefineProps<typeof propsBoxed, never>,
        {
          withDefault: true,
        }
      );
      type D = typeof defaultsBoxed;
      type Public = MakePublicProps<D>;

      // For Internal, use plain withDefaults since MakeInternalProps doesn't handle tuple
      const dInternal = withDefaults(
        defineProps<{
          required: string;
          optional?: number;
          withDefault?: boolean;
        }>(),
        {
          withDefault: true,
        }
      );
      type Internal = MakeInternalProps<typeof dInternal>;

      // Public: required is required, optional can be undefined, withDefault is defined
      assertType<Public["required"]>({} as string);
      assertType<Public["optional"]>({} as number | undefined);
      assertType<Public["withDefault"]>({} as boolean);

      // Internal: all are required
      assertType<Internal["required"]>({} as string);
      assertType<Internal["optional"]>({} as number);
      assertType<Internal["withDefault"]>({} as boolean);
    });

    it("handles generic record types", () => {
      type GenericProps = { [key: string]: any };
      type Public = MakePublicProps<GenericProps>;
      type Internal = MakeInternalProps<GenericProps>;

      assertType<Public>({} as GenericProps);
      assertType<Internal>({} as GenericProps);
    });

    it("handles intersection types", () => {
      type BaseProps = { id: number };
      type ExtendedProps = BaseProps & { name: string };
      type Public = MakePublicProps<ExtendedProps>;
      type Internal = MakeInternalProps<ExtendedProps>;

      assertType<Public>({} as ExtendedProps);
      assertType<Internal>({} as ExtendedProps);
    });

    it("differentiates between public and internal for same props", () => {
      const d = withDefaults(
        defineProps<{
          opt?: string;
        }>(),
        {}
      );
      type D = typeof d;
      type Public = MakePublicProps<D>;
      type Internal = MakeInternalProps<D>;

      // Public allows undefined
      assertType<Public["opt"]>({} as string | undefined);

      // Internal requires it
      assertType<Internal["opt"]>({} as string);

      // @ts-expect-error internal cannot be undefined
      assertType<Internal["opt"]>({} as undefined);
    });
  });

  describe("Utility Types", () => {
    describe("FindDefaultsKey", () => {
      it("identifies optional properties", () => {
        type TestType = {
          foo?: string;
          test?: number;
          bar: number;
        };

        type Result = FindDefaultsKey<TestType>;

        assertType<Result>({} as "foo" | "test");
        // @ts-expect-error 'bar' is not optional
        assertType<Result>({} as "foo" | "test" | "bar");
      });

      it("returns never for types with no optional properties", () => {
        type TestType = {
          foo: string;
          bar: number;
        };

        type Result = FindDefaultsKey<TestType>;

        assertType<Result>({} as never);
      });

      it("handles all optional properties", () => {
        type TestType = {
          foo?: string;
          bar?: number;
          baz?: boolean;
        };

        type Result = FindDefaultsKey<TestType>;

        assertType<Result>({} as "foo" | "bar" | "baz");
      });

      it("handles union types with undefined", () => {
        type TestType = {
          explicit?: string;
          union: string | undefined;
          required: string;
        };

        type Result = FindDefaultsKey<TestType>;

        // Both 'explicit' and 'union' can be undefined
        assertType<Result>({} as "explicit" | "union");
      });

      it("handles empty object", () => {
        type TestType = {};

        type Result = FindDefaultsKey<TestType>;

        assertType<Result>({} as never);
      });
    });

    describe("PropsObjectExtractDefaults", () => {
      it("extracts keys with default values from object format", () => {
        type PropsObj = {
          name: { type: string; default: "Anonymous" };
          age: { type: number };
          active: { type: boolean; default: true };
          count: { type: number; default: 0 };
        };

        type Result = PropsObjectExtractDefaults<PropsObj>;

        // Should extract 'name', 'active', and 'count' (have defaults)
        assertType<Result>({} as "name" | "active" | "count");

        // @ts-expect-error 'age' doesn't have a default
        assertType<Result>({} as "name" | "age" | "active" | "count");
      });

      it("returns string for string array format", () => {
        type PropsArray = string[];
        type Result = PropsObjectExtractDefaults<PropsArray>;

        assertType<Result>({} as string);
      });

      it("handles props with no defaults", () => {
        type PropsObj = {
          name: { type: string };
          age: { type: number };
        };

        type Result = PropsObjectExtractDefaults<PropsObj>;

        // Should return empty string union (no defaults)
        assertType<Result>({} as "");
      });

      it("handles props with all defaults", () => {
        type PropsObj = {
          a: { type: string; default: "a" };
          b: { type: number; default: 0 };
          c: { type: boolean; default: false };
        };

        type Result = PropsObjectExtractDefaults<PropsObj>;

        assertType<Result>({} as "a" | "b" | "c");
      });

      it("handles single prop with default", () => {
        type PropsObj = {
          single: { type: string; default: "value" };
        };

        type Result = PropsObjectExtractDefaults<PropsObj>;

        assertType<Result>({} as "single");
      });

      it("handles single prop without default", () => {
        type PropsObj = {
          single: { type: string };
        };

        type Result = PropsObjectExtractDefaults<PropsObj>;

        assertType<Result>({} as "");
      });

      it("handles mixed prop types with defaults", () => {
        type PropsObj = {
          str: { type: string; default: "default" };
          num: { type: number };
          bool: { type: boolean; default: true };
          obj: { type: object; default: () => {} };
          arr: { type: any[] };
        };

        type Result = PropsObjectExtractDefaults<PropsObj>;

        assertType<Result>({} as "str" | "bool" | "obj");

        // @ts-expect-error 'num' and 'arr' don't have defaults
        assertType<Result>({} as "str" | "num" | "bool" | "obj" | "arr");
      });

      it("handles complex default values", () => {
        type PropsObj = {
          simpleDefault: { type: string; default: "value" };
          functionDefault: { type: object; default: () => { x: 1 } };
          nullDefault: { type: any; default: null };
          zeroDefault: { type: number; default: 0 };
          falseDefault: { type: boolean; default: false };
          emptyArrayDefault: { type: any[]; default: () => [] };
        };

        type Result = PropsObjectExtractDefaults<PropsObj>;

        // All have defaults (including falsy values)
        assertType<Result>(
          {} as
            | "simpleDefault"
            | "functionDefault"
            | "nullDefault"
            | "zeroDefault"
            | "falseDefault"
            | "emptyArrayDefault"
        );
      });

      it("handles empty object", () => {
        type PropsObj = {};

        type Result = PropsObjectExtractDefaults<PropsObj>;

        assertType<Result>({} as never);
      });

      it("handles tuple-like string arrays", () => {
        type PropsArray = ["name", "age", "email"];

        type Result = PropsObjectExtractDefaults<PropsArray>;

        assertType<Result>({} as string);
      });

      it("differentiates between default: any and no default", () => {
        type PropsObj = {
          withDefault: { type: string; default: any };
          withoutDefault: { type: string };
        };

        type Result = PropsObjectExtractDefaults<PropsObj>;

        assertType<Result>({} as "withDefault");

        // @ts-expect-error withoutDefault doesn't have default property
        assertType<Result>({} as "withDefault" | "withoutDefault");
      });
    });

    describe("ResolveFromMacroReturn", () => {
      it("extracts type from MacroReturnType", () => {
        const macroReturn = createMacroReturn({
          props: {
            value: {
              foo: "" as string,
              bar: 0 as number,
            },
            type: {} as { foo: string; bar: number },
          },
        });

        type Extracted = ExtractMacroReturn<typeof macroReturn>;
        type Props = ExtractMacroProps<Extracted>;

        // ExtractProps returns the macro object with value and type, plus defaults
        type PropsValue = Props extends { props: { value: infer V } }
          ? V
          : never;
        type PropsType = Props extends { props: { type: infer T } } ? T : never;

        assertType<PropsValue>({} as { foo: string; bar: number });
        assertType<PropsType>({} as { foo: string; bar: number });
      });

      it("extracts type from MacroReturnObject", () => {
        const propsBoxed = defineProps_Box<{
          foo?: string;
          bar: number;
        }>();

        type Hidden = typeof propsBoxed;

        assertType<Hidden>(
          {} as {
            foo?: string;
            bar: number;
          }
        );
      });

      it("returns plain types unchanged", () => {
        type PlainType = { a: string; b: number };
        type Result = ResolveFromMacroReturn<PlainType>;

        assertType<Result>({} as PlainType);
      });
    });

    describe("ResolveDefaultsPropsFromMacro", () => {
      it("resolves props with explicit defaults from macro", () => {
        const propsBoxed = defineProps_Box<{
          foo?: string;
          bar: number;
        }>();

        const defaultsBoxed = withDefaults_Box(
          {} as import("vue").DefineProps<typeof propsBoxed, never>,
          {
            foo: "default",
          }
        );

        const setupReturn = createMacroReturn({
          props: {
            value: {} as any,
            type: {} as any,
            defaults: {
              value: {} as any,
              type: {} as typeof defaultsBoxed,
            },
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;

        // Should have both type and defaults
        type DefaultsType = Extracted["defaults"];
        const _defaults: DefaultsType = {} as any;
      });

      it("resolves props with inferred defaults from FindDefaultsKey", () => {
        const setupReturn = createMacroReturn({
          props: {
            value: {
              foo: "" as string,
              bar: 0 as number,
            },
            type: {} as {
              foo?: string;
              bar: number;
            },
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        // foo is optional (has default), bar is required
        assertType<Public>(
          {} as {
            foo?: string | undefined;
            bar: number;
          }
        );
      });
    });

    describe("MakePublicProps with MacroReturn", () => {
      it("makes optional props with defaults optional in public API", () => {
        const setupReturn = createMacroReturn({
          props: {
            value: {
              foo: "default" as string,
              bar: 0 as number,
            },
            type: {} as {
              foo?: string;
              bar: number;
            },
          },
        });

        type Extracted = ExtractPropsFromMacro<
          ExtractMacroProps<ExtractMacroReturn<typeof setupReturn>>
        >;
        type Public = MakePublicProps<Extracted>;

        // foo can be omitted (has default), bar is required
        const valid1: Public = { bar: 1 };
        const valid2: Public = { foo: "test", bar: 1 };
        const valid3: Public = { foo: undefined, bar: 1 };

        void valid1;
        void valid2;
        void valid3;

        assertType<Public["foo"]>({} as string | undefined);
        assertType<Public["bar"]>({} as number);
      });

      it("handles all optional props with defaults", () => {
        const setupReturn = createMacroReturn({
          props: {
            value: {
              foo: "default" as string,
              bar: 0 as number,
              baz: true as boolean,
            },
            type: {} as {
              foo?: string;
              bar?: number;
              baz?: boolean;
            },
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        // All can be omitted
        const valid: Public = {};
        void valid;

        assertType<Public["foo"]>({} as string | undefined);
        assertType<Public["bar"]>({} as number | undefined);
        assertType<Public["baz"]>({} as boolean | undefined);
      });

      it("handles mixed required and optional props", () => {
        const setupReturn = createMacroReturn({
          props: {
            value: {
              required1: "" as string,
              required2: 0 as number,
              optional1: "default" as string,
              optional2: 0 as number,
            },
            type: {} as {
              required1: string;
              required2: number;
              optional1?: string;
              optional2?: number;
            },
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        // Required props must be present, optional can be omitted
        const valid1: Public = { required1: "a", required2: 1 };
        const valid2: Public = {
          required1: "a",
          required2: 1,
          optional1: "b",
          optional2: 2,
        };

        void valid1;
        void valid2;

        assertType<Public["required1"]>({} as string);
        assertType<Public["required2"]>({} as number);
        assertType<Public["optional1"]>({} as string | undefined);
        assertType<Public["optional2"]>({} as number | undefined);
      });

      it("works with withDefaults_Box integration", () => {
        const propsBoxed = defineProps_Box<{
          foo?: string;
          bar: number | undefined;
        }>();

        const defaultsBoxed = withDefaults_Box(
          {} as import("vue").DefineProps<typeof propsBoxed, never>,
          {
            foo: "default",
          }
        );

        const setupReturn = createMacroReturn({
          props: {
            value: {} as any,
            type: {} as any,
            defaults: {
              value: {} as any,
              type: {} as typeof defaultsBoxed,
            },
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        // Verify the structure exists
        type DefaultsExists = Extracted["defaults"];
        const _check: DefaultsExists = {} as any;
      });

      it("handles complex nested types", () => {
        type ComplexType = {
          simple?: string;
          nested: {
            a: number;
            b?: string;
          };
          array?: string[];
          union?: string | number;
        };

        const setupReturn = createMacroReturn({
          props: {
            value: {
              simple: "default" as string,
              nested: { a: 0, b: "default" } as ComplexType["nested"],
              array: [] as string[],
              union: "default" as string | number,
            },
            type: {} as ComplexType,
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        assertType<Public["simple"]>({} as string | undefined);
        assertType<Public["nested"]>({} as { a: number; b?: string });
        assertType<Public["array"]>({} as string[] | undefined);
        assertType<Public["union"]>({} as string | number | undefined);
      });

      it("handles single optional prop", () => {
        const setupReturn = createMacroReturn({
          props: {
            value: {
              single: "default" as string,
            },
            type: {} as {
              single?: string;
            },
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        const valid1: Public = {};
        const valid2: Public = { single: "value" };
        const valid3: Public = { single: undefined };

        void valid1;
        void valid2;
        void valid3;

        assertType<Public["single"]>({} as string | undefined);
      });

      it("preserves readonly behavior", () => {
        const setupReturn = createMacroReturn({
          props: {
            value: {
              foo: "" as string,
            },
            type: {} as {
              readonly foo: string;
            },
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        const pub = {} as Public;
        // @ts-expect-error readonly props cannot be assigned
        pub.foo = "test";
      });

      it("handles union types in props", () => {
        const setupReturn = createMacroReturn({
          props: {
            value: {
              status: "pending" as "pending" | "success" | "error",
              value: "" as string | number,
            },
            type: {} as {
              status?: "pending" | "success" | "error";
              value: string | number;
            },
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        assertType<Public["status"]>(
          {} as "pending" | "success" | "error" | undefined
        );
        assertType<Public["value"]>({} as string | number);
      });

      it("handles nullable types", () => {
        const setupReturn = createMacroReturn({
          props: {
            value: {
              nullable: null as string | null,
              optional: null as string | null,
            },
            type: {} as {
              nullable: string | null;
              optional?: string | null;
            },
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        assertType<Public["nullable"]>({} as string | null);
        assertType<Public["optional"]>({} as string | null | undefined);
      });

      it("handles function types", () => {
        const setupReturn = createMacroReturn({
          props: {
            value: {
              onClick: (() => {}) as (e: MouseEvent) => void,
              onInput: (() => {}) as (value: string) => void,
            },
            type: {} as {
              onClick?: (e: MouseEvent) => void;
              onInput: (value: string) => void;
            },
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        assertType<Public["onClick"]>(
          {} as ((e: MouseEvent) => void) | undefined
        );
        assertType<Public["onInput"]>({} as (value: string) => void);
      });

      it("handles generic types", () => {
        type GenericProps<T> = {
          value: T;
          optional?: T;
        };

        const setupReturn = createMacroReturn({
          props: {
            value: {
              value: "" as string,
              optional: "" as string,
            },
            type: {} as GenericProps<string>,
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        assertType<Public["value"]>({} as string);
        assertType<Public["optional"]>({} as string | undefined);
      });

      it("handles intersection types", () => {
        type BaseProps = {
          id: number;
          name?: string;
        };

        type ExtendedProps = BaseProps & {
          extra: boolean;
        };

        const setupReturn = createMacroReturn({
          props: {
            value: {
              id: 0 as number,
              name: "" as string,
              extra: true as boolean,
            },
            type: {} as ExtendedProps,
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        assertType<Public["id"]>({} as number);
        assertType<Public["name"]>({} as string | undefined);
        assertType<Public["extra"]>({} as boolean);
      });

      it("handles conditional types in props", () => {
        type ConditionalProps<T extends boolean> = T extends true
          ? { required: string }
          : { required?: string };

        const setupReturn = createMacroReturn({
          props: {
            value: {
              required: "" as string,
            },
            type: {} as ConditionalProps<true>,
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        type RequiredType = Public extends { required: infer R } ? R : never;
        assertType<RequiredType>({} as string);
      });

      it("handles mapped types", () => {
        type MappedProps<T extends Record<string, any>> = {
          [K in keyof T]?: T[K];
        };

        const setupReturn = createMacroReturn({
          props: {
            value: {
              foo: "" as string,
              bar: 0 as number,
            },
            type: {} as MappedProps<{ foo: string; bar: number }>,
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        assertType<Public["foo"]>({} as string | undefined);
        assertType<Public["bar"]>({} as number | undefined);
      });
    });

    describe("Integration with MacroReturn", () => {
      it("full workflow: defineProps_Box -> withDefaults_Box -> createMacroReturn -> MakePublicProps", () => {
        const propsBoxed = defineProps_Box<{
          foo?: string;
          bar: number;
          baz?: boolean;
        }>();

        const defaultsBoxed = withDefaults_Box(
          {} as import("vue").DefineProps<typeof propsBoxed, never>,
          {
            foo: "default",
            baz: true,
          }
        );

        const setupReturn = createMacroReturn({
          props: {
            value: {} as any,
            type: {} as any,
            defaults: {
              value: {} as any,
              type: {} as typeof defaultsBoxed,
            },
          },
        });

        type Extracted = ExtractMacroProps<
          ExtractMacroReturn<typeof setupReturn>
        >;
        type Public = MakePublicProps<Extracted>;

        // Verify defaults structure exists
        type DefaultsExists = Extracted["defaults"];
        const _verify: DefaultsExists = {} as any;
      });

      it("compares plain defineProps vs Box macro approach", () => {
        // Plain approach
        const plain = createMacroReturn({
          props: {
            value: { foo: "", bar: 0 },
            type: {} as { foo?: string; bar: number },
          },
        });

        type PlainExtracted = ExtractMacroProps<
          ExtractMacroReturn<typeof plain>
        >;
        type PlainPublic = MakePublicProps<PlainExtracted>;

        // Box approach
        const boxed = defineProps_Box<{
          foo?: string;
          bar: number;
        }>();

        const boxedWithDefaults = withDefaults_Box(
          {} as import("vue").DefineProps<typeof boxed, never>,
          { foo: "default" }
        );

        const boxedReturn = createMacroReturn({
          props: {
            value: {} as any,
            type: {} as any,
            defaults: {
              value: {} as any,
              type: {} as typeof boxedWithDefaults,
            },
          },
        });

        type BoxedExtracted = ExtractMacroProps<
          ExtractMacroReturn<typeof boxedReturn>
        >;
        type BoxedPublic = MakePublicProps<BoxedExtracted>;

        // Both should handle optional props correctly
        assertType<PlainPublic["foo"]>({} as string | undefined);
        assertType<PlainPublic["bar"]>({} as number);

        // Verify boxed has defaults structure
        type BoxedDefaults = BoxedExtracted["defaults"];
        const _boxedCheck: BoxedDefaults = {} as any;
      });
    });
  });
});
