import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";

import { ScriptBlockPlugin } from "./script-block.js";

describe("process script plugin script block", () => {
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
      (x) => x.type === "script"
    ) as ParsedBlockScript;

    const r = processScript(scriptBlock.result.items, [ScriptBlockPlugin], {
      ...parsed,
      s,
      filename: "test.vue",
      blocks: parsed.blocks,
      block: scriptBlock,
      isAsync: parsed.isAsync,
      blockNameResolver: (name) => name,
    });

    return r;
  }

  it("leave the script untouched", () => {
    const { result } = parse(
      `let a = 0`,
      false,
      "js",
      "<template></template>",
      "<style></style>"
    );
    expect(result).toMatchInlineSnapshot(
      `"<template></template>;function script  (){let a = 0}<style></style>"`
    );
  });

  it("multiple script", () => {
    const { result } = parse(
      `let a = 0`,
      false,
      "js",
      "<template></template>",
      "<script>first = 0;</script><style></style>"
    );
    expect(result).toContain("first = 0;");
    expect(result).toContain("<template></template>");
    expect(result).toContain("let a = 0");
    expect(result).toContain("<style></style>");
  });

  it("async", () => {
    const { result } = parse(
      `await fetch('')`,
      false,
      "js",
      "<template></template>",
      "<style></style>"
    );
    expect(result).toMatchInlineSnapshot(
      `"<template></template>;async function script  (){await fetch('')}<style></style>"`
    );
  });

  it("generic", () => {
    const { result } = parse(
      `let a = {} as unknown as T`,
      false,
      "js",
      "<template></template>",
      "<style></style>",
      'generic="T"'
    );
    expect(result).toMatchInlineSnapshot(
      `"<template></template>;function script   <T>(){let a = {} as unknown as T}<style></style>"`
    );
  });

  it("add ; before function", () => {
    const { result } = parse(
      `import {a} from 'b'
let a = 0`,
      false,
      "js"
    );
    expect(result).toMatchInlineSnapshot(
      `
      ";function script  (){import {a} from 'b'
      let a = 0}"
    `
    );
  });

  it("not move non-main script to the beginning if its the beginning", () => {
    const { result } = parse(
      `let a = 0`,
      false,
      "js",
      "<script>let first = 0;</script>"
    );
    expect(result).toContain("let first = 0;");
    expect(result).toContain("let a = 0");
  });

  describe("options", () => {
    test("remove tag", () => {
      const { result } = parse(`export default {}`, "", "js");
      // Options script should keep export default
      expect(result).toContain("export default {}");
    });
  });
});
