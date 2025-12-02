import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";

import { ScriptBlockPlugin } from "../script-block";
import { ImportsPlugin } from "../imports";
import { SFCCleanerPlugin } from "./index.js";

describe("process script plugin sfc-cleaner", () => {
  function parse(
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
      // @ts-expect-error TODO this should not be error because x.type narrows the type
      (x) => x.type === "script" && x.isMain
    ) as ParsedBlockScript;

    const r = processScript(
      scriptBlock.result.items,
      [ScriptBlockPlugin, ImportsPlugin, SFCCleanerPlugin],
      {
        ...parsed,
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        block: scriptBlock,
        isAsync: parsed.isAsync,
        blockNameResolver: (name) => name,
      }
    );

    return r;
  }

  it("comments non-script blocks (single line)", () => {
    const { result } = parse(
      `let a = 0`,
      false,
      "js",
      "<template></template>",
      "<style></style>"
    );
    expect(result).toMatchInlineSnapshot(`
      "// <template></template>
      ;function script  (){let a = 0}
      // <style></style>"
    `);
  });

  it("comments every line in other blocks", () => {
    const pre = `<template>\n<div>hi</div>\n</template>`;
    const post = `\n<style>\n.a { color: red }\n</style>`;
    const { result } = parse(`let a = 0`, false, "js", pre, post);
    expect(result).toMatchInlineSnapshot(`
      "// <template>
      // <div>hi</div>
      // </template>
      ;function script  (){let a = 0}

      // <style>
      // .a { color: red }
      // </style>"
    `);
  });

  it("handle /**/ comments", () => {
    const pre = `<script>/*let foo = 1;*/</script>`;
    const post = `<style>\n.a { color: red }</style>`;
    const { result } = parse(`let a = 0 /* comment */`, false, "js", pre, post);
    expect(result).toMatchInlineSnapshot(`
      "
      /*let foo = 1;*/

      ;function script  (){let a = 0 /* comment */}
      // <style>
      // .a { color: red }</style>"
    `);
  });

  it("handle multi line /* */ comments", () => {
    const pre = `<script>let foo = '1'/* comment line 1\n comment line 2 */</script>`;
    const post = `<style>.a { color: red }</style>`;
    const { result } = parse(`let a = 0 `, false, "js", pre, post);
    expect(result).toMatchInlineSnapshot(`
      "
      let foo = '1'/* comment line 1
       comment line 2 */

      ;function script  (){let a = 0 }
      // <style>.a { color: red }</style>"
    `);
  });

  it("not comment imports", () => {
    const pre = `<script>import vue from 'foo';\nlet foo = '1'</script>`;
    const post = `<style>.a { color: red }</style>`;
    const { result } = parse(`import foo from 'vue'`, false, "js", pre, post);
    expect(result).toContain("import foo from 'vue'");
    expect(result).toContain("import vue from 'foo';");
    expect(result).toContain("let foo = '1'");
  });
});
