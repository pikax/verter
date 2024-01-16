import { compileScript, parse } from "@vue/compiler-sfc";
import GenericPlugin from "./index.js";
import { LocationType } from "../types.js";

describe("Generic plugin", () => {
  it("walk should be undefined", () => {
    // @ts-expect-error
    expect(GenericPlugin.walk).toBe(undefined);
  });
  describe("process", () => {
    it("non generic", () => {
      const parsed = compileScript(
        parse(`
          <script setup lang="ts">
      defineProps(['foo']);
      </script>
      <template>
        <span>1</span>
      </template>
      `).descriptor,
        {
          id: "random-id",
        }
      );

      expect(
        GenericPlugin.process({
          script: parsed.scriptSetupAst!,
          generic: parsed.attrs.generic,
        })
      ).toBe(undefined);
    });

    test.each([
      ["T", [{ name: "T", content: "T", constraint: undefined, index: 0 }]],
      [
        "T, A",
        [
          { name: "T", content: "T", constraint: undefined, index: 0 },
          { name: "A", content: "A", constraint: undefined, index: 1 },
        ],
      ],
      [
        "T extends string, A = number",
        [
          {
            name: "T",
            content: "T extends string",
            constraint: "string",
            index: 0,
          },
          {
            name: "A",
            content: "A = number",
            constraint: undefined,
            default: "number",
            index: 1,
          },
        ],
      ],
      [
        "Clearable extends boolean, ValueType extends string | number | null | undefined",
        [
          {
            name: "Clearable",
            content: "Clearable extends boolean",
            constraint: "boolean",
            index: 0,
          },
          {
            name: "ValueType",
            content: "ValueType extends string | number | null | undefined",
            constraint: "string | number | null | undefined",
            index: 1,
          },
        ],
      ],
      [
        "T extends MyInterface",
        [
          {
            name: "T",
            content: "T extends MyInterface",
            constraint: "MyInterface",
            index: 0,
          },
        ],
      ],
    ])("%s to result correctly", (generic, items) => {
      expect(GenericPlugin.process({ generic } as any)).toEqual({
        type: LocationType.Generic,
        node: undefined,
        items,
      });
    });

    it("test generic", () => {
      const generic = "T extends string";
      // const generic = "T extends string, A = number";

      expect(GenericPlugin.process({ generic })).toEqual({
        type: LocationType.Generic,
        node: undefined,
        items: [
          {
            name: "T",
            content: "T extends string",
            constraint: "string",
            index: 0,
          },
        ],
      });
    });
  });
});
