import { parser } from "../../../../parser";
import { parseTemplate } from "../../../../parser/template";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { extractBlocksFromDescriptor } from "../../../../utils";
import { processTemplate } from "../../template";
import { TextPlugin } from "./index";
import { MagicString, parse as parseSFC } from "@vue/compiler-sfc";

describe("process template plugins text", () => {
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
        TextPlugin,
        // clean template tag
        {
          post: (s) => {
            s.remove(0, "<template>".length);
            s.remove(source.length - "</template>".length, source.length);
          },
        },
      ],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
      }
    );

    return r;
  }

  it("should handle text", () => {
    const { result } = parse(`test`);
    expect(result).toMatchInlineSnapshot(`"{ "test" }"`);
  });

  it("should not handle if starts < ", () => {
    const { result } = parse(`<`);
    expect(result).toMatchInlineSnapshot(`"<"`);
  });

  it("should parse text if has <", () => {
    const { result } = parse(`2 < 1`);
    expect(result).toMatchInlineSnapshot(`"{ "2 < 1" }"`);
  });

  it('should escape " in text', () => {
    const { result } = parse(`"`);
    expect(result).toMatchInlineSnapshot(`"{ "\\"" }"`);
  });
});
