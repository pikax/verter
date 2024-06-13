import { compileScript, parse } from "@vue/compiler-sfc";
import { LocationType } from "../../types.js";
import SlotsPlugin from "./index.js";

describe("Slots plugin", () => {
  it("sanitise value", () => {
    expect(SlotsPlugin).toEqual({
      name: "Slots",
      walk: expect.any(Function),
    });
  });
  describe("walk", () => {
    describe("undefined", () => {
      test("isSetup: false", () => {
        expect(
          // @ts-expect-error not the right type
          SlotsPlugin.walk(undefined, { isSetup: false })
        ).toBeUndefined();
      });
      test("empty node", () => {
        // @ts-expect-error empty node
        expect(SlotsPlugin.walk({}, { isSetup: true })).toBeUndefined();
      });
    });

    describe("ExpressionStatement", () => {
      test("empty", () => {
        expect(
          SlotsPlugin.walk(
            {
              type: "ExpressionStatement",
              // @ts-expect-error not the right type
              expression: {},
            },
            { isSetup: true }
          )
        ).toBeUndefined();
      });

      test("different callee", () => {
        expect(
          SlotsPlugin.walk(
            {
              type: "ExpressionStatement",
              expression: {
                type: "CallExpression",
                // @ts-expect-error not the right type
                callee: {
                  name: "defineSlot",
                },
              },
            },
            {
              isSetup: true,
              script: {
                loc: {
                  source: "",
                },
              },
            }
          )
        ).toBeUndefined();
      });

      test("empty defineSlots", () => {
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineSlots",
          },
          start: 0,
          end: "defineSlots()".length,
          arguments: [],
        };
        expect(
          SlotsPlugin.walk(
            {
              type: "ExpressionStatement",
              // @ts-expect-error
              expression,
            },
            {
              isSetup: true,
              script: {
                loc: {
                  source: "defineSlots()",
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Slots,
            generated: false,
            content: "defineSlots()",
            expression,
            varName: undefined,
          },
        ]);
      });

      test("type defineSlots", () => {
        const type = "{ foo: string; bar: number; }";
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineSlots",
          },
          arguments: [],
          typeParameters: {
            type: "TSExpressionWithtypeParameters",
            params: [
              {
                start: "defineSlots<".length,
                end: "defineSlots<".length + type.length,
              },
            ],
          },
        };
        expect(
          SlotsPlugin.walk(
            {
              type: "ExpressionStatement",
              // @ts-expect-error
              expression,
            },
            {
              isSetup: true,
              script: {
                loc: {
                  source: `defineSlots<${type}>();`,
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Slots,
            generated: false,
            content: "defineSlots<{ foo: string; bar: number; }>()",
            expression,
            varName: undefined,
          },
        ]);
      });

      test("defineSlots array", () => {
        const code = `defineSlots(['foo', 'bar'])`;
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineSlots",
          },
          start: 0,
          end: code.length,
          arguments: [
            {
              type: "ObjectExpression",
              start: code.indexOf("{"),
              end: code.indexOf("}") + 1,
              properties: [
                {
                  type: "ObjectProperty",
                  key: {
                    type: "Identifier",
                    name: "default",
                  },
                  value: {
                    type: "BooleanLiteral",
                    value: true,
                  },
                },
              ],
            },
          ],
        };

        // const argumentExpression = expression.arguments[0];
        expect(
          SlotsPlugin.walk(
            {
              type: "ExpressionStatement",
              // @ts-expect-error
              expression,
            },
            {
              isSetup: true,
              script: {
                loc: {
                  source: code,
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Slots,
            generated: false,
            content: code,
            expression,
            varName: undefined,
          },
        ]);
      });

      test("defineSlots Options undefined", () => {
        const code = `defineSlots({ default: true })`;
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineSlots",
          },
          start: 0,
          end: code.length,
          arguments: [
            {
              type: "ObjectExpression",
              start: code.indexOf("{"),
              end: code.indexOf("}") + 1,
              properties: [
                {
                  type: "ObjectProperty",
                  key: {
                    type: "Identifier",
                    name: "default",
                  },
                  value: {
                    type: "BooleanLiteral",
                    value: true,
                  },
                },
              ],
            },
          ],
        };

        expect(
          SlotsPlugin.walk(
            {
              type: "ExpressionStatement",
              // @ts-expect-error
              expression,
            },
            {
              isSetup: true,
              script: {
                loc: {
                  source: code,
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Slots,
            generated: false,
            content: code,
            expression,
            varName: undefined,
          },
        ]);
      });

      it("test with parse", () => {
        const parsed = compileScript(
          parse(`
    <script setup lang="ts">
defineSlots<{ foo: (test: string)=> any}>();
</script>
<template>
  <span>1</span>
</template>
`).descriptor,
          {
            id: "random-id",
          }
        );

        const parseAst = parsed.scriptSetupAst!;

        expect(
          SlotsPlugin.walk(parseAst[0], {
            isSetup: true,
            script: parsed,
          } as any)
        ).toEqual([
          {
            type: LocationType.Slots,
            generated: false,
            content: "defineSlots<{ foo: (test: string)=> any}>()",
            expression: expect.any(Object),
            varName: undefined,
          },
        ]);
      });

      // TODO add example with type override
      // test("defineModel full", () => {});
    });
  });
});
