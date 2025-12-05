import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import {
  ParsedBlockScript,
  ParsedBlockTemplate,
} from "../../../../parser/types";
import { processScript } from "../../script";

import { TemplateBindingPlugin } from "./index";
import { ScriptBlockPlugin } from "../script-block";
import { BindingPlugin } from "../binding";
import { TemplateTypes } from "../../../../parser/template/types";

describe("process script plugin template-binding", () => {
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
    const template = parsed.blocks.find(
      (x) => x.type === "template"
    ) as ParsedBlockTemplate;

    const r = processScript(
      scriptBlock.result.items,
      [ScriptBlockPlugin, TemplateBindingPlugin, BindingPlugin],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        block: scriptBlock,
        isAsync: parsed.isAsync,
        templateBindings:
          template?.result?.items.filter(
            (x) => x.type === TemplateTypes.Binding
          ) ?? [],
        isSetup: wrapper === false,
        blockNameResolver: (name) => name,
      }
    );
    return r;
  }

  it("work", () => {
    const { result } = parse(`let a = 0`, false, "js");
    expect(result).toMatchInlineSnapshot(
      `
      "/** @returns {{}} */;function ___VERTER___TemplateBindingFN  (){let a = 0;return{}}
      /** @typedef {ReturnType<typeof ___VERTER___TemplateBindingFN>} ___VERTER___TemplateBinding */
      /** @type {___VERTER___TemplateBinding} */
      export const ___VERTER___TemplateBinding = null;
      "
    `
    );
  });

  it("template binding", () => {
    const { result } = parse(
      `let a = 0`,
      false,
      "js",
      "<template><div>{{ a }}</div></template>",
      ""
    );
    expect(result).toMatchInlineSnapshot(`
      "<template><div>{{ a }}</div></template>/** @returns {{a:___VERTER___UnwrapRef<typeof a>}} */;function ___VERTER___TemplateBindingFN  (){let a = 0;return{a/*67,68*/: ___VERTER___unref(a)}}
      /** @typedef {ReturnType<typeof ___VERTER___TemplateBindingFN>} ___VERTER___TemplateBinding */
      /** @type {___VERTER___TemplateBinding} */
      export const ___VERTER___TemplateBinding = null;
      "
    `);
  });

  it("ts template binding", () => {
    const { result } = parse(
      `let a = 0`,
      false,
      "ts",
      "<template><div>{{ a }}</div></template>",
      ""
    );

    expect(result).toMatchInlineSnapshot(
      `"<template><div>{{ a }}</div></template>;function ___VERTER___TemplateBindingFN  (){let a = 0;return{a/*67,68*/: a as unknown as ___VERTER___UnwrapRef<typeof a>,...{}}};export type ___VERTER___TemplateBinding=ReturnType<typeof ___VERTER___TemplateBindingFN>;"`
    );
  });

  it("async", () => {
    const { result } = parse(
      `let a = await Promise.resolve(0)`,
      false,
      "ts",
      "<template><div>{{ a }}</div></template>",
      ""
    );
    expect(result).toMatchInlineSnapshot(
      `"<template><div>{{ a }}</div></template>;function ___VERTER___TemplateBindingFN  (){let a = 0;return{a/*67,68*/: a as unknown as ___VERTER___UnwrapRef<typeof a>}};export type ___VERTER___TemplateBinding=ReturnType<typeof ___VERTER___TemplateBindingFN>;"`
    );
  });

  it("Component", () => {
    const { result } = parse(
      `import Comp from './Comp.vue'; let a = 0`,
      false,
      "js",
      "<template><Comp>{{ a }}</Comp></template>",
      ""
    );
    expect(result).toMatchInlineSnapshot(`"<template><div>{{ a }}</div></template>;async function ___VERTER___TemplateBindingFN  (){let a = await Promise.resolve(0);return{a/*67,68*/: a as unknown as ___VERTER___UnwrapRef<typeof a>}};export type ___VERTER___TemplateBinding=ReturnType<typeof ___VERTER___TemplateBindingFN> extends Promise<infer R>?R:never;"`);
  });
});
