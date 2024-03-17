import { MagicString, compileScript, parse } from "@vue/compiler-sfc";
import { LocationType } from "../../types.js";
import EmitsPlugin from "./index.js";

describe("Emits plugin", () => {
  it("sanitise value", () => {
    expect(EmitsPlugin).toEqual({
      name: "Emits",
      walk: expect.any(Function),
    });
  });
  describe("walk", () => {
    describe("undefined", () => {
      test("isSetup: false", () => {
        expect(
          // @ts-expect-error not the right type
          EmitsPlugin.walk(undefined, { isSetup: false })
        ).toBeUndefined();
      });
      test("empty node", () => {
        // @ts-expect-error empty node
        expect(EmitsPlugin.walk({}, { isSetup: true })).toBeUndefined();
      });


    });

    describe('sourcemap', () => {
      test('empty', () => {
        const code = "defineEmits()";
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineEmits",
          },
          start: 0,
          end: code.length,
          arguments: [],
        };

        const result = EmitsPlugin.walk(
          {
            type: "ExpressionStatement",
            // @ts-expect-error not the right type
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
        );

        const s = new MagicString(code);

        result.forEach(x => x.applyMap?.(s));

        expect(s.toString()).toBe("const __emits = defineEmits()");
      })

      test("['foo']", () => {
        const code = "defineEmits(['foo'])";
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineEmits",
          },
          start: 0,
          end: code.length,
          arguments: [],
        };

        const result = EmitsPlugin.walk(
          {
            type: "ExpressionStatement",
            // @ts-expect-error not the right type
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
        );

        const s = new MagicString(code);

        result.forEach(x => x.applyMap?.(s));

        expect(s.toString()).toBe("const __emits = defineEmits(['foo'])");
      })

      test('{ foo: String }', () => {
        const code = "defineEmits({ foo: String })";
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineEmits",
          },
          start: 0,
          end: code.length,
          arguments: [],
        };

        const result = EmitsPlugin.walk(
          {
            type: "ExpressionStatement",
            // @ts-expect-error not the right type
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
        );

        const s = new MagicString(code);

        result.forEach(x => x.applyMap?.(s));

        expect(s.toString()).toBe("const __emits = defineEmits({ foo: String })");

      })
    })

    describe("ExpressionStatement", () => {
      test("empty", () => {
        expect(
          EmitsPlugin.walk(
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
          EmitsPlugin.walk(
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

      test("empty defineEmits", () => {
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineEmits",
          },
          start: 0,
          end: "defineEmits()".length,
          arguments: [],
        };
        expect(
          EmitsPlugin.walk(
            {
              type: "ExpressionStatement",
              // @ts-expect-error
              expression,
            },
            {
              isSetup: true,
              script: {
                loc: {
                  source: "defineEmits()",
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Import,
            node: expression,
            generated: true,
            from: "vue",
            items: [
              {
                name: "DeclareEmits",
                // TODO change the import location
                type: true,
              },
            ],
          },
          // create variable with return
          {
            type: LocationType.Declaration,
            node: expression,
            generated: true,

            declaration: {
              name: "__emits",
              content: "defineEmits()",
            },
            applyMap: expect.any(Function),
          },
          // get the type from variable
          {
            type: LocationType.Declaration,
            node: expression,
            generated: true,

            // TODO debug this to check if this is the correct type
            declaration: {
              type: "type",
              name: "Type__emits",
              content: `DeclareEmits<typeof __emits>;`,
            },
          },
          {
            type: LocationType.Emits,
            node: expression,
            content: "Type__emits",
          },
        ]);
      });

      test("type defineEmits", () => {
        const type = "{ foo: string; bar: number; }";
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineEmits",
          },
          arguments: [],
          typeParameters: {
            type: "TSExpressionWithtypeParameters",
            params: [
              {
                start: "defineEmits<".length,
                end: `defineEmits<${type}>()`.length,
              },
            ],
          },
        };
        expect(
          EmitsPlugin.walk(
            {
              type: "ExpressionStatement",
              // @ts-expect-error
              expression,
            },
            {
              isSetup: true,
              script: {
                loc: {
                  source: `defineEmits<${type}>();`,
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Import,
            generated: true,
            node: expression,
            from: "vue",
            items: [
              {
                name: "DeclareEmits",
                type: true,
              },
            ],
          },
          // create variable with return
          {
            type: LocationType.Declaration,
            node: expression,
            generated: true,

            declaration: {
              name: "__emits",
              content: `defineEmits<${type}>()`,
            },
            applyMap: expect.any(Function),
          },
          // get the type from variable
          {
            type: LocationType.Declaration,
            node: expression,
            generated: true,

            declaration: {
              type: "type",
              name: "Type__emits",
              content: `DeclareEmits<typeof __emits>;`,
            },
          },
          {
            type: LocationType.Emits,
            node: expression,
            content: "Type__emits",
          },
        ]);
      });

      test("defineEmits array", () => {
        const code = `defineEmits(['foo', 'bar'])`;
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineEmits",
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
          EmitsPlugin.walk(
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
            generated: true,

            from: "vue",
            items: [
              {
                name: "DeclareEmits",
                type: true,
              },
            ],
          },
          // create variable with return
          {
            type: LocationType.Declaration,
            node: expression,
            generated: true,

            declaration: {
              name: "__emits",
              content: code,
            },
            applyMap: expect.any(Function),

          },
          // get the type from variable
          {
            type: LocationType.Declaration,
            node: expression,
            generated: true,

            declaration: {
              type: "type",
              name: "Type__emits",
              content: `DeclareEmits<typeof __emits>;`,
            },
          },
          {
            type: LocationType.Emits,
            node: expression,
            content: "Type__emits",
          },
        ]);
      });

      test("defineEmits Options", () => {
        const code = `defineEmits({ default: true })`;
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineEmits",
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
          EmitsPlugin.walk(
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
            node: expression, generated: true,

            from: "vue",
            items: [
              {
                name: "DeclareEmits",
                type: true,
              },
            ],
          },
          // create variable with return
          {
            type: LocationType.Declaration,
            node: expression,
            generated: true,

            declaration: {
              name: "__emits",
              content: code,
            },
            applyMap: expect.any(Function),

          },
          // get the type from variable
          {
            type: LocationType.Declaration,
            node: expression,
            generated: true,

            declaration: {
              type: "type",
              name: "Type__emits",
              content: `DeclareEmits<typeof __emits>;`,
            },
          },
          {
            type: LocationType.Emits,
            node: expression,
            content: "Type__emits",
          },
        ]);
      });

      // describe("withDefaults", () => {});

      it("test with parse", () => {
        const parsed = compileScript(
          parse(`
    <script setup lang="ts">
defineEmits(['foo']);
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
          EmitsPlugin.walk(parseAst[0], {
            isSetup: true,
            script: parsed,
          })
        ).toMatchObject([
          {
            from: "vue",
            items: [
              {
                name: "DeclareEmits",
                type: true,
              },
            ],
            type: "import",
          },
          {
            declaration: {
              content: `defineEmits(['foo'])`,
              name: "__emits",
            },
            type: "declaration",
          },
          {
            declaration: {
              content: "DeclareEmits<typeof __emits>;",
              name: "Type__emits",
              type: "type",
            },
            type: "declaration",
          },
          {
            content: "Type__emits",
            type: "emits",
          },
        ]);


        // expect(
        //   EmitsPlugin.walk(parseAst[0], {
        //     isSetup: true,
        //     script: parsed,
        //   })
        // ).toMatchObject(
        //   [
        //     {
        //       "from": "vue",
        //       "generated": true,
        //       "items": [
        //         {
        //           "name": "DeclareEmits",
        //           "type": true,
        //         },
        //       ],
        //       "type": "import",
        //     },
        //     {
        //       "applyMap": [Function],
        //       "declaration": {
        //         "content": "defineEmits(['foo'])",
        //         "name": "__emits",
        //       },
        //       "generated": true,
        //       "type": "declaration",
        //     },
        //     {
        //       "declaration": {
        //         "content": "DeclareEmits<typeof __emits>;",
        //         "name": "Type__emits",
        //         "type": "type",
        //       },
        //       "generated": true,
        //       "type": "declaration",
        //     },
        //     {
        //       "content": "Type__emits",
        //       "type": "emits",
        //     },
        //   ]
        // );

        // for (let i = 0; i < parseAst.length; i++) {
        //   const element = parseAst[i];
        //   checkForSetupMethodCall("defineEmits", element);
        // }
      });

      // TODO add example with type override
      // test("defineModel full", () => {});
    });
  });
});
