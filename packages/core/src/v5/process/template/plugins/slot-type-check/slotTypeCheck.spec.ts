import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { processTemplate, TemplateContext } from "../../template";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { SlotTypeCheckPlugin } from "./index.js";

describe("process template plugins slot-type-check", () => {
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
        SlotTypeCheckPlugin,
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
        block: templateBlock,blockNameResolver: (name) => name,
      }
    );

    return r;
  }

  it("should render", () => {
    const { result } = parse(`<Comp> <div /><div /></Comp>`);
    expect(result).toMatchInlineSnapshot(
      `"<Comp> <div /><div /></Comp>"`
    );
  });

  it("slot rendering", () => {
    const { result } = parse(
      `<Comp> <template v-slot:foo> <div /> </template> </Comp>`
    );
    expect(result).toMatchInlineSnapshot(
      `"<Comp> <template v-slot:foo> <div /> </template> </Comp>"`
    );
  });

  it("v-slot", () => {
    const { result } = parse(`<Comp v-slot:foo><div /></Comp>`);
    expect(result).toMatchInlineSnapshot(
      `"<Comp v-slot:foo><div /></Comp>"`
    );
  });
});
