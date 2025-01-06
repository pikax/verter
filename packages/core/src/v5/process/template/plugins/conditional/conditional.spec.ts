import { parser } from "../../../../parser";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { processTemplate, TemplateContext } from "../../template";
import { MagicString, parse as parseSFC } from "@vue/compiler-sfc";
import { ConditionalPlugin } from "./index";

describe("process template plugins conditional", () => {
  function parse(content: string, options: Partial<TemplateContext> = {}) {
    const source = `<template>${content}</template>`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const templateBlock = parsed.blocks.find(
      (x) => x.type === "template"
    ) as ParsedBlockTemplate;

    const r = processTemplate(
      templateBlock.result.items,
      [
        ConditionalPlugin,
        // clean template tag
        {
          post: (s) => {
            s.remove(0, "<template>".length);
            s.remove(source.length - "</template>".length, source.length);
          },
        },
      ],
      {
        ...options,
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
      }
    );

    return r;
  }

  it("v-if", () => {
    const { result } = parse(
      `<div v-if="typeof test === 'string'" :test="()=>test" />`
    );
    expect(result).toMatchInlineSnapshot(
      `"{(typeof test === 'string')?<div  :test="()=>test" />: undefined}"`
    );
  });

  it.only("v-if v-else", () => {
    const { result } = parse(
      `<div v-if="typeof test === 'string'" :test="()=>test" />
        <div v-else :test="()=>test" />`
    );
    expect(result).toMatchInlineSnapshot(`"<div v-else test={()=>test} />"`);
  });

  it("v-if narrow prop", () => {
    const { result } = parse(
      `<div v-if="typeof test === 'string'" :test="()=>test" />`
    );
    expect(result).toMatchInlineSnapshot(
      `"<div v-if="typeof test === 'string'" test={()=>test} />"`
    );
  });

  //   it("v-else-if", () => {
  //     const { result } = parse(
  //       `<div v-else-if="typeof test === 'string'" :test="()=>test" />`
  //     );
  //     expect(result).toMatchInlineSnapshot(
  //       `"<div v-else-if="typeof test === 'string'" test={()=>test} />"`
  //     );
  //   });

  //   it("v-else", () => {
  //     const { result } = parse(`<div v-else :test="()=>test" />`);
  //     expect(result).toMatchInlineSnapshot(`"<div v-else test={()=>test} />"`);
  //   });

  it("v-if v-else-if v-else", () => {
    const { result } = parse(
      `<div v-if="typeof test === 'string'" :test="()=>test" />

        <div v-else-if="typeof test === number" :test="()=>test" />

        <div v-else :test="()=>test" />`
    );
    expect(result).toMatchInlineSnapshot(`"<div v-else test={()=>test} />"`);
  });

  it("deep", () => {
    const { result } = parse(
      `<div v-if="typeof test === 'string'" :test="()=>test" />
        <div v-else-if="typeof test === number" :test="()=>test" />
        <div v-else :test="()=>test" />
    <div v-else>else</div>`
    );
    expect(result).toMatchInlineSnapshot(`"<div v-else test={()=>test} />"`);
  });
});
