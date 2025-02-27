import { parser } from "../../../../parser";
import { parseTemplate } from "../../../../parser/template";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { extractBlocksFromDescriptor } from "../../../../utils";
import { processTemplate } from "../../template";
import { TemplateTagPlugin } from "./index";
import { MagicString, parse as parseSFC } from "@vue/compiler-sfc";

describe("process template plugins comment", () => {
  function parse(content: string) {
    const source = `<template>${content}</template>`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const templateBlock = parsed.blocks.find(
      (x) => x.type === "template"
    ) as ParsedBlockTemplate;

    const r = processTemplate(
      templateBlock.result.items,
      [
        TemplateTagPlugin
      ],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        block: templateBlock,
        blockNameResolver: (name) => name,
        generic: parsed.generic,
      }
    );

    return r;
  }
  // TODO add more tests
  it('should replace tag', ()=> {
    const { result } = parse("<div></div>");

    expect(result).toMatchInlineSnapshot(`
      "export function template(){
      <><div></div>
      </>}"
    `);
  })


});
