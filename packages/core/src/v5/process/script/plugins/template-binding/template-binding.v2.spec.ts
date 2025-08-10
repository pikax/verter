import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import {
  ParsedBlockScript,
  ParsedBlockTemplate,
} from "../../../../parser/types";
import { processScript } from "../../script";

import { TemplateBindingPlugin } from "./template-binding.v2";
import { ScriptBlockPlugin } from "../script-block";
import { BindingPlugin } from "../binding";
import { TemplateTypes } from "../../../../parser/template/types";
import { MacrosPlugin } from "../macros";

describe("process script plugins template-binding", () => {
  function _parse(
    content: string,
    wrapper: string | false = false,
    lang = "js",

    pre = "",
    post = "",
    attrs = ""
  ) {
    const prepend = `${pre}<script ${
      wrapper === false ? "setup" : ""
    } lang="${lang}"${attrs ? ` ${attrs}` : ""}>`;
    const source = `${prepend}${content}</script>${post}`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const scriptBlock = parsed.blocks.find(
      (x) => x.type === "script"
    ) as ParsedBlockScript;
    const template = parsed.blocks.find(
      (x) => x.type === "template"
    ) as ParsedBlockTemplate;

    const r = processScript(
      scriptBlock.result.items,
      [ScriptBlockPlugin, TemplateBindingPlugin, BindingPlugin, MacrosPlugin],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        block: scriptBlock,
        isAsync: parsed.isAsync,
        isTS: lang === "ts",
        isSetup: wrapper === false,
        templateBindings:
          template?.result?.items.filter(
            (x) => x.type === TemplateTypes.Binding
          ) ?? [],
        blockNameResolver: (name) => name,
      }
    );
    return r;
  }

  describe("options", () => {
    describe("ts", () => {
      function parse(content: string) {
        return _parse(content, "defineComponent", "ts");
      }

      it("should generate a function declaration", () => {
        const { result } = parse(`let a = 0`);
        expect(result).toMatchInlineSnapshot(`"let a = 0"`);
      });

      it("defineProps", () => {
        const { result } = parse(`defineProps({})`);
        expect(result).toMatchInlineSnapshot(`"defineProps({})"`);
      });
    });
  });

  describe("setup", () => {
    describe("ts", () => {
      function parse(content: string) {
        return _parse(content, false, "ts");
      }

      describe("defineProps", () => {
        it("defineProps()", () => {
          const { result } = parse(`defineProps()`);
          expect(result).toMatchInlineSnapshot(`"defineProps({})"`);
        });

        it("defineProps({foo:String})", () => {
          const { result } = parse(`defineProps({foo:String})`);
          expect(result).toMatchInlineSnapshot(`"defineProps({foo:String})"`);
        });

        it("defineProps<{foo:string}>()", () => {
          const { result } = parse(`defineProps<{foo:string}>()`);
          expect(result).toMatchInlineSnapshot(
            `"defineProps<{foo:string}>({})"`
          );
        });
      });

      describe("mix", () => {
        it.only("type based", () => {
          const { result } = parse(
            `defineProps<{foo:string}>();defineEmits<{bar: (a:{ foo: string})=>void}>();defineSlots<{default: (arg:{foo:string})=>void}>();defineExpose({arg: 'foo'});defineOptions({name:'foo'});defineModel<'foo'|'bar'>();`
          );
          expect(result).toMatchInlineSnapshot(
            `"defineProps<{foo:string}>({})"`
          );
        });
      });
    });
  });
});
