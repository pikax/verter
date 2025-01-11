import { DefaultPlugins } from "../..";
import { parser } from "../../../../parser";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { processTemplate, TemplateContext } from "../../template";
import { MagicString, parse as parseSFC } from "@vue/compiler-sfc";

describe("process template plugins directive", () => {
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

  describe("v-model", () => {
    it('div v-model="foo"', () => {
      const { result } = parse(`<div v-model="foo" />`);
      expect(result).toMatchInlineSnapshot(
        `"<div value={___VERTER___ctx.foo} onUpdate:modelValue={($event)=>(___VERTER___ctx.foo=$event)} />"`
      );
    });

    it('Comp v-model="foo"', () => {
      const { result } = parse(`<Comp v-model="foo" />`);
      expect(result).toMatchInlineSnapshot(
        `"<Comp modelValue={___VERTER___ctx.foo} onUpdate:modelValue={($event)=>(___VERTER___ctx.foo=$event)} />"`
      );
    });

    it('Comp v-model="foo" modelValue="bar"', () => {
      const { result } = parse(`<Comp v-model="foo" modelValue="bar" />`);
      expect(result).toMatchInlineSnapshot(
        `"<Comp modelValue={___VERTER___ctx.foo} onUpdate:modelValue={($event)=>(___VERTER___ctx.foo=$event)} modelValue={"bar"} />"`
      );
    });

    it('Comp v-model:msg="foo"', () => {
      const { result } = parse(`<Comp v-model:msg="foo" />`);
      expect(result).toMatchInlineSnapshot(
        `"<Comp msg={___VERTER___ctx.foo} onUpdate:msg={($event)=>(___VERTER___ctx.foo=$event)} />"`
      );
    });

    it('Comp v-model:msg="foo" msg="bar"', () => {
      const { result } = parse(`<Comp v-model:msg="foo" msg="bar" />`);
      expect(result).toMatchInlineSnapshot(
        `"<Comp msg={___VERTER___ctx.foo} onUpdate:msg={($event)=>(___VERTER___ctx.foo=$event)} msg={"bar"} />"`
      );
    });

    it('Comp v-model:msg="foo" v-model="bar"', () => {
      const { result } = parse(`<Comp v-model:msg="foo" v-model="bar" />`);
      expect(result).toMatchInlineSnapshot(
        `"<Comp msg={___VERTER___ctx.foo} onUpdate:msg={($event)=>(___VERTER___ctx.foo=$event)} modelValue={___VERTER___ctx.bar} onUpdate:modelValue={($event)=>(___VERTER___ctx.bar=$event)} />"`
      );
    });

    it('Comp v-model:[msg]="foo"', () => {
      const { result } = parse(`<Comp v-model:[msg]="foo" />`);
      expect(result).toMatchInlineSnapshot(
        `"<Comp {...{[___VERTER___ctx.msg]:___VERTER___ctx.foo,[\`onUpdate:\${___VERTER___ctx.msg}\`]:($event)=>(___VERTER___ctx.foo=$event)}} />"`
      );
    });

    it('Comp v-model:[`${msg}ss`]="msg"', () => {
      const { result } = parse(`<Comp v-model:[\`\${msg}ss\`]="msg" />`);
      expect(result).toMatchInlineSnapshot(
        `"<Comp {...{[\`\${___VERTER___ctx.msg}ss\`]:___VERTER___ctx.msg,[\`onUpdate:\${\`\${___VERTER___ctx.msg}ss\`}\`]:($event)=>(___VERTER___ctx.msg=$event)}} />"`
      );
    });
  });

  describe("vue", () => {
    describe("v-text", () => {
      it('div v-text="foo"', () => {
        const { result } = parse(`<div v-text="foo" />`);
        expect(result).toMatchInlineSnapshot(
          `"<div v-text={___VERTER___ctx.foo} />"`
        );
      });
    });
    describe("v-once", () => {});
    describe("v-pre", () => {});
    describe("v-cloak", () => {});
    describe("v-show", () => {});
    describe("v-html", () => {});
  });

  describe("bespoke directives", () => {});
});
