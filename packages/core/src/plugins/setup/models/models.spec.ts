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
            { isSetup: true, script: { loc: {} } }
          )
        ).toEqual([
          {
            type: LocationType.Emits,
            node: expression,
            properties: [
              {
                name: `'update:modelValue'`,
                content: "any",
              },
            ],
          },
          {
            type: LocationType.Props,
            node: expression,
            properties: [
              {
                name: "modelValue",
                content: "any",
              },
            ],
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
                  source: "defineModel<{ foo: string; bar: number; }>()",
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Emits,
            node: expression,
            properties: [
              {
                name: `'update:modelValue'`,
                content: type,
              },
            ],
          },
          {
            type: LocationType.Props,
            node: expression,
            properties: [
              {
                name: "modelValue",
                content: type,
              },
            ],
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
                  source: `defineModel('${name}')`,
                },
              },
            }
          )
        ).toEqual([
          {
            type: LocationType.Emits,
            node: expression,
            properties: [
              {
                name: `'update:${name}'`,
                content: "any",
              },
            ],
          },
          {
            type: LocationType.Props,
            node: expression,
            properties: [
              {
                name: name,
                content: "any",
              },
            ],
          },
        ]);
      });

      test("defineModel Options", () => {
        const code = `defineModel({ default: true })`;
        const name = "modelValue";

        const varName = getModelVarName(name);
        const typeName = `TYPE_${varName}`;

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

        const argumentExpression = expression.arguments[0];
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
            type: LocationType.Import,
            generated: true,
            node: argumentExpression,
            // TODO change the import location
            from: "<helpers>",
            items: [
              {
                name: "ExtractModelType",
                type: true,
              },
            ],
          },
          // get the variable from defineModel
          {
            type: LocationType.Declaration,
            node: argumentExpression,
            generated: true,

            declaration: {
              name: varName,
              content: code,
            },
          },
          // get the type from the variable
          {
            type: LocationType.Declaration,
            generated: true,
            node: argumentExpression,
            declaration: {
              name: typeName,
              type: "type",
              content: `ExtractModelType<typeof ${varName}>;`,
            },
          },
          {
            type: LocationType.Emits,
            generated: true,
            node: argumentExpression,
            properties: [
              {
                name: `'update:${name}'`,
                content: typeName,
              },
            ],
          },
          {
            type: LocationType.Props,
            generated: true,
            node: argumentExpression,
            properties: [
              {
                name: name,
                content: typeName,
              },
            ],
          },
        ]);
      });

      test("defineModel name and options", () => {
        const code = `defineModel('amazingModel', { default: true })`;
        const name = "amazingModel";

        const varName = getModelVarName(name);
        const typeName = `TYPE_${varName}`;

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

        const argumentExpression = expression.arguments[1];
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
            type: LocationType.Import,
            node: argumentExpression,
            generated: true,
            // TODO change the import location
            from: "<helpers>",
            items: [
              {
                name: "ExtractModelType",
                type: true,
              },
            ],
          },
          // get the variable from defineModel
          {
            type: LocationType.Declaration,
            node: argumentExpression,
            generated: true,

            declaration: {
              name: varName,
              content: code,
            },
          },
          // get the type from the variable
          {
            type: LocationType.Declaration,
            generated: true,
            node: argumentExpression,
            declaration: {
              name: typeName,
              type: "type",
              content: `ExtractModelType<typeof ${varName}>;`,
            },
          },
          {
            type: LocationType.Emits,
            node: argumentExpression,
            generated: true,
            properties: [
              {
                name: `'update:${name}'`,
                content: typeName,
              },
            ],
          },
          {
            type: LocationType.Props,
            generated: true,
            node: argumentExpression,
            properties: [
              {
                name: name,
                content: typeName,
              },
            ],
          },
        ]);
      });

      // TODO add example with type override
      // test("defineModel full", () => {});
    });
  });
});
