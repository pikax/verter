import "vue/jsx";
import { defineProps, defineEmits } from "vue";

import { describe, it } from "vitest";
import { createMacroReturn } from "../setup/setup";
import { PublicInstanceFromMacro } from "./instance";

describe("Instance Types", () => {
  describe("PublicInstanceFromMacro", () => {
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

          // withDefaults: {
          //   value: {
          //     foo: "default foo",
          //   },
          //   // type: { foo: "default foo"}
          // }
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
      const c = new Comp<"foo">();
      c.$props;

      <>
        <Comp
          value="foo"
          onChange={(e) => {
            // @ts-expect-error
            e === "a";
          }}
          v-slot={(instance) => {
            //@ts-expect-error
            instance.$props.value === "a";
          }}
        />
      </>;
    });
  });
});
