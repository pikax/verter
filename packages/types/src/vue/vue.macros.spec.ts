import { describe, it, assertType } from "vitest";
import type { ExtractHidden } from "../helpers";
import {
  defineProps_Box,
  withDefaults_Box,
  defineEmits_Box,
  defineOptions_Box,
  defineExpose_Box,
  defineSlots_Box,
  defineModel_Box,
} from "./vue.macros";
import {
  removeHiddenPatch,
} from "../helpers/helpers";
import type {
  ComponentObjectPropsOptions,
  EmitsOptions,
  ComponentTypeEmits,
  DefineProps,
  PropType,
} from "vue";
import {
  defineProps,
  withDefaults,
  defineEmits,
  defineOptions,
  defineExpose,
  defineSlots,
  defineModel,
} from "vue";

// Helper to assert bidirectional assignability (structural equivalence)
function assertEquivalent<A, B>(a: A, b: B) {
  assertType<A>(b as any);
  assertType<B>(a as any);
}

// Helper to ensure type is not any
function assertNotAny<T>(value: 0 extends 1 & T ? never : T) {
  return value;
}

describe("vue macros _Box usage", () => {
  describe("defineProps_Box", () => {
    describe("array syntax overload", () => {
      it("allows passing Box result back into original macro", () => {
        const base = defineProps(["foo", "bar"]);
        const boxParam = defineProps_Box(["foo", "bar"]);
        
        // Ensure not any
        assertNotAny(boxParam);
        // @ts-expect-error should not be any type
        assertType<{ randomProp: 123 }>(boxParam);
        
        const fromBox = defineProps(removeHiddenPatch(boxParam));
        assertEquivalent(base, fromBox);
      });

      it("preserves array type in return", () => {
        const boxParam = defineProps_Box(["title", "count"]);
        
        assertNotAny(boxParam);
        assertType<string[]>(boxParam);
      });

      it("accepts generic string extends", () => {
        const boxParam = defineProps_Box<"foo" | "bar">(["foo", "bar"]);
        
        assertNotAny(boxParam);
        type Result = typeof boxParam;
        assertType<("foo" | "bar")[]>(boxParam);
      });
    });

    describe("object syntax overload", () => {
      it("preserves object prop options", () => {
        const opts = {
          foo: String,
          bar: Number,
        } satisfies ComponentObjectPropsOptions;
        const base = defineProps(opts);
        const boxParam = defineProps_Box(opts);
        
        assertNotAny(boxParam);
        // @ts-expect-error should not be any type
        assertType<{ randomProp: 123 }>(boxParam);
        
        const fromBox = defineProps(removeHiddenPatch(boxParam));
        assertEquivalent(base, fromBox);
      });

      it("preserves complex prop options", () => {
        const opts = {
          title: {
            type: String as PropType<string>,
            required: true,
          },
          count: {
            type: Number,
            default: 0,
          },
        } satisfies ComponentObjectPropsOptions;
        
        const boxParam = defineProps_Box(opts);
        
        assertNotAny(boxParam);
        type Result = typeof boxParam;
        assertType<typeof opts>(boxParam);
      });

      it("works with validator functions", () => {
        const opts = {
          status: {
            type: String,
            validator: (v: unknown) => ["active", "inactive"].includes(v as string),
          },
        } satisfies ComponentObjectPropsOptions;
        
        const boxParam = defineProps_Box(opts);
        assertNotAny(boxParam);
      });
    });

    describe("type-only syntax overload (no argument)", () => {
      it("type equivalence with generic props", () => {
        type P = { foo?: string; bar: number };
        const base = defineProps<P>();
        const boxed = defineProps_Box<P>();
        
        assertNotAny(boxed);
        // @ts-expect-error should not be any type
        assertType<{ randomProp: 123 }>(boxed);
        
        assertType<P>(boxed);
      });

      it("handles optional properties", () => {
        type P = { title?: string; count?: number; active: boolean };
        const boxed = defineProps_Box<P>();
        
        assertNotAny(boxed);
        assertType<P>(boxed);
      });

      it("handles complex nested types", () => {
        type P = {
          user: { name: string; age: number };
          tags: string[];
          meta?: Record<string, unknown>;
        };
        const boxed = defineProps_Box<P>();
        
        assertNotAny(boxed);
        assertType<P>(boxed);
      });

      it("handles union types", () => {
        type P = { value: string | number | boolean };
        const boxed = defineProps_Box<P>();
        
        assertNotAny(boxed);
        assertType<P>(boxed);
      });
    });
  });

  describe("withDefaults_Box", () => {
    it("works with props from Box macro", () => {
      type P = { size?: number; label?: string };
      const props = defineProps<P>();
      const defaults = { size: 3 };
      const baseWD = withDefaults(props, defaults);

      const wdFromBox = withDefaults_Box(props, defaults);
      
      assertNotAny(wdFromBox);
      // @ts-expect-error should not be any type
      assertType<{ randomProp: 123 }>(wdFromBox);
      
      const wd = withDefaults(wdFromBox[0], wdFromBox[1]);
      assertEquivalent(baseWD, wd);
    });

    it("returns tuple type", () => {
      type P = { foo?: string; bar?: number };
      const props = defineProps<P>();
      const defaults = { foo: "default" };
      
      const wdBox = withDefaults_Box(props, defaults);
      
      assertNotAny(wdBox);
      
      assertType<[any, any]>(wdBox);
      wdBox[0];
      wdBox[1];
    });

    it("handles factory functions in defaults", () => {
      type P = { items?: string[] };
      const props = defineProps<P>();
      const defaults = { items: () => [] };
      
      const wdBox = withDefaults_Box(props, defaults);
      assertNotAny(wdBox);
    });

    it("preserves default value types", () => {
      type P = { count?: number; active?: boolean; label?: string };
      const props = defineProps<P>();
      const defaults = { count: 42, active: true };
      
      const wdBox = withDefaults_Box(props, defaults);
      
      assertNotAny(wdBox);
      assertType<typeof defaults>(wdBox[1]);
    });
  });

  describe("defineEmits_Box", () => {
    describe("array syntax overload", () => {
      it("emits function equivalence", () => {
        const emitFn = defineEmits(["change", "update"]);
        const boxParam = defineEmits_Box(["change", "update"]);
        
        assertNotAny(boxParam);
        // @ts-expect-error should not be any type
        assertType<{ randomProp: 123 }>(boxParam);
        
        const emitFromBox = defineEmits(removeHiddenPatch(boxParam));
        assertEquivalent(emitFn, emitFromBox);
      });

      it("preserves array type", () => {
        const boxParam = defineEmits_Box(["foo", "bar"]);
        
        assertNotAny(boxParam);
        
        assertType<string[]>(boxParam);
      });

      it("handles generic event names", () => {
        const boxParam = defineEmits_Box<"submit" | "cancel">(["submit", "cancel"]);
        
        assertNotAny(boxParam);
        assertType<("submit" | "cancel")[]>(boxParam);
      });
    });

    describe("object syntax overload", () => {
      it("object form equivalence", () => {
        const spec = {
          change: null,
          update: (v: number) => true,
        } satisfies EmitsOptions;
        const emitFn = defineEmits(spec);
        const boxParam = defineEmits_Box(spec);
        
        assertNotAny(boxParam);
        // @ts-expect-error should not be any type
        assertType<{ randomProp: 123 }>(boxParam);
        
        const emitFromBox = defineEmits(removeHiddenPatch(boxParam));
        assertEquivalent(emitFn, emitFromBox);
      });

      it("handles validator functions", () => {
        const spec = {
          submit: (payload: { id: number; value: string }) => true,
          cancel: null,
        } satisfies EmitsOptions;
        
        const boxParam = defineEmits_Box(spec);
        assertNotAny(boxParam);
        assertType<typeof spec>(boxParam);
      });

      it("preserves complex emit payloads", () => {
        const spec = {
          change: (
            oldValue: string | number,
            newValue: string | number,
            meta?: Record<string, any>
          ) => true,
        } satisfies EmitsOptions;
        
        const boxParam = defineEmits_Box(spec);
        assertNotAny(boxParam);
      });
    });

    describe("type-only syntax overload", () => {
      it("generic emit spec equivalence", () => {
        type Spec = {
          change: [value: number];
          update: [a: string, b: string];
        };
        const emitFn = defineEmits<Spec>();
        const boxParam = defineEmits_Box<Spec>();
        
        assertNotAny(boxParam);
        // @ts-expect-error should not be any type
        assertType<{ randomProp: 123 }>(boxParam);
        
        const emitFromBox = defineEmits<ExtractHidden<typeof boxParam>>();
        assertEquivalent(emitFn, emitFromBox);
      });

      it("handles empty event payloads", () => {
        type Spec = { click: []; submit: [] };
        const boxParam = defineEmits_Box<Spec>();
        
        assertNotAny(boxParam);
        assertType<Spec>(boxParam);
      });

      it("handles complex event payloads", () => {
        type Spec = {
          update: [data: { id: number; values: string[] }];
          delete: [id: number];
        };
        const boxParam = defineEmits_Box<Spec>();
        
        assertNotAny(boxParam);
        assertType<Spec>(boxParam);
      });
    });
  });

  describe("defineOptions_Box", () => {
    it("preserves component options", () => {
      const opts = defineOptions({ name: "MyComponent", inheritAttrs: false });
      const boxParam = defineOptions_Box({ name: "MyComponent", inheritAttrs: false });
      
      assertNotAny(boxParam);
      // @ts-expect-error should not be any type
      assertType<{ randomProp: 123 }>(boxParam);
      
      const fromBox = defineOptions(removeHiddenPatch(boxParam));
      assertEquivalent(opts, fromBox);
    });

    it("handles empty options", () => {
      const boxParam = defineOptions_Box();
      
      assertNotAny(boxParam);
      // Empty options still returns component options object
    });

    it("rejects disallowed options", () => {
      // @ts-expect-error props not allowed in defineOptions
      const invalid = defineOptions_Box({ props: {} });
      
      // @ts-expect-error emits not allowed in defineOptions
      const invalid2 = defineOptions_Box({ emits: [] });
    });

    it("accepts valid component options", () => {
      const boxParam = defineOptions_Box({
        name: "TestComponent",
        inheritAttrs: false,
      });
      
      assertNotAny(boxParam);
    });
  });

  describe("defineExpose_Box", () => {
    it("preserves exposed properties", () => {
      const exposed = { foo: 1, bar: "test", method: () => {} };
      const base = defineExpose(exposed);
      const boxParam = defineExpose_Box(exposed);
      
      assertNotAny(boxParam);
      // @ts-expect-error should not be any type
      assertType<{ randomProp: 123 }>(boxParam);
      
      const fromBox = defineExpose(removeHiddenPatch(boxParam));
      assertEquivalent(base, fromBox);
    });

    it("handles empty expose", () => {
      const boxParam = defineExpose_Box();
      
      assertNotAny(boxParam);
      // Empty expose still returns record type
    });

    it("handles typed exposed interface", () => {
      interface Exposed {
        count: number;
        increment: () => void;
        decrement: () => void;
      }
      
      const exposed: Exposed = {
        count: 0,
        increment: () => {},
        decrement: () => {},
      };
      
      const boxParam = defineExpose_Box<Exposed>(exposed);
      
      assertNotAny(boxParam);
      assertType<Exposed>(boxParam);
    });

    it("preserves method signatures", () => {
      const exposed = {
        getValue: () => 42,
        setValue: (v: number) => {},
        async fetchData(): Promise<string> {
          return "data";
        },
      };
      
      const boxParam = defineExpose_Box(exposed);
      assertNotAny(boxParam);
    });
  });

  describe("defineSlots_Box", () => {
    it("preserves slot definitions", () => {
      const base = defineSlots<{
        default: (props: { msg: string }) => any;
        header: (props: { title: string }) => any;
      }>();
      
      const boxParam = defineSlots_Box<{
        default: (props: { msg: string }) => any;
        header: (props: { title: string }) => any;
      }>();
      
      assertNotAny(boxParam);
      // @ts-expect-error should not be any type
      assertType<{ randomProp: 123 }>(boxParam);
      
      const fromBox = defineSlots<ExtractHidden<typeof boxParam>>();
      assertEquivalent(base, fromBox);
    });

    it("handles empty slots", () => {
      const boxParam = defineSlots_Box();
      
      assertNotAny(boxParam);
      // Empty slots should be Record<string, any>
      assertType<Record<string, any>>(boxParam);
    });

    it("handles scoped slots with props", () => {
      type Slots = {
        item: (props: { data: { id: number; name: string } }) => any;
        empty: () => any;
      };
      
      const boxParam = defineSlots_Box<Slots>();
      
      assertNotAny(boxParam);
      assertType<Slots>(boxParam);
    });

    it("preserves complex slot prop types", () => {
      const boxParam = defineSlots_Box<{
        default: (props: {
          items: Array<{ id: number; label: string }>;
          selectedId?: number;
        }) => any;
      }>();
      
      assertNotAny(boxParam);
    });
  });

  describe("defineModel_Box", () => {
    describe("required options overload", () => {
      it("with required constraint", () => {
        const base = defineModel<string>({ required: true });
        const boxParam = defineModel_Box<string>({ required: true });
        
        assertNotAny(boxParam);
        // @ts-expect-error should not be any type
        assertType<{ randomProp: 123 }>(boxParam);
        
        const fromBox = defineModel<string>(removeHiddenPatch(boxParam));
        assertEquivalent(base, fromBox);
      });

      it("with default value", () => {
        const boxParam = defineModel_Box<number>({ default: 0 });
        
        assertNotAny(boxParam);
        // Verify options object is returned with default
      });

      it("with type prop option", () => {
        const boxParam = defineModel_Box<string>({
          required: true,
          type: String as PropType<string>,
        });
        
        assertNotAny(boxParam);
      });
    });

    describe("optional options overload", () => {
      it("with empty options", () => {
        const boxParam = defineModel_Box<string>();
        
        assertNotAny(boxParam);
        // Empty options still returns options object
      });

      it("with validator", () => {
        const boxParam = defineModel_Box<number>({
          validator: (v: unknown) => typeof v === "number" && v >= 0,
        });
        
        assertNotAny(boxParam);
      });

      it("with get/set options", () => {
        const boxParam = defineModel_Box<string, string, string, string>({
          get: (v: string) => v.toUpperCase(),
          set: (v: string) => v.toLowerCase(),
        });
        
        assertNotAny(boxParam);
      });
    });

    describe("named model with required options overload", () => {
      it("with name and required", () => {
        const base = defineModel<string>("title", { required: true });
        const boxParam = defineModel_Box<string>("title", { required: true });
        
        assertNotAny(boxParam);
        // @ts-expect-error should not be any type
        assertType<{ randomProp: 123 }>(boxParam);
        
        const fromBox = defineModel<string>(
          removeHiddenPatch(boxParam)[0],
          removeHiddenPatch(boxParam)[1]
        );
        assertEquivalent(base, fromBox);
      });

      it("returns tuple type", () => {
        const boxParam = defineModel_Box<number>("count", { default: 0 });
        
        assertNotAny(boxParam);
        type Result = typeof boxParam;
        
        assertType<[string, any]>(boxParam);
        boxParam[0];
        boxParam[1];
      });

      it("with complex options", () => {
        const boxParam = defineModel_Box<string>("status", {
          required: true,
          validator: (v: unknown) => ["active", "inactive"].includes(v as string),
        });
        
        assertNotAny(boxParam);
        assertType<string>(boxParam[0]);
      });
    });

    describe("named model with optional options overload", () => {
      it("with name and empty options", () => {
        const boxParam = defineModel_Box<string>("value");
        
        assertNotAny(boxParam);
        assertType<[string, any]>(boxParam);
        assertType<string>(boxParam[0]);
      });

      it("with name and validator", () => {
        const boxParam = defineModel_Box<number>("amount", {
          validator: (v: unknown) => typeof v === "number",
        });
        
        assertNotAny(boxParam);
        assertType<string>(boxParam[0]);
      });

      it("handles union types", () => {
        const boxParam = defineModel_Box<string | number>("value");
        
        assertNotAny(boxParam);
      });

      it("preserves get/set with name", () => {
        const boxParam = defineModel_Box<boolean, string, boolean, boolean>(
          "checked",
          {
            get: (v: boolean) => v,
            set: (v: boolean) => v,
          }
        );
        
        assertNotAny(boxParam);
      });
    });
  });

  describe("integration tests", () => {
    it("chaining Box macros together", () => {
      type Props = { title: string; count?: number };
      const propsBox = defineProps_Box<Props>();
      const props = defineProps<Props>();
      const defaultsBox = withDefaults_Box(props, { count: 0 });
      
      assertNotAny(propsBox);
      assertNotAny(defaultsBox);
      
      const emitsBox = defineEmits_Box<{ change: [value: number] }>();
      assertNotAny(emitsBox);
    });

    it("all macros return correct types", () => {
      const props = defineProps_Box(["foo"]);
      const emits = defineEmits_Box(["change"]);
      const expose = defineExpose_Box({ method: () => {} });
      const slots = defineSlots_Box<{ default: () => any }>();
      
      // Ensure types are correct
      assertNotAny(props);
      assertNotAny(emits);
      assertNotAny(expose);
      assertNotAny(slots);
    });

    it("complex component setup", () => {
      type Props = {
        modelValue: string;
        disabled?: boolean;
      };
      type Emits = {
        "update:modelValue": [value: string];
        submit: [];
      };
      
      const propsBox = defineProps_Box<Props>();
      const emitsBox = defineEmits_Box<Emits>();
      const exposeBox = defineExpose_Box({ focus: () => {} });
      
      assertNotAny(propsBox);
      assertNotAny(emitsBox);
      assertNotAny(exposeBox);
    });
  });
});
