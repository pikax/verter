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

    const r = processTemplate(templateBlock.result.items, [...DefaultPlugins.filter(x => x.name !== 'VerterContext')], {
      ...options,
      s,
      filename: "test.vue",
      blocks: parsed.blocks,
      block: templateBlock,
      blockNameResolver: (name) => name,
    });

    return r;
  }

  it("should handle event", () => {
    const { result } = parse(`<div @click="test.toString()" />`);
    expect(result).toContain(
      `onClick={(...___VERTER___eventArgs)=>___VERTER___eventCallbacks(___VERTER___eventArgs,($event)=>$event&&0?undefined:___VERTER___ctx.test.toString())}`
    );
  });

  it("string", () => {
    const { result } = parse(`<div @click="'foo'" />`);
    expect(result).toContain(
      `onClick={(...___VERTER___eventArgs)=>___VERTER___eventCallbacks(___VERTER___eventArgs,($event)=>$event&&0?undefined:'foo')}`
    );
  });
  it("string interpolation", () => {
    const { result } = parse(`<div @click="\`foo\${'test'}\`" />`);

    expect(result).toContain(
      "onClick={(...___VERTER___eventArgs)=>___VERTER___eventCallbacks(___VERTER___eventArgs,($event)=>$event&&0?undefined:`foo${'test'}`)"
    );
  });

  it("ternary", () => {
    const { result } = parse(`<div @click="foo ? 'bar' : 'baz'" />`);

    expect(result).toContain(
      `onClick={(...___VERTER___eventArgs)=>___VERTER___eventCallbacks(___VERTER___eventArgs,($event)=>$event&&0?undefined:___VERTER___ctx.foo ? 'bar' : 'baz')}`
    );
  });

  it("narrow", () => {
    const { result } = parse(
      `<div v-if="typeof msg === 'string'" @click="msg.toLowerCase()" @auxclick="() => msg.toLowerCase()"></div>`
    );

    expect(result).toContain(
      `onClick={(...___VERTER___eventArgs)=>___VERTER___eventCallbacks(___VERTER___eventArgs,($event)=>$event&&0?undefined:!((typeof ___VERTER___ctx.msg === 'string'))? undefined :___VERTER___ctx.msg.toLowerCase())}`
    );
  });

  it("deep narrow with narrow:true", () => {
    const { result } = parse(
      `<div v-if="typeof msg === 'string'" @click="msg.toLowerCase()" @auxclick="() => msg.toLowerCase()"></div>`,
      { narrow: true }
    );

    expect(result).toContain(
      `onClick={(...___VERTER___eventArgs)=>___VERTER___eventCallbacks(___VERTER___eventArgs,($event)=>$event&&0?undefined:!((typeof ___VERTER___ctx.msg === 'string'))? undefined :___VERTER___ctx.msg.toLowerCase())}`
    );
  });

  describe("vue", () => {
    it("@vue:mounted", () => {
      const { result } = parse(`<div @vue:mounted="test" />`);
      expect(result).toContain(
        `<div onVue:mounted={___VERTER___ctx.test} />`
      );
    });
  });

  describe("not events", () => {
    test("no call", () => {
      const { result } = parse(`<div @click="test" />`);
      expect(result).toContain(`<div onClick={___VERTER___ctx.test} />`);
    });

    test("partial", () => {
      const { result } = parse(`<div @click="test.foo" />`);
      expect(result).toContain(`<div onClick={___VERTER___ctx.test.foo} />`);
    });

    test("function", () => {
      const { result } = parse(`<div @click="()=>test" />`);
      expect(result).toContain(`<div onClick={()=>___VERTER___ctx.test} />`);
    });

    test("object", () => {
      const { result } = parse(`<div @click="{test}" />`);
      expect(result).toContain(`<div onClick={{test: ___VERTER___ctx.test}} />`);
    });
  });
});
