import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";

import { MacrosPlugin } from "./index.js";

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

    const r = processScript(scriptBlock.result.items, [MacrosPlugin], {
      s,
      filename: "test.vue",
      blocks: parsed.blocks,
      isSetup: wrapper === false,
      block: scriptBlock,
    });

    return r;
  }

  //   describe("ts", () => {});

  //   it.only("test", () => {
  //     const { s } = _parse(`export default { setup(){ defineExpose({ a: 0 }); }}`, 'ddd');
  //     expect(s.original).toBe(s.toString());
  //   });

  describe("setup", () => {
    function parse(content: string, lang = "ts", pre: string = "") {
      return _parse(`${pre ? pre + "\n" : ""}${content}`, false, lang, pre);
    }

    it("defineProps", () => {
      const { result, context } = parse(
        `const props = defineProps({ a: String })`
      );

      expect(result).toContain(
        `let ___VERTER___Props;const props=___VERTER___Props = defineProps({ a: String })`
      );

      expect(context.items).toMatchObject([
        {
          name: "___VERTER___Props",
          type: "macro-binding",
          originalName: undefined,
          macro: "defineProps",
        },
      ]);
    });

    describe.only("defineModel", () => {
      it("model without assignment", () => {
        const { result, context } = parse(`defineModel()`);
        expect(result).toContain(
          `const ___VERTER___models_modelValue=defineModel()`
        );

        expect(context.items).toMatchObject([{
          type: 'import'
        },
          {
            name: "___VERTER___models_modelValue",
            type: "macro-binding",
            macro: "defineModel",
            originalName: "modelValue",
          },
        ]);
      });

      it("defineModel()", () => {
        const { result, context } = parse(`const model = defineModel()`);
        expect(result).toContain(
          `let ___VERTER___models_modelValue;const model=___VERTER___models_modelValue = defineModel()`
        );

        expect(context.items).toMatchObject([{
          type: 'import'
        },
          {
            name: "___VERTER___models_modelValue",
            type: "macro-binding",
            macro: "defineModel",
            originalName: "modelValue",
          },
        ]);
      });

      it("defineModel({})", () => {
        const { result, context } = parse(`const model = defineModel({})`);
        expect(result).toContain(
          `let ___VERTER___models_modelValue;const model=___VERTER___models_modelValue = defineModel({})`
        );

        expect(context.items).toMatchObject([{
          type: 'import'
        },
          {
            name: "___VERTER___models_modelValue",
            type: "macro-binding",
            macro: "defineModel",
            originalName: "modelValue",
          },
        ]);
      });

      it('defineModel("model")', () => {
        const { result, context } = parse(`const model = defineModel("model")`);
        expect(result).toContain(
          `let ___VERTER___models_model;const model=___VERTER___models_model = defineModel("model")`
        );

        expect(context.items).toMatchObject([{
          type: 'import'
        },
          {
            name: "___VERTER___models_model",
            type: "macro-binding",
            macro: "defineModel",
            originalName: "model",
          },
        ]);
      });

      it('defineModel("model", {})', () => {
        const { result, context } = parse(
          `const model = defineModel("model", {})`
        );
        expect(result).toContain(
          `let ___VERTER___models_model;const model=___VERTER___models_model = defineModel("model", {})`
        );

        expect(context.items).toMatchObject([
          {
            type: 'import'
          },
          {
            name: "___VERTER___models_model",
            type: "macro-binding",
            macro: "defineModel",
            originalName: "model",
          },
        ]);
      });
    });

    describe("defineOptions", () => {
      it("defineOptions({})", () => {
        const { result, context } = parse(`defineOptions({})`);
        expect(result).toContain(`defineOptions({})`);

        expect(context.items).toMatchObject([
          {
            type: "options",
            expression: {
              type: "ObjectExpression",
              properties: [],
            },
          },
        ]);
      });
      it("const foo = defineOptions({})", () => {
        const { result, context } = parse(`const foo = defineOptions({})`);
        expect(result).toContain(`const foo = defineOptions({})`);

        expect(context.items).toMatchObject([
          {
            type: "options",
            expression: {
              type: "ObjectExpression",
              properties: [],
            },
          },
        ]);
      });

      it("defineOptions({ a: 0 })", () => {
        const { result, context } = parse(`defineOptions({ a: 0 })`);
        expect(result).toContain(`defineOptions({ a: 0 })`);

        expect(context.items).toMatchObject([
          {
            type: "options",
            expression: {
              type: "ObjectExpression",
              properties: [
                {
                  type: "ObjectProperty",
                  key: { type: "Identifier", name: "a" },
                  value: { type: "Literal", value: 0 },
                },
              ],
            },
          },
        ]);
      });

      it("defineOptions(myOptions)", () => {
        const { result, context } = parse(`defineOptions(myOptions)`);
        expect(result).toContain(`defineOptions(myOptions)`);

        expect(context.items).toMatchObject([
          {
            type: "options",
            expression: {
              type: "Identifier",
              name: "myOptions",
            },
          },
        ]);
      });

      describe("invalid", () => {
        it("defineOptions()", () => {
          const { result, context } = parse(`defineOptions()`);
          expect(result).toContain(`defineOptions()`);

          expect(context.items).toMatchObject([
            {
              type: "warning",
              message: "INVALID_DEFINE_OPTIONS",
            },
          ]);
        });

        it("const foo = defineOptions()", () => {
          const { result, context } = parse(`const foo = defineOptions()`);
          expect(result).toContain(`const foo = defineOptions()`);

          expect(context.items).toMatchObject([
            {
              type: "warning",
              message: "INVALID_DEFINE_OPTIONS",
            },
          ]);
        });

        it('defineOptions("a")', () => {
          const { result, context } = parse(`defineOptions("a")`);
          expect(result).toContain(`defineOptions("a")`);

          expect(context.items).toMatchObject([
            {
              type: "warning",
              message: "INVALID_DEFINE_OPTIONS",
            },
          ]);
        });

        it("defineOptions({}, hello)", () => {
          const { result, context } = parse(`defineOptions({}, hello)`);
          expect(result).toContain(`defineOptions({}, hello)`);

          expect(context.items).toMatchObject([
            {
              type: "options",
              expression: {
                type: "ObjectExpression",
                properties: [],
              },
            },
            {
              type: "warning",
              message: "INVALID_DEFINE_OPTIONS",
            },
          ]);
        });
      });
    });
  });

  describe.each(["js", "ts"])("lang %s", (lang) => {
    describe.each([true, "defineComponent", "randomComponentDeclarator"])(
      "options %s",
      (wrapper) => {
        function parse(content: string, pre: string = "") {
          return _parse(
            `${pre ? pre + "\n" : ""}export default ${
              wrapper ? wrapper + "(" : ""
            }${content}${wrapper ? ")" : ""}`,
            wrapper,
            lang,
            pre
          );
        }

        it("leave the script untouched", () => {
          const { s } = parse("{ data(){ return { a: 0 } } }");
          expect(s.original).toBe(s.toString());
        });

        it("defineExpose", () => {
          const { s, context } = parse(`{ setup(){ defineExpose({ a: 0 }) }}`);
          // expect(s.original).toBe(s.toString());

          expect(context.items).toMatchObject([
            {
              message: "MACRO_NOT_IN_SETUP",
              type: "warning",
            },
          ]);
        });

        it("defineModel", () => {
          const { s } = parse(`{ setup(){ defineModel() }}`);
          expect(s.original).toBe(s.toString());
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
