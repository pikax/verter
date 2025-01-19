import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";

import { BindingContextPlugin } from "./index.js";
import { MacrosPlugin } from "../macros";

describe("process script plugin binding context", () => {
  function _parse(
    content: string,
    wrapper: string | boolean = false,
    lang = "js",
    attributes = "",

    pre = "",
    post = ""
  ) {
    const prepend = `${pre}<script ${
      wrapper === false ? "setup" : ""
    } lang="${lang}" ${attributes}>`;
    const source = `${prepend}${content}</script>${post}`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const scriptBlock = parsed.blocks.find(
      (x) => x.type === "script"
    ) as ParsedBlockScript;

    const r = processScript(
      scriptBlock.result.items,
      [
        BindingContextPlugin,
        MacrosPlugin,
        // // clean template tag
        // {
        //   post: (s) => {
        //     s.update(pre.length, pre.length + prepend.length, "");
        //     s.update(source.length - "</script>".length - pos, source.length, "");
        //   },
        // },
      ],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        isSetup: wrapper === false,
        block: scriptBlock,
        generic: parsed.generic,
      }
    );

    return r;
  }

  //   describe("ts", () => {});

  //   it.only("test", () => {
  //     const { s } = _parse(`export default { setup(){ defineExpose({ a: 0 }); }}`, 'ddd');
  //     expect(s.original).toBe(s.toString());
  //   });
  
  describe('ts', ()=> {
    describe('setup', ()=> {
        function parse(content: string, attributes = '') {
            return _parse(content, false, 'ts', attributes);
        }
        it('work', ()=> {
            const {result} = parse('let a = 0')
            expect(result).toMatchInlineSnapshot(`"export function ___VERTER___BindingContext(){ let a = 0;return {a: typeof a}}"`)
        })

        it('attributes', ()=> {
            const {result} = parse('let a = 0;', 'attributes="HTMLAttributes"')
            expect(result).toMatchInlineSnapshot(`
              "export function ___VERTER___BindingContext(){ let a = 0;;return {a: typeof a}} /**
               * ___VERTER___ATTRIBUTES
               */type ___VERTER___attributes=HTMLAttributes;"
            `)
        })

        it('generic', ()=> {
            const {result} = parse('let a = 0;', 'generic="T"')
            expect(result).toMatchInlineSnapshot(`"export function ___VERTER___BindingContext<T>(){let a = 0;;return {a: typeof a}}"`)
        })

        it('generic and attributes', ()=> {
            const {result} = parse('let a = 0;', 'generic="T" attributes="T extends true ? HTMLAttributes : {}"')
            expect(result).toMatchInlineSnapshot(`
              "export function ___VERTER___BindingContext<T>(){let a = 0;;return {a: typeof a}} /**
               * ___VERTER___ATTRIBUTES
               */type ___VERTER___attributes=T extends true ? HTMLAttributes : {}T;"
            `)
        })

        it('attributes and generic', ()=> {
            const {result} = parse('let a = 0;', 'attributes="T extends true ? HTMLAttributes : {}" generic="T"')
            expect(result).toMatchInlineSnapshot(`
              "export function ___VERTER___BindingContext<T>(){let a = 0;;return {a: typeof a}} /**
               * ___VERTER___ATTRIBUTES
               */type ___VERTER___attributes=T extends true ? HTMLAttributes : {}T;"
            `)
        })

        it('defineProps', ()=> {
            const {result} = parse('const props = defineProps({ a: String })')
            expect(result).toMatchInlineSnapshot(`"export function ___VERTER___BindingContext(){ const props=___VERTER___Props = defineProps({ a: String });return {props: typeof props,___VERTER___Props: typeof ___VERTER___Props}}"`)
        })
    })

  })
});
