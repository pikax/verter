import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";

import { MacrosPlugin } from "./index.js";
import { TemplateBindingPlugin } from "../template-binding";
import { ScriptBlockPlugin } from "../script-block";
import { BindingPlugin } from "../binding";
import { testSourceMaps } from "../../../../utils.test-utils";

describe("process script plugin script block", () => {
  function _parse(
    content: string,
    wrapper: string | boolean = false,
    lang = "js",

    pre = "",
    post = ""
  ) {
    const prepend = `${pre}<script ${
      wrapper === false ? "setup" : ""
    } lang="${lang}">`;
    const source = `${prepend}${content}</script>${post}`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const scriptBlock = parsed.blocks.find(
      (x) => x.type === "script"
    ) as ParsedBlockScript;

    const r = processScript(
      scriptBlock.result.items,
      [MacrosPlugin, TemplateBindingPlugin, ScriptBlockPlugin, BindingPlugin],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        isSetup: wrapper === false,
        block: scriptBlock,
        blockNameResolver: (name) => name,
      }
    );

    return r;
  }

  describe("setup", () => {
    function parse(content: string, lang = "ts", pre: string = "") {
      return _parse(`${pre ? pre + "\n" : ""}${content}`, false, lang, pre);
    }

    // TODO add tests for javascript

    describe("defineModel", () => {
      describe("function & variable", () => {
        it("defineModel()", () => {
          const { result, context } = parse(`defineModel()`);
          expect(result).toContain(
            `const ___VERTER___models_modelValue=defineModel()`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",

                name: "modelValue",
                varName: "___VERTER___models_modelValue",
                isType: false,
                valueName: "___VERTER___models_modelValue",
                typeName: undefined,
                objectName: undefined,
              }),
            ])
          );
        });

        it("defineModel({})", () => {
          const { result, context } = parse(`defineModel({})`);

          expect(result).toContain(
            `const ___VERTER___modelValue_defineModel_Boxed=___VERTER___defineModel_Box({})`
          );
          expect(result).toContain(
            `const ___VERTER___models_modelValue=defineModel(___VERTER___modelValue_defineModel_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",

                name: "modelValue",
                varName: "___VERTER___models_modelValue",
                isType: false,
                valueName: "___VERTER___models_modelValue",
                typeName: undefined,
                objectName: "___VERTER___modelValue_defineModel_Boxed",
              }),
            ])
          );
        });

        it('defineModel("model")', () => {
          const { result, context } = parse(`defineModel("model")`);

          expect(result).toContain(
            `const ___VERTER___model_defineModel_Boxed=___VERTER___defineModel_Box("model")`
          );
          expect(result).toContain(
            `const ___VERTER___models_model=defineModel(___VERTER___model_defineModel_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",
                name: "model",
                varName: "___VERTER___models_model",

                isType: false,
                valueName: "___VERTER___models_model",
                typeName: undefined,
                objectName: "___VERTER___model_defineModel_Boxed",
              }),
            ])
          );
        });

        it('defineModel("model", {})', () => {
          const { result, context } = parse(`defineModel("model", {})`);

          expect(result).toContain(
            `const ___VERTER___model_defineModel_Boxed=___VERTER___defineModel_Box("model", {})`
          );

          expect(result).toContain(
            `const ___VERTER___models_model=defineModel(___VERTER___model_defineModel_Boxed[0],___VERTER___model_defineModel_Boxed[1])`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",
                name: "model",
                varName: "___VERTER___models_model",

                isType: false,
                valueName: "___VERTER___models_model",
                typeName: undefined,
                objectName: "___VERTER___model_defineModel_Boxed",
              }),
            ])
          );
        });
        it("const model = defineModel()", () => {
          const { result, context } = parse(`const model = defineModel()`);
          expect(result).toContain(`const model = defineModel()`);

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",

                name: "modelValue",
                varName: "model",
                isType: false,
                valueName: "model",
                typeName: undefined,
                objectName: undefined,
              }),
            ])
          );
        });

        it("const model = defineModel({})", () => {
          const { result, context } = parse(`const model = defineModel({})`);

          expect(result).toContain(
            `const ___VERTER___modelValue_defineModel_Boxed=___VERTER___defineModel_Box({})`
          );
          expect(result).toContain(
            `const model = defineModel(___VERTER___modelValue_defineModel_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",

                name: "modelValue",
                varName: "model",
                isType: false,
                valueName: "model",
                typeName: undefined,
                objectName: "___VERTER___modelValue_defineModel_Boxed",
              }),
            ])
          );
        });

        it('const model = defineModel("model")', () => {
          const { result, context } = parse(
            `const model = defineModel("model")`
          );

          expect(result).toContain(
            `const ___VERTER___model_defineModel_Boxed=___VERTER___defineModel_Box("model")`
          );

          expect(result).toContain(
            `const model = defineModel(___VERTER___model_defineModel_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",
                name: "model",
                varName: "model",

                isType: false,
                valueName: "model",
                typeName: undefined,
                objectName: "___VERTER___model_defineModel_Boxed",
              }),
            ])
          );
        });

        it('const model = defineModel("model", {})', () => {
          const { result, context } = parse(
            `const model = defineModel("model", {})`
          );
          expect(result).toContain(
            `const ___VERTER___model_defineModel_Boxed=___VERTER___defineModel_Box("model", {})`
          );
          expect(result).toContain(
            `const model = defineModel(___VERTER___model_defineModel_Boxed[0],___VERTER___model_defineModel_Boxed[1])`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",
                name: "model",
                varName: "model",

                isType: false,
                valueName: "model",
                typeName: undefined,
                objectName: "___VERTER___model_defineModel_Boxed",
              }),
            ])
          );
        });
      });

      describe("types", () => {
        it("defineModel<string>()", () => {
          const { result, context } = parse(`defineModel<string>()`);

          expect(result).toContain(
            `type ___VERTER___modelValue_defineModel_Type=string`
          );
          expect(result).toContain(
            `const ___VERTER___models_modelValue=defineModel<___VERTER___modelValue_defineModel_Type>()`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",

                name: "modelValue",
                varName: "___VERTER___models_modelValue",
                isType: true,
                valueName: "___VERTER___models_modelValue",
                typeName: "___VERTER___modelValue_defineModel_Type",
                objectName: undefined,
              }),
            ])
          );
        });
        it("const model = defineModel<string>()", () => {
          const { result, context } = parse(
            `const model = defineModel<string>()`
          );

          expect(result).toContain(
            `type ___VERTER___modelValue_defineModel_Type=string`
          );
          expect(result).toContain(
            `const model = defineModel<___VERTER___modelValue_defineModel_Type>()`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",

                name: "modelValue",
                varName: "model",
                isType: true,
                valueName: "model",
                typeName: "___VERTER___modelValue_defineModel_Type",
                objectName: undefined,
              }),
            ])
          );
        });
        it("defineModel<string>({})", () => {
          const { result, context } = parse(`defineModel<string>({})`);

          expect(result).toContain(
            `type ___VERTER___modelValue_defineModel_Type=string`
          );
          expect(result).toContain(
            `const ___VERTER___modelValue_defineModel_Boxed=___VERTER___defineModel_Box({})`
          );
          expect(result).toContain(
            `const ___VERTER___models_modelValue=defineModel<___VERTER___modelValue_defineModel_Type>(___VERTER___modelValue_defineModel_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",

                name: "modelValue",
                varName: "___VERTER___models_modelValue",
                isType: true,
                valueName: "___VERTER___models_modelValue",
                typeName: "___VERTER___modelValue_defineModel_Type",
                objectName: "___VERTER___modelValue_defineModel_Boxed",
              }),
            ])
          );
        });
        it("const model = defineModel<string>({})", () => {
          const { result, context } = parse(
            `const model = defineModel<string>({})`
          );
          expect(result).toContain(
            `type ___VERTER___modelValue_defineModel_Type=string`
          );
          expect(result).toContain(
            `const ___VERTER___modelValue_defineModel_Boxed=___VERTER___defineModel_Box({})`
          );
          expect(result).toContain(
            `const model = defineModel<___VERTER___modelValue_defineModel_Type>(___VERTER___modelValue_defineModel_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",

                name: "modelValue",
                varName: "model",
                isType: true,
                valueName: "model",
                typeName: "___VERTER___modelValue_defineModel_Type",
                objectName: "___VERTER___modelValue_defineModel_Boxed",
              }),
            ])
          );
        });
        it('defineModel<string>("model")', () => {
          const { result, context } = parse(`defineModel<string>("model")`);

          expect(result).toContain(
            `type ___VERTER___model_defineModel_Type=string`
          );
          expect(result).toContain(
            `const ___VERTER___model_defineModel_Boxed=___VERTER___defineModel_Box("model")`
          );
          expect(result).toContain(
            `const ___VERTER___models_model=defineModel<___VERTER___model_defineModel_Type>(___VERTER___model_defineModel_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",
                name: "model",
                varName: "___VERTER___models_model",

                isType: true,
                valueName: "___VERTER___models_model",
                typeName: "___VERTER___model_defineModel_Type",
                objectName: "___VERTER___model_defineModel_Boxed",
              }),
            ])
          );
        });
        it('const model = defineModel<string>("model")', () => {
          const { result, context } = parse(
            `const model = defineModel<string>("model")`
          );

          expect(result).toContain(
            `type ___VERTER___model_defineModel_Type=string`
          );
          expect(result).toContain(
            `const ___VERTER___model_defineModel_Boxed=___VERTER___defineModel_Box("model")`
          );

          expect(result).toContain(
            `const model = defineModel<___VERTER___model_defineModel_Type>(___VERTER___model_defineModel_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",
                name: "model",
                varName: "model",

                isType: true,
                valueName: "model",
                typeName: "___VERTER___model_defineModel_Type",
                objectName: "___VERTER___model_defineModel_Boxed",
              }),
            ])
          );
        });
        it('defineModel<string>("model", {})', () => {
          const { result, context } = parse(`defineModel<string>("model", {})`);

          expect(result).toContain(
            `type ___VERTER___model_defineModel_Type=string`
          );
          expect(result).toContain(
            `const ___VERTER___model_defineModel_Boxed=___VERTER___defineModel_Box("model", {})`
          );

          expect(result).toContain(
            `const ___VERTER___models_model=defineModel<___VERTER___model_defineModel_Type>(___VERTER___model_defineModel_Boxed[0],___VERTER___model_defineModel_Boxed[1])`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",
                name: "model",
                varName: "___VERTER___models_model",

                isType: true,
                valueName: "___VERTER___models_model",
                typeName: "___VERTER___model_defineModel_Type",
                objectName: "___VERTER___model_defineModel_Boxed",
              }),
            ])
          );
        });
        it('const model = defineModel<string>("model", {})', () => {
          const { result, context } = parse(
            `const model = defineModel<string>("model", {})`
          );
          expect(result).toContain(
            `type ___VERTER___model_defineModel_Type=string`
          );
          expect(result).toContain(
            `const ___VERTER___model_defineModel_Boxed=___VERTER___defineModel_Box("model", {})`
          );
          expect(result).toContain(
            `const model = defineModel<___VERTER___model_defineModel_Type>(___VERTER___model_defineModel_Boxed[0],___VERTER___model_defineModel_Boxed[1])`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",
                name: "model",
                varName: "model",

                isType: true,
                valueName: "model",
                typeName: "___VERTER___model_defineModel_Type",
                objectName: "___VERTER___model_defineModel_Boxed",
              }),
            ])
          );
        });
      });
    });

    describe("defineOptions", () => {
      it("defineOptions({})", () => {
        const { result, context } = parse(`defineOptions({})`);
        expect(result).toContain(`defineOptions({})`);

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "options",
              expression: expect.objectContaining({
                type: "ObjectExpression",
                properties: [],
              }),
            }),
          ])
        );
      });
      it("const foo = defineOptions({})", () => {
        const { result, context } = parse(`const foo = defineOptions({})`);
        expect(result).toContain(`const foo = defineOptions({})`);

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "options",
              expression: expect.objectContaining({
                type: "ObjectExpression",
                properties: [],
              }),
            }),
          ])
        );
      });

      it("defineOptions({ a: 0 })", () => {
        const { result, context } = parse(`defineOptions({ a: 0 })`);
        expect(result).toContain(`defineOptions({ a: 0 })`);

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "options",
              expression: expect.objectContaining({
                type: "ObjectExpression",
                properties: [
                  expect.objectContaining({
                    type: "Property",
                    key: expect.objectContaining({
                      type: "Identifier",
                      name: "a",
                    }),
                    value: expect.objectContaining({
                      type: "Literal",
                      value: 0,
                    }),
                  }),
                ],
              }),
            }),
          ])
        );
      });

      it("defineOptions(myOptions)", () => {
        const { result, context } = parse(`defineOptions(myOptions)`);
        expect(result).toContain(`defineOptions(myOptions)`);

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "options",
              expression: expect.objectContaining({
                type: "Identifier",
                name: "myOptions",
              }),
            }),
          ])
        );
      });

      describe("invalid", () => {
        it("defineOptions()", () => {
          const { result, context } = parse(`defineOptions()`);
          expect(result).toContain(`defineOptions()`);

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "warning",
                message: "INVALID_DEFINE_OPTIONS",
              }),
            ])
          );
        });

        it("const foo = defineOptions()", () => {
          const { result, context } = parse(`const foo = defineOptions()`);
          expect(result).toContain(`const foo = defineOptions()`);

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "warning",
                message: "INVALID_DEFINE_OPTIONS",
              }),
            ])
          );
        });

        it('defineOptions("a")', () => {
          const { result, context } = parse(`defineOptions("a")`);
          expect(result).toContain(`defineOptions("a")`);

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "warning",
                message: "INVALID_DEFINE_OPTIONS",
              }),
            ])
          );
        });

        it("defineOptions({}, hello)", () => {
          const { result, context } = parse(`defineOptions({}, hello)`);
          expect(result).toContain(`defineOptions({}, hello)`);

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "options",
                expression: expect.objectContaining({
                  type: "ObjectExpression",
                  properties: [],
                }),
              }),
              expect.objectContaining({
                type: "warning",
                message: "INVALID_DEFINE_OPTIONS",
              }),
            ])
          );
        });
      });
    });

    describe("defineProps", () => {
      describe("function & variable", () => {
        it("defineProps()", () => {
          const { result, context } = parse(`defineProps()`);

          expect(result).toContain(`const ___VERTER___props=defineProps();`);

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                isType: false,
                name: "___VERTER___props",

                objectName: undefined,
                typeName: undefined,
                valueName: "___VERTER___props",
              }),
            ])
          );
        });
        it("const props = defineProps()", () => {
          const { result, context } = parse(`const props = defineProps()`);

          expect(result).toContain(`const props = defineProps();`);

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                isType: false,
                name: "props",

                objectName: undefined,
                typeName: undefined,
                valueName: "props",
              }),
            ])
          );
        });

        it("defineProps({ a: String })", () => {
          const { result, context } = parse(`defineProps({ a: String })`);

          expect(result).toContain(
            `const ___VERTER___defineProps_Boxed=___VERTER___defineProps_Box({ a: String })`
          );
          expect(result).toContain(
            `const ___VERTER___props=defineProps(___VERTER___defineProps_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                isType: false,
                name: "___VERTER___props",

                objectName: "___VERTER___defineProps_Boxed",
                typeName: undefined,
                valueName: "___VERTER___props",
              }),
            ])
          );
        });
        it("const props = defineProps", () => {
          const { result, context } = parse(
            `const props = defineProps({ a: String })`
          );

          expect(result).toContain(
            `const ___VERTER___defineProps_Boxed=___VERTER___defineProps_Box({ a: String })`
          );
          expect(result).toContain(
            `const props = defineProps(___VERTER___defineProps_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                isType: false,
                name: "props",

                objectName: "___VERTER___defineProps_Boxed",
                typeName: undefined,
                valueName: "props",
              }),
            ])
          );
        });
      });
    });

    describe("types", () => {
      describe("withDefaults", () => {
        // it.skip("withDefaults(defineProps)", () => {
        //   const { result, context } = parse(
        //     `withDefaults(defineProps({ a: String }), {})`
        //   );

        //   expect(result).toContain(
        //     `const ___VERTER___Props=defineProps({ a: String });withDefaults(___VERTER___Props, {})`
        //   );

        //   expect(context.items).toMatchObject([
        //     {
        //       type: "macro-binding",
        //       macro: "defineProps",
        //       name: "___VERTER___Props",
        //     },
        //   ]);
        // });
        it.skip("const props = withDefaults(defineProps({a: String}), {})", () => {
          const { result, context, ...other } = parse(
            `const props = withDefaults(defineProps({ a: String }), {})`
          );

          expect(result).toContain(
            [
              "let ___VERTER___defineProps_Boxed;",
              "const ___VERTER___withDefaults_Boxed=___VERTER___withDefaults_Box(",
              "defineProps(___VERTER___defineProps_Boxed=___VERTER___defineProps_Box({ a: String })), {});",
            ].join("")
          );
          expect(result).toContain(
            "const props = withDefaults(___VERTER___withDefaults_Boxed[0],___VERTER___withDefaults_Boxed[1])"
          );
          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                name: "props",
                isType: false,

                objectName: "___VERTER___defineProps_Boxed",
                typeName: undefined,
                valueName: "props",
              }),
              expect.objectContaining({
                type: "macro-binding",
                macro: "withDefaults",
                name: "props",
                isType: false,

                objectName: "___VERTER___withDefaults_Boxed",
                typeName: undefined,
                valueName: "props",
              }),
              expect.objectContaining({
                type: "warning",
                message: "INVALID_WITH_DEFAULTS_DEFINE_PROPS_WITH_OBJECT_ARG",
                // this is bound to defineProps, but maybe it could be withDefaults, to be investigated and confirmed
                start: 51,
                end: 77,
              }),
            ])
          );
        });

        it.skip("withDefaults(defineProps<{ a?: string }>(), {})", () => {
          const { result, context, ...other } = parse(
            `withDefaults(defineProps<{ a?: string }>(), {})`,
            "ts"
          );

          expect(result).toContain(
            [
              "const ___VERTER___withDefaults_Boxed=___VERTER___withDefaults_Box(",
              "defineProps<___VERTER___defineProps_Type>(), {});",
            ].join("")
          );
          expect(result).toContain(
            "const ___VERTER___withDefaults=withDefaults(___VERTER___withDefaults_Boxed[0],___VERTER___withDefaults_Boxed[1])"
          );
          expect(result).toContain(
            ";type ___VERTER___defineProps_Type={}&{ a?: string };"
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                // NOTE THis maybe should be the name for withDefaults
                name: "___VERTER___withDefaults",
                isType: true,

                objectName: undefined,
                typeName: "___VERTER___defineProps_Type",
                valueName: "___VERTER___withDefaults",
              }),

              expect.objectContaining({
                type: "macro-binding",
                macro: "withDefaults",
                name: "___VERTER___withDefaults",
                isType: false,

                objectName: "___VERTER___withDefaults_Boxed",
                typeName: undefined,
                valueName: "___VERTER___withDefaults",
              }),
            ])
          );
        });
        it.skip("const props = withDefaults(defineProps<{ a?: string }>(), {})", () => {
          const { result, context, ...other } = parse(
            `const props = withDefaults(defineProps<{ a?: string }>(), {})`,
            "ts"
          );

          expect(result).toContain(
            [
              "const ___VERTER___withDefaults_Boxed=___VERTER___withDefaults_Box(",
              "defineProps<___VERTER___defineProps_Type>(), {});",
            ].join("")
          );
          expect(result).toContain(
            "const props = withDefaults(___VERTER___withDefaults_Boxed[0],___VERTER___withDefaults_Boxed[1])"
          );
          expect(result).toContain(
            ";type ___VERTER___defineProps_Type={}&{ a?: string };"
          );

          expect(context.items[0]).toMatchObject({
            type: "macro-binding",
            macro: "defineProps",
            name: "___VERTER___Props",
          });

          expect(context.items[1]).toMatchObject({
            type: "warning",
            message: "INVALID_WITH_DEFAULTS_DEFINE_PROPS_WITH_OBJECT_ARG",
            // this is bound to defineProps, but maybe it could be withDefaults, to be investigated and confirmed
            start: 51,
            end: 77,
          });
        });
      });
    });
  });

  describe.each(["js", "ts"])("lang %s", (lang) => {
    describe.each([true, "defineComponent", "randomComponentDeclarator"])(
      "options %s",
      (wrapper) => {
        function parse(content: string, pre: string = "") {
          const original = `${pre ? pre + "\n" : ""}export default ${
            wrapper ? wrapper + "(" : ""
          }${content}${wrapper ? ")" : ""}`;

          return { ..._parse(original, wrapper, lang, pre), original };
        }

        it.skip("leave the script untouched", () => {
          const { s, original } = parse("{ data(){ return { a: 0 } } }");
          expect(s.toString()).toContain(original);
        });

        // this should return the binding for expose
        it.todo("defineExpose", () => {
          const { s, context, original } = parse(
            `{ setup(){ defineExpose({ a: 0 }) }}`
          );
          expect(s.toString()).toContain(original);
          expect(context.items).toMatchObject([
            { type: "import" },
            {
              type: "macro-binding",
              macro: "defineExpose",
              name: "exposed",
              originalName: undefined,
            },
          ]);
        });

        it.skip("defineModel", () => {
          const { s, original } = parse(`{ setup(){ defineModel() }}`);
          expect(s.toString()).toContain(original);
        });
      }
    );

    // describe.each([false, "defineComponent", "randomComponentDeclarator"])(
    //   "wrapper %s",
    //   (wrapper) => {
    //     function parseWrapper(content: string) {

    //     }

    //   }
    // );
  });
});
