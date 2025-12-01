/**
 * @ai-generated - This test file was updated with AI assistance.
 * Tests defineOptions macro transformation:
 * - Boxing of defineOptions arguments
 * - Moving defineOptions to start of script
 * - Handling both standalone calls and variable declarations
 */
import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";
import { ProcessItemType } from "../../../types";

import { DefineOptionsPlugin } from "./index.js";
import { TemplateBindingPlugin } from "../template-binding";
import { ScriptBlockPlugin } from "../script-block";
import { BindingPlugin } from "../binding";

describe("process defineOptions", () => {
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
      [
        DefineOptionsPlugin,
        TemplateBindingPlugin,
        ScriptBlockPlugin,
        BindingPlugin,
      ],
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

    it("should move and box defineOptions", () => {
      const { result, context } = parse(`defineOptions(myOptions)`);
      
      // Should declare the boxed variable
      expect(result).toContain(`let ___VERTER___defineOptions_Boxed;`);
      
      // Should box the options argument
      expect(result).toContain(`___VERTER___defineOptions_Boxed=___VERTER___defineOptions_Box(myOptions)`);
      
      // Should move to beginning (before template binding function)
      expect(result).toContain(`defineOptions(`);
      expect(result).toContain(`function ___VERTER___TemplateBindingFN`);
      
      // Should add helper import
      expect(context.items).toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            type: ProcessItemType.Import,
            from: "$verter/types$",
            items: expect.arrayContaining([
              expect.objectContaining({ name: "defineOptions_Box" }),
            ]),
          }),
        ])
      );
    });

    it("should move and box defineOptions with declaration", () => {
      const { result, context } = parse(`const foo = defineOptions(myOptions)`);
      
      // Should declare the boxed variable
      expect(result).toContain(`let ___VERTER___defineOptions_Boxed;`);
      
      // Should box the options argument
      expect(result).toContain(`___VERTER___defineOptions_Boxed=___VERTER___defineOptions_Box(myOptions)`);
      
      // Should keep original variable declaration structure
      expect(result).toContain(`const foo = defineOptions(`);
      
      // Should move to beginning (before template binding function)
      expect(result).toContain(`function ___VERTER___TemplateBindingFN`);
      
      // Should add helper import
      expect(context.items).toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            type: ProcessItemType.Import,
            from: "$verter/types$",
            items: expect.arrayContaining([
              expect.objectContaining({ name: "defineOptions_Box" }),
            ]),
          }),
        ])
      );
    });

    it("should handle object literal options", () => {
      const { result } = parse(`defineOptions({ inheritAttrs: false })`);
      
      // Should box the object literal
      expect(result).toContain(`___VERTER___defineOptions_Box({ inheritAttrs: false })`);
    });

    it("should handle complex options object", () => {
      const { result } = parse(`defineOptions({ 
        inheritAttrs: false,
        name: 'MyComponent',
        customOptions: { foo: 'bar' }
      })`);
      
      // Should contain the boxed declaration
      expect(result).toContain(`let ___VERTER___defineOptions_Boxed;`);
      expect(result).toContain(`___VERTER___defineOptions_Box(`);
    });

    it("should handle defineOptions with no arguments", () => {
      const { result, context } = parse(`defineOptions()`);
      
      // Should not transform or box when no arguments are provided
      // MacrosPlugin will handle validation
      expect(result).not.toContain(`___VERTER___defineOptions_Boxed`);
      expect(result).not.toContain(`___VERTER___defineOptions_Box`);
      
      // Original call should remain
      expect(result).toContain(`defineOptions()`);
    });
  });

  describe("options (non-setup)", () => {
    function parse(content: string, lang = "ts") {
      return _parse(content, "", lang);
    }

    it("should not process defineOptions in non-setup script (no-op)", () => {
      // defineOptions is only valid in <script setup>
      // In options API, it produces a warning and is left untransformed
      const { result } = parse(`defineOptions({ inheritAttrs: false })`);
      
      // The call should remain in the output unchanged
      expect(result).toContain(`defineOptions({ inheritAttrs: false })`);
    });
  });
});
