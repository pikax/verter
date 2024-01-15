import { compileScript, parse } from "@vue/compiler-sfc";
import { LocationType } from "../types.js";
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
            type: LocationType.Import,
            node: expression,
            // TODO change the import location
            from: "vue",
            items: [
              {
                name: "ExtractPropTypes",
                type: true,
              },
            ],
          },
          // create variable with return
          {
            type: LocationType.Declaration,
            node: expression,

            declaration: {
              name: "__props",
              content: "defineProps()",
            },
          },
          // get the type from variable
          {
            type: LocationType.Declaration,
            node: expression,

            // TODO debug this to check if this is the correct type
            declaration: {
              type: "type",
              name: "Type__props",
              content: `ExtractPropTypes<typeof __props>;`,
            },
          },
          {
            type: LocationType.Props,
            node: expression,
            content: "Type__props",
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
            type: LocationType.Import,
            node: expression,
            // TODO change the import location
            from: "vue",
            items: [
              {
                name: "ExtractPropTypes",
                type: true,
              },
            ],
          },
          // create variable with return
          {
            type: LocationType.Declaration,
            node: expression,

            declaration: {
              name: "__props",
              content: `defineProps<${type}>()`,
            },
          },
          // get the type from variable
          {
            type: LocationType.Declaration,
            node: expression,

            // TODO debug this to check if this is the correct type
            declaration: {
              type: "type",
              name: "Type__props",
              content: `ExtractPropTypes<typeof __props>;`,
            },
          },
          {
            type: LocationType.Props,
            node: expression,
            content: "Type__props",
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
          // {
          //   type: LocationType.Import,
          //   node: expression,
          //   // TODO change the import location
          //   from: "vue",
          //   items: [
          //     {
          //       name: "ExtractPropTypes",
          //       type: true,
          //     },
          //   ],
          // },
          // // create variable with return
          // {
          //   type: LocationType.Declaration,
          //   node: expression,

          //   declaration: {
          //     name: "__props",
          //     content: `defineProps<${type}>()`,
          //   },
          // },
          // // get the type from variable
          // {
          //   type: LocationType.Declaration,
          //   node: expression,

          //   // TODO debug this to check if this is the correct type
          //   declaration: {
          //     type: "type",
          //     name: "Type__props",
          //     content: `ExtractPropTypes<typeof __props>;`,
          //   },
          // },
          {
            type: LocationType.Props,
            node: expression.typeParameters.params[0],
            content: type,
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
            type: LocationType.Import,
            node: expression,
            // TODO change the import location
            from: "vue",
            items: [
              {
                name: "ExtractPropTypes",
                type: true,
              },
            ],
          },
          // create variable with return
          {
            type: LocationType.Declaration,
            node: expression,

            declaration: {
              name: "__props",
              content: code,
            },
          },
          // get the type from variable
          {
            type: LocationType.Declaration,
            node: expression,

            // TODO debug this to check if this is the correct type
            declaration: {
              type: "type",
              name: "Type__props",
              content: `ExtractPropTypes<typeof __props>;`,
            },
          },
          {
            type: LocationType.Props,
            node: expression,
            content: "Type__props",
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
            type: LocationType.Import,
            node: expression,
            // TODO change the import location
            from: "vue",
            items: [
              {
                name: "ExtractPropTypes",
                type: true,
              },
            ],
          },
          // create variable with return
          {
            type: LocationType.Declaration,
            node: expression,

            declaration: {
              name: "__props",
              content: code,
            },
          },
          // get the type from variable
          {
            type: LocationType.Declaration,
            node: expression,

            // TODO debug this to check if this is the correct type
            declaration: {
              type: "type",
              name: "Type__props",
              content: `ExtractPropTypes<typeof __props>;`,
            },
          },
          {
            type: LocationType.Props,
            node: expression,
            content: "Type__props",
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
          })
        ).toMatchObject([
          {
            from: "vue",
            items: [
              {
                name: "ExtractPropTypes",
                type: true,
              },
            ],
            type: "import",
          },
          {
            declaration: {
              content: `defineProps(['foo'])`,
              name: "__props",
            },
            type: "declaration",
          },
          {
            declaration: {
              content: "ExtractPropTypes<typeof __props>;",
              name: "Type__props",
              type: "type",
            },
            type: "declaration",
          },
          {
            content: "Type__props",
            type: "props",
          },
        ]);

        // for (let i = 0; i < parseAst.length; i++) {
        //   const element = parseAst[i];
        //   checkForSetupMethodCall("defineProps", element);
        // }
      });

      // TODO add example with type override
      // test("defineModel full", () => {});
    });
  });
});
