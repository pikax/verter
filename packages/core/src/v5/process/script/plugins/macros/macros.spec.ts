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

    describe("javascript", () => {
      function parseJS(content: string, pre: string = "") {
        return _parse(`${pre ? pre + "\n" : ""}${content}`, false, "js", pre);
      }

      it("defineProps()", () => {
        const { result, context } = parseJS(`defineProps()`);

        expect(result).toContain(`const ___VERTER___props=defineProps();`);

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "macro-binding",
              macro: "defineProps",
              isType: false,
              name: "___VERTER___props",
            }),
          ])
        );
      });

      it("defineProps({ a: String })", () => {
        const { result, context } = parseJS(`defineProps({ a: String })`);

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
            }),
          ])
        );
      });

      it("const props = defineProps({ a: String })", () => {
        const { result, context } = parseJS(
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
            }),
          ])
        );
      });

      it("defineEmits(['change', 'update'])", () => {
        const { result, context } = parseJS(
          `defineEmits(['change', 'update'])`
        );

        expect(result).toContain(
          `const ___VERTER___defineEmits_Boxed=___VERTER___defineEmits_Box(['change', 'update'])`
        );
        expect(result).toContain(
          `const ___VERTER___emits=defineEmits(___VERTER___defineEmits_Boxed)`
        );

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "macro-binding",
              macro: "defineEmits",
              isType: false,
              name: "___VERTER___emits",
              objectName: "___VERTER___defineEmits_Boxed",
            }),
          ])
        );
      });

      it("const emit = defineEmits(['change'])", () => {
        const { result, context } = parseJS(`const emit = defineEmits(['change'])`);

        expect(result).toContain(
          `const ___VERTER___defineEmits_Boxed=___VERTER___defineEmits_Box(['change'])`
        );
        expect(result).toContain(
          `const emit = defineEmits(___VERTER___defineEmits_Boxed)`
        );

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "macro-binding",
              macro: "defineEmits",
              isType: false,
              name: "emit",
              objectName: "___VERTER___defineEmits_Boxed",
            }),
          ])
        );
      });

      it("defineModel()", () => {
        const { result, context } = parseJS(`defineModel()`);
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
            }),
          ])
        );
      });

      it('const model = defineModel("title")', () => {
        const { result, context } = parseJS(`const model = defineModel("title")`);

        expect(result).toContain(
          `const ___VERTER___title_defineModel_Boxed=___VERTER___defineModel_Box("title")`
        );
        expect(result).toContain(
          `const model = defineModel(___VERTER___title_defineModel_Boxed)`
        );

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "define-model",
              name: "title",
              varName: "model",
              isType: false,
              objectName: "___VERTER___title_defineModel_Boxed",
            }),
          ])
        );
      });

      it("defineExpose({ focus: () => {} })", () => {
        const { result, context } = parseJS(`defineExpose({ focus: () => {} })`);

        expect(result).toContain(
          `const ___VERTER___defineExpose_Boxed=___VERTER___defineExpose_Box({ focus: () => {} })`
        );
        expect(result).toContain(`defineExpose(___VERTER___defineExpose_Boxed)`);

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "macro-binding",
              macro: "defineExpose",
              isType: false,
              name: "___VERTER___expose",
              objectName: "___VERTER___defineExpose_Boxed",
            }),
          ])
        );
      });

      it("defineSlots()", () => {
        const { result, context } = parseJS(`defineSlots()`);

        expect(result).toContain(`const ___VERTER___slots=defineSlots();`);

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "macro-binding",
              macro: "defineSlots",
              isType: false,
              name: "___VERTER___slots",
            }),
          ])
        );
      });

      it("defineOptions({ inheritAttrs: false })", () => {
        const { result, context } = parseJS(`defineOptions({ inheritAttrs: false })`);
        expect(result).toContain(`defineOptions({ inheritAttrs: false })`);

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "options",
              expression: expect.objectContaining({
                type: "ObjectExpression",
              }),
            }),
          ])
        );
      });
    });

    describe("extra", () => {
      function parseTS(content: string, pre: string = "") {
        return _parse(`${pre ? pre + "\n" : ""}${content}`, false, "ts", pre);
      }

      describe("multiple macros in same file", () => {
        it("defineProps and defineEmits together", () => {
          const { result, context } = parseTS(`
const props = defineProps<{ msg: string }>()
const emit = defineEmits<{ change: [value: string] }>()
          `);

          expect(result).toContain("type ___VERTER___defineProps_Type={ msg: string }");
          expect(result).toContain("type ___VERTER___defineEmits_Type={ change: [value: string] }");
          expect(result).toContain("const props = defineProps<___VERTER___defineProps_Type>()");
          expect(result).toContain("const emit = defineEmits<___VERTER___defineEmits_Type>()");

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                name: "props",
                isType: true,
                valueName: "props",
                typeName: "___VERTER___defineProps_Type",
                objectName: undefined,
              }),
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineEmits",
                name: "emit",
                isType: true,
                valueName: "emit",
                typeName: "___VERTER___defineEmits_Type",
                objectName: undefined,
              }),
            ])
          );
        });

        it("multiple defineModel calls", () => {
          const { result, context } = parseTS(`
const firstName = defineModel<string>('firstName')
const lastName = defineModel<string>('lastName')
          `);

          expect(result).toContain("type ___VERTER___firstName_defineModel_Type=string");
          expect(result).toContain("type ___VERTER___lastName_defineModel_Type=string");
          expect(result).toContain(
            "const ___VERTER___firstName_defineModel_Boxed=___VERTER___defineModel_Box('firstName')"
          );
          expect(result).toContain(
            "const ___VERTER___lastName_defineModel_Boxed=___VERTER___defineModel_Box('lastName')"
          );
          expect(result).toContain(
            "const firstName = defineModel<___VERTER___firstName_defineModel_Type>(___VERTER___firstName_defineModel_Boxed)"
          );
          expect(result).toContain(
            "const lastName = defineModel<___VERTER___lastName_defineModel_Type>(___VERTER___lastName_defineModel_Boxed)"
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",
                name: "firstName",
                varName: "firstName",
                isType: true,
                valueName: "firstName",
                typeName: "___VERTER___firstName_defineModel_Type",
                objectName: "___VERTER___firstName_defineModel_Boxed",
              }),
              expect.objectContaining({
                type: "define-model",
                name: "lastName",
                varName: "lastName",
                isType: true,
                valueName: "lastName",
                typeName: "___VERTER___lastName_defineModel_Type",
                objectName: "___VERTER___lastName_defineModel_Boxed",
              }),
            ])
          );
        });

        it("defineProps, defineEmits, defineSlots, and defineExpose together", () => {
          const { result, context } = parseTS(`
const props = defineProps<{ msg: string }>()
const emit = defineEmits<{ change: [] }>()
const slots = defineSlots<{ default: () => any }>()
defineExpose({ focus: () => {} })
          `);

          expect(result).toContain("type ___VERTER___defineProps_Type={ msg: string }");
          expect(result).toContain("type ___VERTER___defineEmits_Type={ change: [] }");
          expect(result).toContain("type ___VERTER___defineSlots_Type={ default: () => any }");
          expect(result).toContain(
            "const ___VERTER___defineExpose_Boxed=___VERTER___defineExpose_Box({ focus: () => {} })"
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                name: "props",
                isType: true,
                valueName: "props",
                typeName: "___VERTER___defineProps_Type",
              }),
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineEmits",
                name: "emit",
                isType: true,
                valueName: "emit",
                typeName: "___VERTER___defineEmits_Type",
              }),
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineSlots",
                name: "slots",
                isType: true,
                valueName: "slots",
                typeName: "___VERTER___defineSlots_Type",
              }),
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineExpose",
                name: "___VERTER___expose",
                isType: false,
                valueName: "___VERTER___expose",
                objectName: "___VERTER___defineExpose_Boxed",
              }),
            ])
          );
        });
      });

      describe("complex type arguments", () => {
        it("defineProps with union types", () => {
          const { result, context } = parseTS(
            `defineProps<{ value: string | number }>()`
          );

          expect(result).toContain(
            "type ___VERTER___defineProps_Type={ value: string | number }"
          );
          expect(result).toContain(
            "const ___VERTER___props=defineProps<___VERTER___defineProps_Type>();"
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                name: "___VERTER___props",
                isType: true,
                valueName: "___VERTER___props",
                typeName: "___VERTER___defineProps_Type",
                objectName: undefined,
              }),
            ])
          );
        });

        it("defineProps with generic imported type", () => {
          const { result, context } = parseTS(
            `defineProps<MyProps>()`,
            `import type { MyProps } from './types'`
          );

          expect(result).toContain("type ___VERTER___defineProps_Type=MyProps");
          expect(result).toContain(
            "const ___VERTER___props=defineProps<___VERTER___defineProps_Type>();"
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                name: "___VERTER___props",
                isType: true,
                valueName: "___VERTER___props",
                typeName: "___VERTER___defineProps_Type",
                objectName: undefined,
              }),
            ])
          );
        });

        it("defineEmits with function signature type", () => {
          const { result, context } = parseTS(
            `defineEmits<{ (e: 'change', value: number): void; (e: 'update'): void }>()`
          );

          expect(result).toContain(
            "type ___VERTER___defineEmits_Type={ (e: 'change', value: number): void; (e: 'update'): void }"
          );
          expect(result).toContain(
            "const ___VERTER___emits=defineEmits<___VERTER___defineEmits_Type>();"
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineEmits",
                name: "___VERTER___emits",
                isType: true,
                valueName: "___VERTER___emits",
                typeName: "___VERTER___defineEmits_Type",
                objectName: undefined,
              }),
            ])
          );
        });

        it("defineSlots with complex slot props", () => {
          const { result, context } = parseTS(
            `defineSlots<{ default: (props: { item: { id: number; name: string }; index: number }) => any }>()`
          );

          expect(result).toContain(
            "type ___VERTER___defineSlots_Type={ default: (props: { item: { id: number; name: string }; index: number }) => any }"
          );
          expect(result).toContain(
            "const ___VERTER___slots=defineSlots<___VERTER___defineSlots_Type>();"
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineSlots",
                name: "___VERTER___slots",
                isType: true,
                valueName: "___VERTER___slots",
                typeName: "___VERTER___defineSlots_Type",
                objectName: undefined,
              }),
            ])
          );
        });

        it("defineModel with generic type and options", () => {
          const { result, context } = parseTS(
            `const model = defineModel<string | undefined>('value', { required: false })`
          );

          expect(result).toContain(
            "type ___VERTER___value_defineModel_Type=string | undefined"
          );
          expect(result).toContain(
            "const ___VERTER___value_defineModel_Boxed=___VERTER___defineModel_Box('value', { required: false })"
          );
          expect(result).toContain(
            "const model = defineModel<___VERTER___value_defineModel_Type>(___VERTER___value_defineModel_Boxed[0],___VERTER___value_defineModel_Boxed[1])"
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "define-model",
                name: "value",
                varName: "model",
                isType: true,
                valueName: "model",
                typeName: "___VERTER___value_defineModel_Type",
                objectName: "___VERTER___value_defineModel_Boxed",
              }),
            ])
          );
        });
      });

      describe("destructuring patterns", () => {
        it("destructured defineProps should still work", () => {
          // Note: Vue doesn't support destructuring directly, but let's test the behavior
          const { result, context } = parseTS(
            `const { foo } = defineProps<{ foo: string }>()`
          );

          expect(result).toContain("type ___VERTER___defineProps_Type={ foo: string }");
          expect(result).toContain("defineProps<___VERTER___defineProps_Type>()");

          // The macro should still be processed
          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                isType: true,
                typeName: "___VERTER___defineProps_Type",
                objectName: undefined,
              }),
            ])
          );
        });
      });

      describe("macros with variable references", () => {
        it("defineProps with variable as argument", () => {
          const { result, context } = parseTS(
            `const props = defineProps(propsConfig)`,
            `const propsConfig = { foo: String }`
          );

          expect(result).toContain(
            "const ___VERTER___defineProps_Boxed=___VERTER___defineProps_Box(propsConfig)"
          );
          expect(result).toContain(
            "const props = defineProps(___VERTER___defineProps_Boxed)"
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                name: "props",
                isType: false,
                valueName: "props",
                typeName: undefined,
                objectName: "___VERTER___defineProps_Boxed",
              }),
            ])
          );
        });

        it("defineEmits with variable as argument", () => {
          const { result, context } = parseTS(
            `const emit = defineEmits(emitsConfig)`,
            `const emitsConfig = ['change', 'update']`
          );

          expect(result).toContain(
            "const ___VERTER___defineEmits_Boxed=___VERTER___defineEmits_Box(emitsConfig)"
          );
          expect(result).toContain(
            "const emit = defineEmits(___VERTER___defineEmits_Boxed)"
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineEmits",
                name: "emit",
                isType: false,
                valueName: "emit",
                typeName: undefined,
                objectName: "___VERTER___defineEmits_Boxed",
              }),
            ])
          );
        });
      });

      describe("whitespace and formatting", () => {
        it("defineProps with multiline type", () => {
          const { result, context } = parseTS(`defineProps<{
  foo: string
  bar: number
}>()`);

          expect(result).toContain("type ___VERTER___defineProps_Type=");
          expect(result).toContain(
            "const ___VERTER___props=defineProps<___VERTER___defineProps_Type>();"
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                name: "___VERTER___props",
                isType: true,
                valueName: "___VERTER___props",
                typeName: "___VERTER___defineProps_Type",
                objectName: undefined,
              }),
            ])
          );
        });

        it("defineEmits with multiline object", () => {
          const { result, context } = parseTS(`defineEmits({
  change: () => true,
  update: (val: string) => true
})`);

          expect(result).toContain(
            "const ___VERTER___defineEmits_Boxed=___VERTER___defineEmits_Box({"
          );
          expect(result).toContain(
            "const ___VERTER___emits=defineEmits(___VERTER___defineEmits_Boxed)"
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineEmits",
                name: "___VERTER___emits",
                isType: false,
                valueName: "___VERTER___emits",
                typeName: undefined,
                objectName: "___VERTER___defineEmits_Boxed",
              }),
            ])
          );
        });
      });

      describe("macro return tracking", () => {
        function getMacroReturnContent(context: any): string {
          const macroReturn = context.items.find(
            (item: any) => item.type === "macro-return"
          );
          return macroReturn?.content ?? "";
        }

        it("tracks macro return content correctly", () => {
          const { context } = parseTS(`
const props = defineProps<{ msg: string }>()
const emit = defineEmits<{ change: [] }>()
          `);

          expect(getMacroReturnContent(context)).toMatchInlineSnapshot(
            `"{props:{"value":{} as typeof props,"type":{} as ___VERTER___defineProps_Type},emits:{"value":{} as typeof emit,"type":{} as ___VERTER___defineEmits_Type}}"`
          );
        });

        it("includes defineModel in macro return with type", () => {
          const { context } = parseTS(`
const model = defineModel<string>()
          `);

          expect(getMacroReturnContent(context)).toMatchInlineSnapshot(
            `"{model:{modelValue:{"value":{} as typeof model,"type":{} as ___VERTER___modelValue_defineModel_Type}}}"`
          );
        });

        it("includes defineModel in macro return with object", () => {
          const { context } = parseTS(`
const model = defineModel({ default: 'hello' })
          `);

          expect(getMacroReturnContent(context)).toMatchInlineSnapshot(
            `"{model:{modelValue:{"value":{} as typeof model,"object":{} as typeof ___VERTER___modelValue_defineModel_Boxed}}}"`
          );
        });

        it("includes multiple defineModel calls in macro return", () => {
          const { context } = parseTS(`
const firstName = defineModel<string>('firstName')
const lastName = defineModel<string>('lastName')
          `);

          expect(getMacroReturnContent(context)).toMatchInlineSnapshot(
            `"{model:{firstName:{"value":{} as typeof firstName,"type":{} as ___VERTER___firstName_defineModel_Type,"object":{} as typeof ___VERTER___firstName_defineModel_Boxed},lastName:{"value":{} as typeof lastName,"type":{} as ___VERTER___lastName_defineModel_Type,"object":{} as typeof ___VERTER___lastName_defineModel_Boxed}}}"`
          );
        });

        it("includes named defineModel with options in macro return", () => {
          const { context } = parseTS(`
const count = defineModel<number>('count', { required: true })
          `);

          expect(getMacroReturnContent(context)).toMatchInlineSnapshot(
            `"{model:{count:{"value":{} as typeof count,"type":{} as ___VERTER___count_defineModel_Type,"object":{} as typeof ___VERTER___count_defineModel_Boxed}}}"`
          );
        });

        it("includes both macros and defineModel in macro return", () => {
          const { context } = parseTS(`
const props = defineProps<{ msg: string }>()
const emit = defineEmits<{ change: [] }>()
const model = defineModel<string>()
          `);

          expect(getMacroReturnContent(context)).toMatchInlineSnapshot(
            `"{props:{"value":{} as typeof props,"type":{} as ___VERTER___defineProps_Type},emits:{"value":{} as typeof emit,"type":{} as ___VERTER___defineEmits_Type},model:{modelValue:{"value":{} as typeof model,"type":{} as ___VERTER___modelValue_defineModel_Type}}}"`
          );
        });

        it("does not include model key when no defineModel is used", () => {
          const { context } = parseTS(`
const props = defineProps<{ msg: string }>()
          `);

          expect(getMacroReturnContent(context)).toMatchInlineSnapshot(
            `"{props:{"value":{} as typeof props,"type":{} as ___VERTER___defineProps_Type}}"`
          );
        });

        it("handles defineModel without assignment correctly", () => {
          const { context } = parseTS(`
defineModel<string>()
          `);

          expect(getMacroReturnContent(context)).toMatchInlineSnapshot(
            `"{model:{modelValue:{"value":{} as typeof ___VERTER___models_modelValue,"type":{} as ___VERTER___modelValue_defineModel_Type}}}"`
          );
        });

        it("empty macro return when no macros used", () => {
          const { context } = parseTS(`const foo = 'bar'`);

          expect(getMacroReturnContent(context)).toMatchInlineSnapshot(`"{}"`);
        });
      });

      describe("warnings", () => {
        it("warns on invalid withDefaults usage", () => {
          const { result, context } = parseTS(
            `withDefaults(defineProps({ foo: String }), { foo: 'default' })`
          );

          expect(result).toContain("___VERTER___withDefaults_Boxed");
          expect(result).toContain("___VERTER___defineProps_Boxed");

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "warning",
                message: "INVALID_WITH_DEFAULTS_DEFINE_PROPS_WITH_OBJECT_ARG",
              }),
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
              }),
              expect.objectContaining({
                type: "macro-binding",
                macro: "withDefaults",
              }),
            ])
          );
        });

        it("warns on defineOptions with no arguments", () => {
          const { result, context } = parseTS(`defineOptions()`);

          expect(result).toContain("defineOptions()");

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "warning",
                message: "INVALID_DEFINE_OPTIONS",
              }),
            ])
          );
        });

        it("warns on defineOptions with invalid argument type", () => {
          const { result, context } = parseTS(`defineOptions(123)`);

          expect(result).toContain("defineOptions(123)");

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "warning",
                message: "INVALID_DEFINE_OPTIONS",
              }),
            ])
          );
        });

        it("warns on defineOptions with extra arguments", () => {
          const { result, context } = parseTS(`defineOptions({}, 'extra')`);

          expect(result).toContain("defineOptions({}, 'extra')");

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "options",
                expression: expect.objectContaining({
                  type: "ObjectExpression",
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

      describe("imports tracking", () => {
        it("adds vue imports for macros", () => {
          const { result, context } = parseTS(`defineProps<{ foo: string }>()`);

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "import",
                from: "vue",
                items: expect.arrayContaining([
                  expect.objectContaining({ name: "defineProps" }),
                ]),
              }),
            ])
          );
        });

        it("adds helper imports for boxed macros", () => {
          const { result, context } = parseTS(`defineProps({ foo: String })`);

          expect(result).toContain("___VERTER___defineProps_Box");

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "import",
                from: "$verter/types$",
                items: expect.arrayContaining([
                  expect.objectContaining({
                    name: "defineProps_Box",
                    alias: "___VERTER___defineProps_Box",
                  }),
                ]),
              }),
              expect.objectContaining({
                type: "import",
                from: "vue",
                items: expect.arrayContaining([
                  expect.objectContaining({ name: "defineProps" }),
                ]),
              }),
            ])
          );
        });

        it("adds correct imports for multiple macros", () => {
          const { result, context } = parseTS(`
const props = defineProps({ foo: String })
const emit = defineEmits(['change'])
          `);

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "import",
                from: "$verter/types$",
                items: expect.arrayContaining([
                  expect.objectContaining({ name: "defineProps_Box" }),
                ]),
              }),
              expect.objectContaining({
                type: "import",
                from: "$verter/types$",
                items: expect.arrayContaining([
                  expect.objectContaining({ name: "defineEmits_Box" }),
                ]),
              }),
              expect.objectContaining({
                type: "import",
                from: "vue",
                items: expect.arrayContaining([
                  expect.objectContaining({ name: "defineProps" }),
                ]),
              }),
              expect.objectContaining({
                type: "import",
                from: "vue",
                items: expect.arrayContaining([
                  expect.objectContaining({ name: "defineEmits" }),
                ]),
              }),
            ])
          );
        });
      });
    });

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

      describe("types", () => {
        it("defineProps<{ a?: string }>()", () => {
          const { result, context } = parse(`defineProps<{ a?: string }>()`);

          expect(result).toContain(
            "type ___VERTER___defineProps_Type={ a?: string }"
          );
          expect(result).toContain(
            `const ___VERTER___props=defineProps<___VERTER___defineProps_Type>();`
          );

          // TODO it should add a warning because this is an invalid usage of `defineProps`
          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                isType: true,
                name: "___VERTER___props",

                objectName: undefined,
                typeName: "___VERTER___defineProps_Type",
                valueName: "___VERTER___props",
              }),
            ])
          );
        });

        it("const props = defineProps<{ a?: string }>()", () => {
          const { result, context } = parse(
            `const props = defineProps<{ a?: string }>()`
          );

          expect(result).toContain(
            "type ___VERTER___defineProps_Type={ a?: string }"
          );
          expect(result).toContain(
            `const props = defineProps<___VERTER___defineProps_Type>();`
          );
          // TODO it should add a warning because this is an invalid usage of `defineProps`

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                isType: true,
                name: "props",

                objectName: undefined,
                typeName: "___VERTER___defineProps_Type",
                valueName: "props",
              }),
            ])
          );
        });

        it("defineProps<{ a?: string }>({ a: String })", () => {
          const { result, context } = parse(
            `defineProps<{ a?: string }>({ a: String })`
          );

          expect(result).toContain(
            "type ___VERTER___defineProps_Type={ a?: string }"
          );

          expect(result).toContain(
            "const ___VERTER___defineProps_Boxed=___VERTER___defineProps_Box({ a: String })"
          );
          expect(result).toContain(
            `const ___VERTER___props=defineProps<___VERTER___defineProps_Type>(___VERTER___defineProps_Boxed);`
          );
          // TODO it should add a warning because this is an invalid usage of `defineProps`

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                isType: true,
                name: "___VERTER___props",

                objectName: "___VERTER___defineProps_Boxed",
                typeName: "___VERTER___defineProps_Type",
                valueName: "___VERTER___props",
              }),
            ])
          );
        });

        it("const props = defineProps<{ a?: string }>({ a: String })", () => {
          const { result, context } = parse(
            `const props = defineProps<{ a?: string }>({ a: String })`
          );

          expect(result).toContain(
            "type ___VERTER___defineProps_Type={ a?: string }"
          );

          expect(result).toContain(
            "const ___VERTER___defineProps_Boxed=___VERTER___defineProps_Box({ a: String })"
          );
          expect(result).toContain(
            `const props = defineProps<___VERTER___defineProps_Type>(___VERTER___defineProps_Boxed);`
          );

          // TODO it should add a warning because this is an invalid usage of `defineProps`

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineProps",
                isType: true,
                name: "props",

                objectName: "___VERTER___defineProps_Boxed",
                typeName: "___VERTER___defineProps_Type",
                valueName: "props",
              }),
            ])
          );
        });
      });
    });

    describe("defineEmits", () => {
      describe("function & variable", () => {
        it("defineEmits()", () => {
          const { result, context } = parse(`defineEmits()`);

          expect(result).toContain(`const ___VERTER___emits=defineEmits();`);

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineEmits",
                isType: false,
                name: "___VERTER___emits",

                objectName: undefined,
                typeName: undefined,
                valueName: "___VERTER___emits",
              }),
            ])
          );
        });

        it("const emit = defineEmits()", () => {
          const { result, context } = parse(`const emit = defineEmits()`);

          expect(result).toContain(`const emit = defineEmits();`);

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineEmits",
                isType: false,
                name: "emit",

                objectName: undefined,
                typeName: undefined,
                valueName: "emit",
              }),
            ])
          );
        });

        it("defineEmits(['change', 'update'])", () => {
          const { result, context } = parse(
            `defineEmits(['change', 'update'])`
          );

          expect(result).toContain(
            `const ___VERTER___defineEmits_Boxed=___VERTER___defineEmits_Box(['change', 'update'])`
          );
          expect(result).toContain(
            `const ___VERTER___emits=defineEmits(___VERTER___defineEmits_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineEmits",
                isType: false,
                name: "___VERTER___emits",

                objectName: "___VERTER___defineEmits_Boxed",
                typeName: undefined,
                valueName: "___VERTER___emits",
              }),
            ])
          );
        });

        it("const emit = defineEmits(['change', 'update'])", () => {
          const { result, context } = parse(
            `const emit = defineEmits(['change', 'update'])`
          );

          expect(result).toContain(
            `const ___VERTER___defineEmits_Boxed=___VERTER___defineEmits_Box(['change', 'update'])`
          );
          expect(result).toContain(
            `const emit = defineEmits(___VERTER___defineEmits_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineEmits",
                isType: false,
                name: "emit",

                objectName: "___VERTER___defineEmits_Boxed",
                typeName: undefined,
                valueName: "emit",
              }),
            ])
          );
        });


        it("defineEmits({ change: ()=> true, update: (arg: {foo:string})=> true})", () => {
          const { result, context } = parse(
            `defineEmits({ change: ()=> true, update: (arg: {foo:string})=> true})`
          );

          expect(result).toContain(
            `const ___VERTER___defineEmits_Boxed=___VERTER___defineEmits_Box({ change: ()=> true, update: (arg: {foo:string})=> true})`
          );
          expect(result).toContain(
            `const ___VERTER___emits=defineEmits(___VERTER___defineEmits_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineEmits",
                isType: false,
                name: "___VERTER___emits",

                objectName: "___VERTER___defineEmits_Boxed",
                typeName: undefined,
                valueName: "___VERTER___emits",
              }),
            ])
          );
        });

        it("const emit = defineEmits({ change: ()=> true, update: (arg: {foo:string})=> true})", () => {
          const { result, context } = parse(
            `const emit = defineEmits({ change: ()=> true, update: (arg: {foo:string})=> true})`
          );

          expect(result).toContain(
            `const ___VERTER___defineEmits_Boxed=___VERTER___defineEmits_Box({ change: ()=> true, update: (arg: {foo:string})=> true})`
          );
          expect(result).toContain(
            `const emit = defineEmits(___VERTER___defineEmits_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineEmits",
                isType: false,
                name: "emit",

                objectName: "___VERTER___defineEmits_Boxed",
                typeName: undefined,
                valueName: "emit",
              }),
            ])
          );
        });
      });

      describe("types", () => {
        it("defineEmits<{ change: [value: number] }>()", () => {
          const { result, context } = parse(
            `defineEmits<{ change: [value: number] }>()`
          );

          expect(result).toContain(
            "type ___VERTER___defineEmits_Type={ change: [value: number] }"
          );
          expect(result).toContain(
            `const ___VERTER___emits=defineEmits<___VERTER___defineEmits_Type>();`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineEmits",
                isType: true,
                name: "___VERTER___emits",

                objectName: undefined,
                typeName: "___VERTER___defineEmits_Type",
                valueName: "___VERTER___emits",
              }),
            ])
          );
        });

        it("const emit = defineEmits<{ change: [value: number] }>()", () => {
          const { result, context } = parse(
            `const emit = defineEmits<{ change: [value: number] }>()`
          );

          expect(result).toContain(
            "type ___VERTER___defineEmits_Type={ change: [value: number] }"
          );
          expect(result).toContain(
            `const emit = defineEmits<___VERTER___defineEmits_Type>();`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineEmits",
                isType: true,
                name: "emit",

                objectName: undefined,
                typeName: "___VERTER___defineEmits_Type",
                valueName: "emit",
              }),
            ])
          );
        });
      });
    });

    describe("defineSlots", () => {
      describe("function & variable", () => {
        it("defineSlots()", () => {
          const { result, context } = parse(`defineSlots()`);

          expect(result).toContain(`const ___VERTER___slots=defineSlots();`);

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineSlots",
                isType: false,
                name: "___VERTER___slots",

                objectName: undefined,
                typeName: undefined,
                valueName: "___VERTER___slots",
              }),
            ])
          );
        });

        it("const slots = defineSlots()", () => {
          const { result, context } = parse(`const slots = defineSlots()`);

          expect(result).toContain(`const slots = defineSlots();`);

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineSlots",
                isType: false,
                name: "slots",

                objectName: undefined,
                typeName: undefined,
                valueName: "slots",
              }),
            ])
          );
        });
      });

      describe("types", () => {
        it("defineSlots<{ default: (props: { msg: string }) => any }>()", () => {
          const { result, context } = parse(
            `defineSlots<{ default: (props: { msg: string }) => any }>()`
          );

          expect(result).toContain(
            "type ___VERTER___defineSlots_Type={ default: (props: { msg: string }) => any }"
          );
          expect(result).toContain(
            `const ___VERTER___slots=defineSlots<___VERTER___defineSlots_Type>();`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineSlots",
                isType: true,
                name: "___VERTER___slots",

                objectName: undefined,
                typeName: "___VERTER___defineSlots_Type",
                valueName: "___VERTER___slots",
              }),
            ])
          );
        });

        it("const slots = defineSlots<{ default: (props: { msg: string }) => any }>()", () => {
          const { result, context } = parse(
            `const slots = defineSlots<{ default: (props: { msg: string }) => any }>()`
          );

          expect(result).toContain(
            "type ___VERTER___defineSlots_Type={ default: (props: { msg: string }) => any }"
          );
          expect(result).toContain(
            `const slots = defineSlots<___VERTER___defineSlots_Type>();`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineSlots",
                isType: true,
                name: "slots",

                objectName: undefined,
                typeName: "___VERTER___defineSlots_Type",
                valueName: "slots",
              }),
            ])
          );
        });
      });
    });

    describe("defineExpose", () => {
      it("defineExpose()", () => {
        const { result, context } = parse(`defineExpose()`);

        expect(result).toContain(`defineExpose()`);

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "macro-binding",
              macro: "defineExpose",
              isType: false,
              name: "___VERTER___expose",

              objectName: undefined,
              typeName: undefined,
              valueName: "___VERTER___expose",
            }),
          ])
        );
      });

      it("defineExpose({ focus: () => {} })", () => {
        const { result, context } = parse(`defineExpose({ focus: () => {} })`);

        expect(result).toContain(
          `const ___VERTER___defineExpose_Boxed=___VERTER___defineExpose_Box({ focus: () => {} })`
        );
        expect(result).toContain(
          `defineExpose(___VERTER___defineExpose_Boxed)`
        );

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "macro-binding",
              macro: "defineExpose",
              isType: false,
              name: "___VERTER___expose",

              objectName: "___VERTER___defineExpose_Boxed",
              typeName: undefined,
              valueName: "___VERTER___expose",
            }),
          ])
        );
      });

      describe("types", () => {
        it("defineExpose<{ focus: () => void }>()", () => {
          const { result, context } = parse(
            `defineExpose<{ focus: () => void }>()`
          );

          expect(result).toContain(
            "type ___VERTER___defineExpose_Type={ focus: () => void }"
          );
          expect(result).toContain(
            `defineExpose<___VERTER___defineExpose_Type>()`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineExpose",
                isType: true,
                name: "___VERTER___expose",

                objectName: undefined,
                typeName: "___VERTER___defineExpose_Type",
                valueName: "___VERTER___expose",
              }),
            ])
          );
        });

        it("defineExpose<{ focus: () => void }>({ focus: () => {} })", () => {
          const { result, context } = parse(
            `defineExpose<{ focus: () => void }>({ focus: () => {} })`
          );

          expect(result).toContain(
            "type ___VERTER___defineExpose_Type={ focus: () => void }"
          );
          expect(result).toContain(
            `const ___VERTER___defineExpose_Boxed=___VERTER___defineExpose_Box({ focus: () => {} })`
          );
          expect(result).toContain(
            `defineExpose<___VERTER___defineExpose_Type>(___VERTER___defineExpose_Boxed)`
          );

          expect(context.items).toEqual(
            expect.arrayContaining([
              expect.objectContaining({
                type: "macro-binding",
                macro: "defineExpose",
                isType: true,
                name: "___VERTER___expose",

                objectName: "___VERTER___defineExpose_Boxed",
                typeName: "___VERTER___defineExpose_Type",
                valueName: "___VERTER___expose",
              }),
            ])
          );
        });
      });
    });

    describe("withDefaults", () => {
      it("withDefaults()", () => {
        const { result, context } = parse(`withDefaults()`);

        expect(result).toContain(
          `const ___VERTER___withDefaults=withDefaults()`
        );

        // TODO it should add a warning because this is an invalid usage of `withDefaults`
        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "macro-binding",
              macro: "withDefaults",
              isType: false,
              name: "___VERTER___withDefaults",

              objectName: undefined,
              typeName: undefined,
              valueName: "___VERTER___withDefaults",
            }),
          ])
        );
      });

      it("const props = withDefaults()", () => {
        const { result, context } = parse(`const props = withDefaults()`);

        expect(result).toContain(`const props = withDefaults()`);

        // TODO it should add a warning because this is an invalid usage of `withDefaults`
        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "macro-binding",
              macro: "withDefaults",
              isType: false,
              name: "props",

              objectName: undefined,
              typeName: undefined,
              valueName: "props",
            }),
          ])
        );
      });

      it("withDefaults(defineProps({ a: String }))", () => {
        const { result, context } = parse(
          `withDefaults(defineProps({ a: String }))`
        );

        expect(result).toContain(
          "const ___VERTER___withDefaults_Boxed=___VERTER___withDefaults_Box(defineProps(___VERTER___defineProps_Boxed=___VERTER___defineProps_Box({ a: String })))"
        );
        expect(result).toContain("let ___VERTER___defineProps_Boxed;");
        expect(result).toContain(
          `const ___VERTER___withDefaults=withDefaults(___VERTER___withDefaults_Boxed)`
        );

        // TODO it should add a warning because this is an invalid usage of `withDefaults` with an object
        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "macro-binding",
              macro: "defineProps",
              isType: false,
              name: "___VERTER___withDefaults",

              objectName: "___VERTER___defineProps_Boxed",
              typeName: undefined,
              valueName: "___VERTER___withDefaults",
            }),
            expect.objectContaining({
              type: "macro-binding",
              macro: "withDefaults",
              isType: false,
              name: "___VERTER___withDefaults",

              objectName: "___VERTER___withDefaults_Boxed",
              typeName: undefined,
              valueName: "___VERTER___withDefaults",
            }),
          ])
        );
      });
      it("const props = withDefaults(defineProps({a: String}), {})", () => {
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

      it("withDefaults(defineProps<{ a?: string }>(), {})", () => {
        const { result, context, ...other } = parse(
          `withDefaults(defineProps<{ a?: string }>(), {})`
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
      it("const props = withDefaults(defineProps<{ a?: string }>(), {})", () => {
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

        expect(context.items).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              type: "macro-binding",
              macro: "defineProps",
              name: "props",
              isType: true,

              objectName: undefined,
              typeName: "___VERTER___defineProps_Type",
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
          ])
        );
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

        // Options API macros inside setup() are not transformed by the macros plugin
        // They remain as-is since the setup() function handles them at runtime
        it("defineExpose in setup() is left untouched", () => {
          const { result, context, original } = parse(
            `{ setup(){ defineExpose({ a: 0 }) }}`
          );
          // The code should remain unchanged for Options API
          expect(result).toContain(`defineExpose({ a: 0 })`);
        });

        it("defineModel in setup() is left untouched", () => {
          const { result, original } = parse(`{ setup(){ defineModel() }}`);
          // The code should remain unchanged for Options API
          expect(result).toContain(`defineModel()`);
        });

        it("defineProps in setup() is left untouched", () => {
          const { result } = parse(`{ setup(){ const props = defineProps(['foo']) }}`);
          expect(result).toContain(`defineProps(['foo'])`);
        });

        it("defineEmits in setup() is left untouched", () => {
          const { result } = parse(`{ setup(){ const emit = defineEmits(['change']) }}`);
          expect(result).toContain(`defineEmits(['change'])`);
        });

        it("defineSlots in setup() is left untouched", () => {
          const { result } = parse(`{ setup(){ const slots = defineSlots() }}`);
          expect(result).toContain(`defineSlots()`);
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
