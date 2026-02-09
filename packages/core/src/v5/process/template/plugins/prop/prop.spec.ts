import { parser } from "../../../../parser";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { processTemplate, TemplateContext } from "../../template";
import { BindingPlugin } from "../binding";
import { PropPlugin } from "./index";
import { MagicString, parse as parseSFC } from "@vue/compiler-sfc";

describe("process template plugins prop", () => {
  function parse(content: string, options: Partial<TemplateContext> = {}) {
    const source = `<template>${content}</template>`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const templateBlock = parsed.blocks.find((x) => x.type === "template") as ParsedBlockTemplate;

    const r = processTemplate(
      templateBlock.result.items,
      [
        PropPlugin,
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
        block: templateBlock,
        blockNameResolver: (name) => name,
      },
    );

    return r;
  }

  describe("prop", () => {
    it("should handle prop", () => {
      const { result } = parse(`<div test="test" />`);
      expect(result).toMatchInlineSnapshot(`"<div test={"test"} />"`);
    });

    it("prop true", () => {
      const { result } = parse(`<div test />`);
      expect(result).toMatchInlineSnapshot(`"<div test />"`);
    });

    it("prop to camelCase", () => {
      const { result } = parse(`<div test-camel-case="test-again" />`);
      expect(result).toMatchInlineSnapshot(`"<div test-camel-case={"test-again"} />"`);
    });
  });
  describe("v-bind", () => {
    it("should handle prop with v-bind", () => {
      const { result } = parse(`<div v-bind:test="test" />`);
      expect(result).toMatchInlineSnapshot(`"<div test={test} />"`);
    });

    it("should handle prop with v-bind shorthand", () => {
      const { result } = parse(`<div :test="test" />`);
      expect(result).toMatchInlineSnapshot(`"<div test={test} />"`);
    });

    it("should handle prop with v-bind shorthand sugar", () => {
      const { result } = parse(`<div :test />`);
      expect(result).toMatchInlineSnapshot(`"<div test={___VERTER___ctx.test} />"`);
    });

    it("should camelise prop with v-bind shorthand sugar", () => {
      const { result } = parse(`<div :test-to-foo />`);
      expect(result).toMatchInlineSnapshot(`"<div test-to-foo={___VERTER___ctx.testToFoo} />"`);
    });

    it("camelCase", () => {
      const { result } = parse(`<div :test-camel-case="'test-camel'" />`);
      expect(result).toMatchInlineSnapshot(`"<div test-camel-case={'test-camel'} />"`);
    });

    it(':[msg]="msg"', () => {
      const { result } = parse(`<div :[msg]="msg" />`);
      expect(result).toMatchInlineSnapshot(`"<div {...{[msg]:msg}} />"`);
    });

    it('v-bind:[msg]="msg"', () => {
      const { result } = parse(`<div v-bind:[msg]="msg" />`);
      expect(result).toMatchInlineSnapshot(`"<div {...{[msg]:msg}} />"`);
    });

    it("supports xlink attributes in svg namespace", () => {
      const { result } = parse(`<svg><a :xlink:special="foo"></a></svg>`);
      expect(result).toMatchInlineSnapshot(`"<svg><a xlink:special={foo}></a></svg>"`);
    });

    it("keeps .prop modifier bound properties", () => {
      const { result } = parse(`<div :someProperty.prop="someObject"></div>`);
      expect(result).toMatchInlineSnapshot(`"<div someProperty.prop={someObject}></div>"`);
    });

    it("supports dot shorthand for prop modifier", () => {
      const { result } = parse(`<div .someProperty="someObject"></div>`);
      expect(result).toMatchInlineSnapshot(`"<div someProperty={someObject}></div>"`);
    });
  });

  describe("events", () => {
    it("should handle prop with v-on", () => {
      const { result } = parse(`<div v-on:test="test" />`);
      expect(result).toMatchInlineSnapshot(`"<div onTest={test} />"`);
    });
    it("should handle prop with @", () => {
      const { result } = parse(`<div @test="test" />`);
      expect(result).toMatchInlineSnapshot(`"<div onTest={test} />"`);
    });

    it("camelise", () => {
      const { result } = parse(`<div @test-camel-case="test-test" />`);
      expect(result).toMatchInlineSnapshot(`"<div onTest-camel-case={test-test} />"`);
    });
  });

  describe("camilise", () => {
    it("should camelise", () => {
      const { result } = parse(`<div test-camel-case="test" />`);
      expect(result).toMatchInlineSnapshot(`"<div test-camel-case={"test"} />"`);
    });

    it("should camelise with v-bind", () => {
      const { result } = parse(`<div v-bind:test-camel-case="test" />`);
      expect(result).toMatchInlineSnapshot(`"<div test-camel-case={test} />"`);
    });

    it("should camelise with v-on", () => {
      const { result } = parse(`<div v-on:test-camel-case="test" />`);
      expect(result).toMatchInlineSnapshot(`"<div onTest-camel-case={test} />"`);
    });

    it("should camelise with @", () => {
      const { result } = parse(`<div @test-camel-case="test" />`);
      expect(result).toMatchInlineSnapshot(`"<div onTest-camel-case={test} />"`);
    });

    it("should camelise with :", () => {
      const { result } = parse(`<div :test-camel-case="test" />`);
      expect(result).toMatchInlineSnapshot(`"<div test-camel-case={test} />"`);
    });

    it("not aria-*", () => {
      const { result } = parse(`<div aria-label="test" />`);
      expect(result).toMatchInlineSnapshot(`"<div aria-label={"test"} />"`);
    });
    it("not data-*", () => {
      const { result } = parse(`<div data-test="test" />`);
      expect(result).toMatchInlineSnapshot(`"<div data-test={"test"} />"`);
    });

    it("not custom", () => {
      const { result } = parse(`<div test-test="test" />`, {
        camelWhitelistAttributes(name) {
          return name.startsWith("test-");
        },
      });
      expect(result).toMatchInlineSnapshot(`"<div test-test={"test"} />"`);
    });
  });

  describe("merge", () => {
    it("style", () => {
      const { result } = parse(`<div :style="{ color: 'red' }" style="color: blue" />`);
      expect(result).toContain(
        `<div style={___VERTER___normalizeStyle([{ color: 'red' },"color: blue"])}  />`,
      );
    });
    it("class", () => {
      const { result } = parse(`<div :class="{ color: 'red' }" class="color" />`);
      expect(result).toContain(
        `<div class={___VERTER___normalizeClass([{ color: 'red' },"color"])}  />`,
      );
    });

    it("style and class", () => {
      const { result } = parse(
        `<div :style="{ color: 'red' }" style="color: blue" :class="{ color: 'red' }" class="color" />`,
      );
      expect(result).toContain(
        `<div style={___VERTER___normalizeStyle([{ color: 'red' },"color: blue"])}  class={___VERTER___normalizeClass([{ color: 'red' },"color"])}  />`,
      );
    });

    it("with other props", () => {
      const { result } = parse(
        `<div v-if="test" :style="{ color: 'red' }" :foo style="color: blue" :class="{ color: 'red' }" @test="onTest" class="color" :test="test" />`,
      );
      expect(result).toContain(
        `style={___VERTER___normalizeStyle([{ color: 'red' },"color: blue"])}`,
      );
      expect(result).toContain(`class={___VERTER___normalizeClass([{ color: 'red' },"color"])}`);
    });
  });
});
