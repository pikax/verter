import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";

import { ScriptBlockPlugin } from "./script-block.js";


describe('process script plugin script block', () => {
function parse(
    content: string,
    wrapper: string | false = false,
    lang = "js",

    pre = '',
    post = ''
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
        ScriptBlockPlugin,
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
      }
    );

    return r;
  }


  it('leave the script untouched', ()=> {
    const { result} = parse(`let a = 0`, false, 'js', '<template></template>', '<style></style>');
    expect(result).toMatchInlineSnapshot(`"<template></template><script setup lang="js">let a = 0</script><style></style>"`)
  })

  it('multiple script', ()=> {
    const { result} = parse(`let a = 0`, false, 'js', '<template></template>', '<script>first = 0;</script><style></style>');
    expect(result).toMatchInlineSnapshot(`"first = 0;<template></template><script setup lang="js">let a = 0</script><style></style>"`)
  })
})