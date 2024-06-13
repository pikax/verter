import { compileScript, parse } from "@vue/compiler-sfc";
import { LocationType } from "../../types.js";
import PropsPlugin from "./index.js";

describe("Props plugin", () => {
  it("sanitise value", () => {
    expect(PropsPlugin).toEqual({
      name: "Props",
      walk: expect.any(Function),
    });
  });
  describe("walk", () => {
    describe("undefined", () => {
      test("isSetup: false", () => {
        expect(
          // @ts-expect-error not the right type
          PropsPlugin.walk(undefined, { isSetup: false })
        ).toBeUndefined();
      });
      test("empty node", () => {
        // @ts-expect-error empty node
        expect(PropsPlugin.walk({}, { isSetup: true })).toBeUndefined();
      });
    });

    describe("ExpressionStatement", () => {
      test("empty", () => {
        expect(
          PropsPlugin.walk(
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
          PropsPlugin.walk(
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

      test("empty defineProps", () => {
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineProps",
          },
          start: 0,
          end: "defineProps()".length,
          arguments: [],
        };
        expect(
          PropsPlugin.walk(
            {
              type: "ExpressionStatement",
              // @ts-expect-error
              expression,
            },
            {
              isSetup: true,
              script: {
                loc: {
                  source: "defineProps()",
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Props,
            generated: false,
            content: "defineProps()",
            expression,
            varName: undefined,
          },
        ]);
      });

      test("type defineProps", () => {
        const type = "{ foo: string; bar: number; }";
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineProps",
          },
          arguments: [],
          typeParameters: {
            type: "TSExpressionWithTypeParameters",
            params: [
              {
                start: "defineProps<".length,
                end: `defineProps<${type}>()`.length,
              },
            ],
          },
        };
        expect(
          PropsPlugin.walk(
            {
              type: "ExpressionStatement",
              // @ts-expect-error
              expression,
            },
            {
              isSetup: true,
              script: {
                loc: {
                  source: `defineProps<${type}>();`,
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Props,
            generated: false,
            content: `defineProps<${type}>()`,
            expression,
            varName: undefined,
          },
        ]);
      });

      test("type defineProps + generic", () => {
        const type = "{ foo: string; bar: number; }";
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineProps",
          },
          arguments: [],
          typeParameters: {
            type: "TSExpressionWithTypeParameters",
            params: [
              {
                start: "defineProps<".length,
                end: `defineProps<${type}`.length,
              },
            ],
          },
        };
        expect(
          PropsPlugin.walk(
            {
              type: "ExpressionStatement",
              // @ts-expect-error
              expression,
            },
            {
              isSetup: true,
              generic: true,
              script: {
                loc: {
                  source: `defineProps<${type}>();`,
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Props,
            generated: false,
            node: expression.typeParameters.params[0],
            content: type,
            expression,
            varName: undefined,
          },
        ]);
      });

      test("defineProps array", () => {
        const code = `defineProps(['foo', 'bar'])`;
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineProps",
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
          PropsPlugin.walk(
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
            type: LocationType.Props,
            generated: false,
            content: code,
            expression,
            varName: undefined,
          },
        ]);
      });

      test("defineProps Options", () => {
        const code = `defineProps({ default: true })`;
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineProps",
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
          PropsPlugin.walk(
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
            type: LocationType.Props,
            generated: false,
            content: code,
            expression,
            varName: undefined,
          },
        ]);
      });

      // describe("withDefaults", () => {});

      it("test with parse", () => {
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

        const parseAst = parsed.scriptSetupAst!;

        expect(
          PropsPlugin.walk(parseAst[0], {
            isSetup: true,
            script: parsed,
          } as any)
        ).toMatchObject([
          {
            type: LocationType.Props,
            generated: false,
            content: `defineProps(['foo'])`,
            varName: undefined,
          },
        ]);
      });

      // TODO add example with type override
      // test("defineModel full", () => {});
    });
  });
});
