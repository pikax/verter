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
    describe("v-once", () => {
      it("div v-once", () => {
        const { result } = parse(`<div v-once />`);
        expect(result).toMatchInlineSnapshot(`"<div v-once />"`);
      });
    });
    describe("v-pre", () => {
      it("div v-pre", () => {
        const { result } = parse(`<div v-pre />`);
        expect(result).toMatchInlineSnapshot(`"<div v-pre />"`);
      });
    });
    describe("v-cloak", () => {
      it("div v-cloak", () => {
        const { result } = parse(`<div v-cloak />`);
        expect(result).toMatchInlineSnapshot(`"<div v-cloak />"`);
      });
    });
    describe("v-show", () => {
      it('div v-show="foo"', () => {
        const { result } = parse(`<div v-show="foo" />`);
        expect(result).toMatchInlineSnapshot(
          `"<div v-show={___VERTER___ctx.foo} />"`
        );
      });
    });
    describe("v-html", () => {
      it('div v-html="foo"', () => {
        const { result } = parse(`<div v-html="foo" />`);
        expect(result).toMatchInlineSnapshot(
          `"<div v-html={___VERTER___ctx.foo} />"`
        );
      });
    });
  });

  describe("bespoke directives", () => {
    it("div v-test", () => {
      const { result } = parse(`<div v-test />`);

      expect(result).toMatchInlineSnapshot(
        `"<div {...{[___VERTER___instancePropertySymbol]:(___VERTER___slotInstance)=>{const ___VERTER___instanceToDirectiveVar=___VERTER___instanceToDirectiveFn(___VERTER___slotInstance);const ___VERTER___directiveName=___VERTER___instanceToDirectiveVar(___VERTER___directiveAccessor.vTest);}}} />"`
      );
    });

    it("div v-test:foo", () => {
      const { result } = parse(`<div v-test:foo />`);

      expect(result).toMatchInlineSnapshot(
        `"<div {...{[___VERTER___instancePropertySymbol]:(___VERTER___slotInstance)=>{const ___VERTER___instanceToDirectiveVar=___VERTER___instanceToDirectiveFn(___VERTER___slotInstance);const ___VERTER___directiveName=___VERTER___instanceToDirectiveVar(___VERTER___directiveAccessor.vTest);___VERTER___directiveName.arg="foo";}}} />"`
      );
    });

    it("div v-test:foo.app", () => {
      const { result } = parse(`<div v-test:foo.app />`);

      expect(result).toMatchInlineSnapshot(
        `"<div {...{[___VERTER___instancePropertySymbol]:(___VERTER___slotInstance)=>{const ___VERTER___instanceToDirectiveVar=___VERTER___instanceToDirectiveFn(___VERTER___slotInstance);const ___VERTER___directiveName=___VERTER___instanceToDirectiveVar(___VERTER___directiveAccessor.vTest);___VERTER___directiveName.arg="foo";___VERTER___directiveName.modifiers=["app"];}}} />"`
      );
    });

    it("div v-test:foo.app.baz", () => {
      const { result } = parse(`<div v-test:foo.app.baz />`);

      expect(result).toMatchInlineSnapshot(
        `"<div {...{[___VERTER___instancePropertySymbol]:(___VERTER___slotInstance)=>{const ___VERTER___instanceToDirectiveVar=___VERTER___instanceToDirectiveFn(___VERTER___slotInstance);const ___VERTER___directiveName=___VERTER___instanceToDirectiveVar(___VERTER___directiveAccessor.vTest);___VERTER___directiveName.arg="foo";___VERTER___directiveName.modifiers=["app","baz"];}}} />"`
      );
    });

    it('div v-test="bar"', () => {
      const { result } = parse(`<div v-test="bar" />`);

      expect(result).toMatchInlineSnapshot(
        `"<div {...{[___VERTER___instancePropertySymbol]:(___VERTER___slotInstance)=>{const ___VERTER___instanceToDirectiveVar=___VERTER___instanceToDirectiveFn(___VERTER___slotInstance);const ___VERTER___directiveName=___VERTER___instanceToDirectiveVar(___VERTER___directiveAccessor.vTest);___VERTER___directiveName.value=___VERTER___ctx.bar;}}} />"`
      );
    });

    it('div v-test:foo="bar"', () => {
      const { result } = parse(`<div v-test:foo="bar" />`);

      expect(result).toMatchInlineSnapshot(
        `"<div {...{[___VERTER___instancePropertySymbol]:(___VERTER___slotInstance)=>{const ___VERTER___instanceToDirectiveVar=___VERTER___instanceToDirectiveFn(___VERTER___slotInstance);const ___VERTER___directiveName=___VERTER___instanceToDirectiveVar(___VERTER___directiveAccessor.vTest);___VERTER___directiveName.arg="foo";___VERTER___directiveName.value=___VERTER___ctx.bar;}}} />"`
      );
    });

    it('div v-test:foo.app="bar"', () => {
      const { result } = parse(`<div v-test:foo.app="bar" />`);

      expect(result).toMatchInlineSnapshot(
        `"<div {...{[___VERTER___instancePropertySymbol]:(___VERTER___slotInstance)=>{const ___VERTER___instanceToDirectiveVar=___VERTER___instanceToDirectiveFn(___VERTER___slotInstance);const ___VERTER___directiveName=___VERTER___instanceToDirectiveVar(___VERTER___directiveAccessor.vTest);___VERTER___directiveName.arg="foo";___VERTER___directiveName.modifiers=["app"];___VERTER___directiveName.value=___VERTER___ctx.bar;}}} />"`
      );
    });

    it('div v-test:foo.app.baz="bar"', () => {
      const { result } = parse(`<div v-test:foo.app.baz="bar" />`);
      expect(result).toMatchInlineSnapshot(
        `"<div {...{[___VERTER___instancePropertySymbol]:(___VERTER___slotInstance)=>{const ___VERTER___instanceToDirectiveVar=___VERTER___instanceToDirectiveFn(___VERTER___slotInstance);const ___VERTER___directiveName=___VERTER___instanceToDirectiveVar(___VERTER___directiveAccessor.vTest);___VERTER___directiveName.arg="foo";___VERTER___directiveName.modifiers=["app","baz"];___VERTER___directiveName.value=___VERTER___ctx.bar;}}} />"`
      );
    });

    it('div v-if="foo" v-test="foo"', () => {
      const { result } = parse(`<div v-if="foo" v-test="foo" />`);

      expect(result).toMatchInlineSnapshot(
        `"{()=>{if(___VERTER___ctx.foo){<div  {...{[___VERTER___instancePropertySymbol]:(___VERTER___slotInstance)=>{if(!((___VERTER___ctx.foo))) return;const ___VERTER___instanceToDirectiveVar=___VERTER___instanceToDirectiveFn(___VERTER___slotInstance);const ___VERTER___directiveName=___VERTER___instanceToDirectiveVar(___VERTER___directiveAccessor.vTest);___VERTER___directiveName.value=___VERTER___ctx.foo;}}} />}}}"`
      );
    });

    it('div v-test.app="bar"', () => {
      const { result } = parse(`<div v-test.app="bar" />`);

      expect(result).toMatchInlineSnapshot(
        `"<div {...{[___VERTER___instancePropertySymbol]:(___VERTER___slotInstance)=>{const ___VERTER___instanceToDirectiveVar=___VERTER___instanceToDirectiveFn(___VERTER___slotInstance);const ___VERTER___directiveName=___VERTER___instanceToDirectiveVar(___VERTER___directiveAccessor.vTest);___VERTER___directiveName.modifiers=["app"];___VERTER___directiveName.value=___VERTER___ctx.bar;}}} />"`
      );
    });
  });
});
