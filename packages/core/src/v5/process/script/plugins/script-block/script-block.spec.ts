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
      s,
      filename: "test.vue",
      blocks: parsed.blocks,
      block: scriptBlock,
      isAsync: parsed.isAsync,
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
      `"<template></template>function script  (){let a = 0}<style></style>"`
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
    expect(result).toMatchInlineSnapshot(
      `"first = 0;<template></template>function script  (){let a = 0}<style></style>"`
    );
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
      `"<template></template>async function script  (){await fetch('')}<style></style>"`
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
      `"<template></template>function script   <T>(){let a = {} as unknown as T}<style></style>"`
    );  
  });
});
