import { parser } from "../../../../parser";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { processTemplate } from "../../template";
import { InterpolationPlugin } from "./index";
import { MagicString, parse as parseSFC } from "@vue/compiler-sfc";

describe("process template plugins interpolation", () => {
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
        InterpolationPlugin,
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

  it("should handle interpolation", () => {
    const { result } = parse(`{{ test }}`);
    expect(result).toMatchInlineSnapshot(`"{ test }"`);
  });

  it("should handle interpolation with spaces", () => {
    const { result } = parse(`{{  test  }}`);
    expect(result).toMatchInlineSnapshot(`"{  test  }"`);
  });

  it("should handle interpolation with spaces and new lines", () => {
    const { result } = parse(`{{  test\n  }}`);
    expect(result).toMatchInlineSnapshot(`
      "{  test
        }"
    `);
  });

  it("without spaces", () => {
    const { result } = parse(`{{test}}`);
    expect(result).toMatchInlineSnapshot(`"{test}"`);
  });
});
