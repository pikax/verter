import { parser } from "../../../../parser";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { processTemplate, TemplateContext } from "../../template";
import { MagicString, parse as parseSFC } from "@vue/compiler-sfc";
import { DefaultPlugins } from "../..";

describe("process template plugins event", () => {
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
        ...DefaultPlugins,
        // clean template tag
        {
          post: (s) => {
            s.update(0, "<template>".length, "");
            s.update(source.length - "</template>".length, source.length, "");
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

  it("should handle event", () => {
    const { result } = parse(`<div @click="test.toString()" />`);
    expect(result).toMatchInlineSnapshot(
      `"<div onClick={(...___VERTER___eventArgs)=>___VERTER___eventCb(___VERTER___eventArgs,($event)=>$event&&0?undefined:___VERTER___ctx.test.toString())} />"`
    );
  });

  it("string", () => {
    const { result } = parse(`<div @click="'foo'" />`);
    expect(result).toMatchInlineSnapshot(
      `"<div onClick={(...___VERTER___eventArgs)=>___VERTER___eventCb(___VERTER___eventArgs,($event)=>$event&&0?undefined:'foo')} />"`
    );
  });
  it("string interpolation", () => {
    const { result } = parse(`<div @click="\`foo\${'test'}\`" />`);

    expect(result).toMatchInlineSnapshot(
      `"<div onClick={(...___VERTER___eventArgs)=>___VERTER___eventCb(___VERTER___eventArgs,($event)=>$event&&0?undefined:\`foo\${'test'}\`)} />"`
    );
  });

  it("ternary", () => {
    const { result } = parse(`<div @click="foo ? 'bar' : 'baz'" />`);

    expect(result).toMatchInlineSnapshot(
      `"<div onClick={(...___VERTER___eventArgs)=>___VERTER___eventCb(___VERTER___eventArgs,($event)=>$event&&0?undefined:___VERTER___ctx.foo ? 'bar' : 'baz')} />"`
    );
  });

  describe("not events", () => {
    test("no call", () => {
      const { result } = parse(`<div @click="test" />`);
      expect(result).toMatchInlineSnapshot(
        `"<div onClick={___VERTER___ctx.test} />"`
      );
    });

    test("partial", () => {
      const { result } = parse(`<div @click="test.foo" />`);
      expect(result).toMatchInlineSnapshot(
        `"<div onClick={___VERTER___ctx.test.foo} />"`
      );
    });

    test("function", () => {
      const { result } = parse(`<div @click="()=>test" />`);
      expect(result).toMatchInlineSnapshot(
        `"<div onClick={()=>___VERTER___ctx.test} />"`
      );
    });

    test("object", () => {
      const { result } = parse(`<div @click="{test}" />`);
      expect(result).toMatchInlineSnapshot(
        `"<div onClick={{test: ___VERTER___ctx.test}} />"`
      );
    });
  });
});
