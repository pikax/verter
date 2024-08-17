import { compileScript, parse } from "@vue/compiler-sfc";
import { LocationType } from "../../types.js";
import ModelsPlugin, { getModelVarName } from "./models.js";

describe("Props plugin", () => {
  it("sanitise value", () => {
    expect(ModelsPlugin).toEqual({
      name: "Model",
      walk: expect.any(Function),
    });
  });

  describe("walk", () => {
    describe("undefined", () => {
      test("isSetup: false", () => {
        expect(
          // @ts-expect-error not the right type
          ModelsPlugin.walk(undefined, { isSetup: false })
        ).toBeUndefined();
      });
      test("empty node", () => {
        // @ts-expect-error empty node
        expect(ModelsPlugin.walk({}, { isSetup: true })).toBeUndefined();
      });
    });

    describe("ExpressionStatement", () => {
      test("empty", () => {
        expect(
          ModelsPlugin.walk(
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
          ModelsPlugin.walk(
            {
              type: "ExpressionStatement",
              expression: {
                type: "CallExpression",
                // @ts-expect-error not the right type
                callee: {
                  name: "defineProps",
                },
              },
            },
            { isSetup: true }
          )
        ).toBeUndefined();
      });

      test("empty defineModel", () => {
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineModel",
          },
          arguments: [],
        };
        expect(
          ModelsPlugin.walk(
            {
              type: "ExpressionStatement",
              // @ts-expect-error
              expression,
            },
            {
              isSetup: true,
              script: {
                loc: {
                  source: "defineModel() ",
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Model,
            generated: false,
            content: "defineModel()",
            expression,
            varName: undefined,
            modelName: "modelValue",
          },
        ]);
      });

      test("type defineModel", () => {
        const type = "{ foo: string; bar: number; }";
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineModel",
          },
          arguments: [],
          typeParameters: {
            type: "TSExpressionWithtypeParameters",
            params: [
              {
                start: "defineModel<".length,
                end: "defineModel<".length + type.length,
              },
            ],
          },
        };
        expect(
          ModelsPlugin.walk(
            {
              type: "ExpressionStatement",
              // @ts-expect-error
              expression,
            },
            {
              isSetup: true,
              script: {
                loc: {
                  source: "defineModel<{ foo: string; bar: number; }>() ",
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Model,
            content: "defineModel<{ foo: string; bar: number; }>()",
            expression,
            generated: false,
            varName: undefined,
            modelName: "modelValue",
          },
        ]);
      });

      test("named defineModel", () => {
        const name = "amazingModel";
        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineModel",
          },
          arguments: [
            {
              type: "StringLiteral",
              value: `${name}`,
            },
          ],
        };
        expect(
          ModelsPlugin.walk(
            {
              type: "ExpressionStatement",
              // @ts-expect-error
              expression,
            },
            {
              isSetup: true,
              script: {
                loc: {
                  source: `defineModel('${name}') `,
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Model,
            generated: false,
            expression,

            content: `defineModel('${name}')`,

            modelName: name,
            varName: undefined,
          },
        ]);
      });

      test("defineModel Options", () => {
        const code = `defineModel({ default: true })`;
        const name = "modelValue";

        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineModel",
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
          ModelsPlugin.walk(
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
            type: LocationType.Model,
            content: "defineModel({ default: true })",
            expression,

            generated: false,
            modelName: name,
            varName: undefined,
          },
        ]);
      });

      test("defineModel name and options", () => {
        const code = `defineModel('amazingModel', { default: true })`;
        const name = "amazingModel";

        const expression = {
          type: "CallExpression",
          callee: {
            name: "defineModel",
          },
          start: 0,
          end: code.length,
          arguments: [
            {
              type: "StringLiteral",
              value: `${name}`,
            },
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
          ModelsPlugin.walk(
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
            type: LocationType.Model,
            generated: false,
            expression,
            content: code,
            varName: undefined,
            modelName: name,
          },
        ]);
      });

      it("test with parse", () => {
        const parsed = compileScript(
          parse(`
    <script setup lang="ts">
const fooModel = defineModel('foo');
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
          ModelsPlugin.walk(parseAst[0], {
            isSetup: true,
            script: parsed,
          } as any)
        ).toMatchObject([
          {
            type: LocationType.Model,
            generated: false,
            content: `defineModel('foo')`,
            varName: "fooModel",
            modelName: "foo",
          },
        ]);
      });
    });
  });
});
