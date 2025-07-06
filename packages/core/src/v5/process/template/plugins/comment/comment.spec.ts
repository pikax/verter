import { parser } from "../../../../parser";
import { parseTemplate } from "../../../../parser/template";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { extractBlocksFromDescriptor } from "../../../../utils";
import { processTemplate } from "../../template";
import { CommentPlugin } from "./index";
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
        CommentPlugin,
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
        block: templateBlock,
        blockNameResolver: (name) => name,
      }
    );

    return r;
  }

  it("should return comment", () => {
    const { result } = parse("<!-- comment -->");

    expect(result).toBe("/* comment */");
  });

  it("it should keep spacing", () => {
    const { result } = parse("<!--comment-->\n");

    expect(result).toBe("/*comment*/\n");
  });

  it("should keep <!-- if inside comment", () => {
    const { result } = parse("<!-- <!-- -->");

    expect(result).toBe("{/* <!-- */}");
  });

  it("if containts < or > it should wrap in { }", () => {
    const { result } = parse("<!-- <MyComp -->");

    expect(result).toBe("{/* <MyComp */}");
  });
});
