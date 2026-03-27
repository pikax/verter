/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests ComponentInstancePlugin:
 * - Generates instance type exports for setup components
 * - Handles generic component declarations
 * - Creates properly typed Component export with constructor
 */
import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";
import { ProcessItemType } from "../../../types";

import { ComponentInstancePlugin } from "./index.js";
import { ScriptBlockPlugin } from "../script-block";
import { ScriptDefaultPlugin } from "../script-default";
import { BindingPlugin } from "../binding";
import { TemplateBindingPlugin } from "../template-binding";
import { MacrosPlugin } from "../macros";
import { AttributesPlugin } from "../attributes";
import { DefineOptionsPlugin } from "../define-options";

describe("process ComponentInstancePlugin", () => {
  function _parse(
    content: string,
    wrapper: string | boolean = false,
    lang = "ts",
    pre = "",
    post = "",
    attrs = "",
  ) {
    const prepend = `${pre}<script ${
      wrapper === false ? "setup" : ""
    } lang="${lang}"${attrs ? ` ${attrs}` : ""}>`;
    const source = `${prepend}${content}</script>${post}`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const scriptBlock = parsed.blocks.find((x) => x.type === "script") as ParsedBlockScript;

    const r = processScript(
      scriptBlock.result.items,
      [
        MacrosPlugin,
        TemplateBindingPlugin,
        ScriptBlockPlugin,
        BindingPlugin,
        AttributesPlugin,
        ScriptDefaultPlugin,
        DefineOptionsPlugin,
        ComponentInstancePlugin,
      ],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        isSetup: wrapper === false,
        block: scriptBlock,
        generic: parsed.generic, // Pass generic info from parser
        blockNameResolver: (name) => name,
      },
    );

    return r;
  }

  describe("setup components", () => {
    function parse(content: string, lang = "ts", attrs = "") {
      return _parse(content, false, lang, "", "", attrs);
    }

    it("generates instance type export", () => {
      const { result } = parse(`const foo = 1`);

      // Should export Instance type
      expect(result).toContain(`export type ___VERTER___Instance =`);

      // Should reference default_Component
      expect(result).toContain(`InstanceType<typeof ___VERTER___default_Component>`);

      // Should use PublicInstanceFromMacro helper
      expect(result).toContain(`___VERTER___PublicInstanceFromMacro<`);
    });

    it("generates Component export with constructor", () => {
      const { result } = parse(`const foo = 1`);

      // Should export Component const
      expect(result).toContain(`export declare const ___VERTER___Component:`);

      // Should have constructor type with optional props parameter that references Instance['$props']
      expect(result).toMatch(
        /\{\s*new\(props\?:\s*___VERTER___Instance\['\$props'\]\):\s*___VERTER___Prettify<___VERTER___Instance>\s*\}/,
      );
    });

    // @ai-generated - Tests the constructor accepts props for proper type inference
    it("generates constructor with props parameter for type inference", () => {
      const { result } = parse(`const foo = 1`);

      // Constructor should accept props? parameter
      expect(result).toMatch(/new\(props\?: ___VERTER___Instance\['\$props'\]\)/);

      // Should return the correct Instance type wrapped in Prettify
      expect(result).toMatch(/\):\s*___VERTER___Prettify<___VERTER___Instance>\s*\}/);
    });

    it("generates TEST instance type for dev mode", () => {
      const { result } = parse(`const foo = 1`);

      // Should export Instance_TEST type for development
      expect(result).toContain(`export type ___VERTER___Instance_TEST =`);

      // Should also include InstanceType<typeof default_Component> in TEST type
      expect(result).toContain(
        `export type ___VERTER___Instance_TEST = Omit<InstanceType<typeof ___VERTER___default_Component>,"$"|"$data"|"$props"|"$attrs"|"$refs"|"$options"|"$emit"|"$el"|"$slots">`,
      );
    });

    it("adds PublicInstanceFromMacro import", () => {
      const { context } = parse(`const foo = 1`);

      expect(context.items).toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            type: ProcessItemType.Import,
            from: "$verter/types$",
            items: expect.arrayContaining([
              expect.objectContaining({ name: "PublicInstanceFromMacro" }),
            ]),
          }),
        ]),
      );
    });

    it("includes TemplateBinding in instance type", () => {
      const { result } = parse(`const foo = 1`);

      expect(result).toContain(`___VERTER___TemplateBinding`);
    });

    it("includes attributes in instance type", () => {
      const { result } = parse(`const foo = 1`);

      expect(result).toContain(`___VERTER___attributes`);
    });

    it("merges defineOptions with instance type", () => {
      const { result } = parse(`defineOptions({ name: 'TestComp' }); const foo = 1`);

      // Should include InstanceType from the defineComponent export
      expect(result).toContain(`InstanceType<typeof ___VERTER___default_Component>`);

      // Should include PublicInstanceFromMacro helper
      expect(result).toContain(`___VERTER___PublicInstanceFromMacro<`);

      // Instance type should be an intersection of Omit<InstanceType<...>> and PublicInstanceFromMacro
      expect(result).toMatch(
        /Omit<InstanceType<[^>]+>[^>]+>\s*&\s*___VERTER___PublicInstanceFromMacro/,
      );
    });
  });

  describe("generic components", () => {
    function parse(content: string, generic: string) {
      return _parse(content, false, "ts", "", "", `generic="${generic}"`);
    }

    it("handles simple generic declaration", () => {
      const { result } = parse(`defineProps<{ value: T }>()`, "T");

      // Should include generic in Instance type declaration (with sanitised prefix)
      expect(result).toContain(`export type ___VERTER___Instance<__VERTER__TS__T = any>`);

      // Should include generic in Component constructor with props parameter
      expect(result).toMatch(
        /new<__VERTER__TS__T = any>\(props\?:\s*___VERTER___Instance<__VERTER__TS__T>\['\$props'\]\):\s*___VERTER___Prettify<___VERTER___Instance<__VERTER__TS__T>>/,
      );
    });

    // @ai-generated - Tests that generic constructor properly infers type from props
    it("generates constructor with generic props for type inference", () => {
      const { result } = parse(`defineProps<{ value: T }>()`, "T");

      // Constructor should have generic parameter
      expect(result).toMatch(/new<[^>]+>/);

      // Constructor should accept props? parameter with generic Instance
      expect(result).toMatch(/new<[^>]+>\(props\?: ___VERTER___Instance<[^>]+>\['\$props'\]\)/);

      // Constructor should return generic Instance wrapped in Prettify
      expect(result).toMatch(/\):\s*___VERTER___Prettify<___VERTER___Instance<[^>]+>>\s*\}/);
    });

    it("handles generic with extends constraint", () => {
      const { result } = parse(`defineProps<{ name: T }>()`, "T extends string");

      // Should include full generic declaration with constraint
      expect(result).toContain(`<__VERTER__TS__T extends string = any>`);
    });

    it("handles multiple generics", () => {
      const { result } = parse(`defineProps<{ key: K; value: V }>()`, "K extends string, V");

      // Should include multiple generics in declaration
      expect(result).toContain(`<__VERTER__TS__K extends string = any, __VERTER__TS__V = any>`);

      // Should include sanitised names in usage
      expect(result).toContain(`<__VERTER__TS__K,__VERTER__TS__V>`);
    });

    it("passes generics to TemplateBinding", () => {
      const { result } = parse(`defineProps<{ value: T }>()`, "T");

      // TemplateBinding should receive generic parameters
      expect(result).toContain(`___VERTER___TemplateBinding<__VERTER__TS__T>`);
    });

    it("generates Instance_TEST with generics", () => {
      const { result } = parse(`defineProps<{ value: T }>()`, "T");

      // TEST type should also have generics
      expect(result).toContain(`export type ___VERTER___Instance_TEST<__VERTER__TS__T = any>`);

      // TEST type should also include InstanceType<typeof default_Component>
      expect(result).toContain(
        `export type ___VERTER___Instance_TEST<__VERTER__TS__T = any> = Omit<InstanceType<typeof ___VERTER___default_Component>,"$"|"$data"|"$props"|"$attrs"|"$refs"|"$options"|"$emit"|"$el"|"$slots">`,
      );
    });
  });

  describe("options (non-setup)", () => {
    function parse(content: string) {
      return _parse(content, "");
    }

    it("generates Instance type for options API", () => {
      const { result } = parse(`export default defineComponent({ props: { foo: String } })`);

      // Should export Instance type using InstanceType of default_Component
      expect(result).toContain(`export type ___VERTER___Instance`);
      expect(result).toContain(`InstanceType<typeof ___VERTER___default_Component>`);
    });

    it("generates Component constructor for options API", () => {
      const { result } = parse(`export default defineComponent({ props: { foo: String } })`);

      // Should export Component with constructor
      expect(result).toContain(`export declare const ___VERTER___Component:`);
      expect(result).toContain(
        `___VERTER___OmitConstructorSignature<typeof ___VERTER___default_Component>`,
      );
      expect(result).toMatch(
        /new\(props\?:\s*___VERTER___Instance\['\$props'\]\):\s*___VERTER___Prettify<___VERTER___Instance>/,
      );
    });

    it("does not include PublicInstanceFromMacro for options API", () => {
      const { result } = parse(`export default defineComponent({ props: { foo: String } })`);

      // Options API should NOT use macro-based instance type
      expect(result).not.toContain(`PublicInstanceFromMacro`);
    });

    it("handles bare object export (no defineComponent wrapper)", () => {
      const { result } = parse(`export default { props: { foo: String } }`);

      // ScriptDefaultPlugin wraps with defineComponent, so Instance should still work
      expect(result).toContain(`export type ___VERTER___Instance`);
      expect(result).toContain(`InstanceType<typeof ___VERTER___default_Component>`);
    });

    it("does not generate generic declarations for options API", () => {
      const { result } = parse(`export default defineComponent({ props: { foo: String } })`);

      // No generic parameters in Instance or Component types
      expect(result).not.toMatch(/___VERTER___Instance</);
      expect(result).not.toMatch(/___VERTER___Component.*<[A-Z]/);
    });
  });
});
