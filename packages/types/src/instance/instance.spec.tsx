import "vue/jsx";
import '../tsx/tsx'
import { defineProps, defineEmits, useTemplateRef } from "vue";

import { assertType, describe, it } from "vitest";
import { CreateMacroReturn, createMacroReturn } from "../setup/setup";
import { CreateExportedInstanceFromMacro, CreateExportedInstanceFromNormalisedMacro, PublicInstanceFromMacro } from "./instance";

describe("Instance Types", () => {
  describe("PublicInstanceFromMacro", () => {
    it("basic", () => {
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
          withDefaults: {
            value: { foo: "default foo" };
            type: { foo: string };
          };

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

      const TT = {} as T;

      TT.$props.foo = undefined;
      TT.$props.foo = "";
      // @ts-expect-error wrong type
      TT.$props.foo = 123;
      TT.$emit("update", 123);
      // @ts-expect-error wrong type
      TT.$emit("update:modelValue", 123);

      assertType<string>(TT.$data.someData);
      // @ts-expect-error does not exist
      TT.$data.foo;
      // @ts-expect-error does not exist
      TT.$data.a;

      TT.$refs.someRef.value?.focus();

      TT.$slots.default()?.toLowerCase();
      // @ts-expect-error does not exist
      TT.$slots.foo();

      assertType<boolean>(TT.$options.customOption);
      assertType<(() => void) | (() => void)[] | undefined>(
        TT.$options.beforeUpdate
      );

      TT.$.attrs;
      // @ts-expect-error
      TT.$.attrs.foo;

      assertType<number>(TT.$.uid);

      assertType<string>(TT.$.proxy.$.proxy!.$data.someData);
      assertType<string>(TT.$.proxy.$data.someData);

      // very very deep access while maintaining types
      TT.$.proxy.$props.foo;
      TT.$.proxy.$.props.foo;
      TT.$.proxy.$.proxy.$props.foo;
      TT.$.proxy.$.proxy.$.props.foo;
      TT.$.proxy.$.proxy.$.proxy.$props.foo;

      assertType<string | undefined>(TT.$.proxy.$.proxy.$.proxy.$props.foo);
      // @ts-expect-error wrong type
      assertType<number>(TT.$.proxy.$.proxy.$.proxy.$props.foo);
    });

    it("generic", () => {
      function setup<T extends string>() {
        type Props = { value: T; onChange: (e: T) => void };
        const props = defineProps<Props>();
        const emit = defineEmits<{
          bar: [e: T];
          baz: [e: number];
        }>();

        return createMacroReturn({
          props: { value: props, type: {} as Props },
          emits: {
            value: emit,
            type: {} as {
              bar: [e: T];
              baz: [e: number];
            },
          },
        });
      }

      const Comp = {} as {
        new <T extends string>(): PublicInstanceFromMacro<
          ReturnType<typeof setup<T>>,
          {},
          HTMLElement,
          false,
          false
        > extends infer I
          ? I & {
              $props: {
                "v-slot"?: (el: I) => any;
              };
            }
          : never;
      };

      <>
        <Comp
          value="foo"
          onChange={(e) => {
            // @ts-expect-error
            e === "a";
          }}
        />
      </>;
    });
  });

  describe("CreateExportedInstanceFromMacro", () => {
    it("generic", () => {
      function setup<T extends string>() {
        type Props = { value: T; onChange: (e: T) => void };
        const props = defineProps<Props>();
        const emit = defineEmits<{
          bar: [e: T];
          baz: [e: number];
        }>();

        return createMacroReturn({
          props: { value: props, type: {} as Props },
          emits: {
            value: emit,
            type: {} as {
              bar: [e: T];
              baz: [e: number];
            },
          },
        });
      }

      const Comp = {} as {
        new <T extends string>(): CreateExportedInstanceFromMacro<
          ReturnType<typeof setup<T>>,
          {},
          HTMLElement,
          false
        >
      };
      <>
        <Comp
          value="foo"
          onChange={(e) => {
            // @ts-expect-error
            e === "a";
          }}
          v-slot={e=> {
            // @ts-expect-error wrong type
            e.$props.value === 'a'

            return {}
          }}
        />
      </>;
    });
  });
});
