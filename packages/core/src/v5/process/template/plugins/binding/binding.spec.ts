import { parser } from "../../../../parser";
import { parseTemplate } from "../../../../parser/template";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { extractBlocksFromDescriptor } from "../../../../utils";
import { processTemplate } from "../../template";
import { BindingPlugin } from "./index";
import { MagicString, parse as parseSFC } from "@vue/compiler-sfc";

describe("process template plugins binding", () => {
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
        BindingPlugin,
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

  it("should handle binding", () => {
    const { result } = parse(`{{ test }}`);
    expect(result).toMatchInlineSnapshot(`"{{ ___VERTER___ctx.test }}"`);
  });

  test("directive", () => {
    const { result } = parse(`<div :test="test" />`);
    expect(result).toMatchInlineSnapshot(
      `"<div :test="___VERTER___ctx.test" />"`
    );
  });

  test("function", () => {
    const { result } = parse(`<div @click="test" />`);
    expect(result).toMatchInlineSnapshot(
      `"<div @click="___VERTER___ctx.test" />"`
    );
  });

  test("for", () => {
    const { result } = parse(
      `<div v-for="item in items"> {{ item + items.length}} </div>`
    );
    expect(result).toMatchInlineSnapshot(
      `"<div v-for="item in ___VERTER___ctx.items"> {{ item + ___VERTER___ctx.items.length}} </div>"`
    );
  });

  test("if", () => {
    const { result } = parse(`<div v-if="test"> test </div>`);
    expect(result).toMatchInlineSnapshot(
      `"<div v-if=\"___VERTER___ctx.test\"> test </div>"`
    );
  });

  test("args", () => {
    const { result } = parse(`<div :test="(e: Argument)=> e + test" />`);
    expect(result).toMatchInlineSnapshot(
      `"<div :test="(e: ___VERTER___ctx.Argument)=> e + ___VERTER___ctx.test" />"`
    );
  });

  describe("nested", () => {
    test("{{ { test } }}", () => {
      const { result } = parse(`{{ { test } }}`);
      expect(result).toMatchInlineSnapshot(
        `"{{ { test: ___VERTER___ctx.test } }}"`
      );
    });

    test("{{ [ test ] }}", () => {
      const { result } = parse(`{{ [ test ] }}`);
      expect(result).toMatchInlineSnapshot(`"{{ [ ___VERTER___ctx.test ] }}"`);
    });
  });
});
