/**
 * @ai-generated - Validates v-directive runner output for custom directives.
 * - Ensures value/arg/modifiers are forwarded into runCustomDirective.
 * - Handles elements with and without directive args.
 * - Supports multiple directives on the same element.
 */
import { MagicString } from "@vue/compiler-sfc";
import { DefaultPlugins } from "../..";
import { parser } from "../../../../parser";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { processTemplate, TemplateContext } from "../../template";
import { ProcessItemType } from "../../../types";
import { expect } from "vitest";

const normalize = (value: string) => value.replace(/\s+/g, "");

function parse(content: string, options: Partial<TemplateContext> = {}) {
  const source = `<template>${content}</template>`;
  const parsed = parser(source);
  const s = new MagicString(source);
  const templateBlock = parsed.blocks.find(
    (x) => x.type === "template"
  ) as ParsedBlockTemplate;

  return processTemplate(
    templateBlock.result.items,
    [...DefaultPlugins.filter((x) => x.name !== "VerterContext")],
    {
      ...options,
      s,
      filename: "test.vue",
      blocks: parsed.blocks,
      block: templateBlock,
      blockNameResolver: (name) => name,
    }
  );
}

describe("directive runner", () => {
  const getWarnings = (ctx: TemplateContext) =>
    ctx.items.filter((item) => item.type === ProcessItemType.Warning);

  const warningMessages = (ctx: TemplateContext) =>
    getWarnings(ctx).map((w) => w.message);

  describe("custom directives", () => {
    it("handles partial v- ", () => {
      const { result } = parse(`<div v- />`);
      const normalized = normalize(result);
      expect(normalized).toContain(
        normalize(
          'v-directive={(___VERTER___slotInstance)=>{const ___VERTER___directiveElement={} as ___VERTER___ExtractLeafElement<typeof ___VERTER___slotInstance>;___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor["v"])(___VERTER___directiveElement,true,undefined,{});}}'
        )
      );
    });

    it("handles dot only", () => {
      const { result } = parse(`<div v-foo. />`);
      const normalized = normalize(result);
      expect(normalized).toContain(
        normalize(
          'v-directive={(___VERTER___slotInstance)=>{const ___VERTER___directiveElement={} as ___VERTER___ExtractLeafElement<typeof ___VERTER___slotInstance>;___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor["vFoo"])(___VERTER___directiveElement,true,undefined,{"":true});}}'
        )
      );
    });

    it("handles directives without arg and modifiers", () => {
      const { result } = parse(`<div v-focus />`);
      const normalized = normalize(result);

      expect(normalized).toContain(
        normalize(
          `___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor["vFocus"])(___VERTER___directiveElement,true,undefined,{});`
        )
      );
    });

    it("emits runCustomDirective with value, arg and modifiers", () => {
      const { result } = parse(`<div v-test:foo.bar="baz" />`);
      const normalized = normalize(result);

      expect(normalized).toContain(
        normalize(
          `v-directive={(___VERTER___slotInstance)=>{const ___VERTER___directiveElement={} as ___VERTER___ExtractLeafElement<typeof ___VERTER___slotInstance>;___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor["vTest"])(___VERTER___directiveElement,___VERTER___ctx.baz,"foo",{"bar":true});}}`
        )
      );
    });

    it("handles directives without arg and modifiers", () => {
      const { result } = parse(`<div v-focus="val" />`);
      const normalized = normalize(result);

      expect(normalized).toContain(
        normalize(
          `___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor["vFocus"])(___VERTER___directiveElement,___VERTER___ctx.val,undefined,{});`
        )
      );
    });

    it("supports multiple directives on the same element", () => {
      const { result } = parse(`<div v-foo="a" v-bar:arg="b" />`);
      const normalized = normalize(result);

      expect(normalized).toContain(
        normalize(
          `___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor["vFoo"])(___VERTER___directiveElement,___VERTER___ctx.a,undefined,{});`
        )
      );
      expect(normalized).toContain(
        normalize(
          `___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor["vBar"])(___VERTER___directiveElement,___VERTER___ctx.b,"arg",{});`
        )
      );
    });

    // @ai-generated - Tests fallback value/arg/modifier defaults
    it("defaults directive value to true and arg/modifiers to undefined/{}", () => {
      const { result } = parse(`<div v-flag />`);
      const normalized = normalize(result);

      expect(normalized).toContain(
        normalize(
          `___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor["vFlag"])(___VERTER___directiveElement,true,undefined,{});`
        )
      );
    });

    // @ai-generated - Tests dynamic args and multiple modifiers
    it("passes dynamic argument expression and modifier map", () => {
      const { result } = parse(`<div v-pin:[side].round.exact="distance" />`);

      expect(normalize(result)).toContain(
        normalize(
          `___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor["vPin"])(___VERTER___directiveElement,___VERTER___ctx.distance,[___VERTER___ctx.side],{"round":true,"exact":true});`
        )
      );
    });

    // @ai-generated - Tests directive call order preservation
    it("preserves directive call order per element", () => {
      const { result } = parse(`<div v-first="a" v-second="b" v-third="c" />`);
      const normalized = normalize(result);

      const first = normalized.indexOf('directiveAccessor["vFirst"]');
      const second = normalized.indexOf('directiveAccessor["vSecond"]');
      const third = normalized.indexOf('directiveAccessor["vThird"]');

      expect(first).toBeGreaterThanOrEqual(0);
      expect(second).toBeGreaterThan(first);
      expect(third).toBeGreaterThan(second);
    });

    // @ai-generated - Tests directives alongside DOM event handlers
    it("coexists with event listeners on elements", () => {
      const { result } = parse(`<button v-foo="handler" @click="onClick" />`);
      const normalized = normalize(result);

      expect(normalized).toContain(
        normalize(
          `___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor["vFoo"])(___VERTER___directiveElement,___VERTER___ctx.handler,undefined,{});`
        )
      );
      expect(result).toContain(`onClick={___VERTER___ctx.onClick}`);
    });

    // @ai-generated - Tests directives inside slot templates with v-slot usage
    it("applies directives within slotted content", () => {
      const { result } = parse(
        `<MyList><template #default><button v-bar="baz">{{ baz }}</button></template></MyList>`
      );
      const normalized = normalize(result);

      expect(normalized).toContain(
        normalize(
          `___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor["vBar"])(___VERTER___directiveElement,___VERTER___ctx.baz,undefined,{});`
        )
      );
      expect(result).toContain(
        `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.default)`
      );
    });

    // @ai-generated - Tests directives on components using inline v-slot syntax
    it("runs directives on components with v-slot shorthand", () => {
      const { result } = parse(
        `<MyList v-foo.bar:arg="value" v-slot="{ value }"><span>{{ value }}</span></MyList>`
      );
      const normalized = normalize(result);

      expect(normalized).toContain(
        normalize(
          `___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor["vFoo"])(___VERTER___directiveElement,___VERTER___ctx.value,undefined,{"bar:arg":true});`
        )
      );
      expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
      expect(result).toContain(
        `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.default)`
      );
    });

    // @ai-generated - Tests directives with named template slot content
    it("runs directives on components with named template slots", () => {
      const { result } = parse(
        `<MyList v-foo.bar:arg="value"><template #test><span>{{ value }}</span></template></MyList>`
      );
      const normalized = normalize(result);

      expect(normalized).toContain(
        normalize(
          `___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor["vFoo"])(___VERTER___directiveElement,___VERTER___ctx.value,undefined,{"bar:arg":true});`
        )
      );
      expect(result).toContain(
        `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.test)`
      );
    });

    it("camel-cases hyphenated directive names for accessor lookup", () => {
      const { result } = parse(`<div v-click-outside="handler" />`);

      expect(result).toContain(
        '___VERTER___directiveAccessor["vClickOutside"]'
      );
    });

    it("does not emit warnings for custom directives", () => {
      const { context } = parse(`<input v-foo:bar.mod="baz" />`);

      expect(warningMessages(context)).toEqual([]);
    });
  });

  describe("built-in directives", () => {
    it("warns when modifiers are used on directives without modifier support", () => {
      const { context } = parse(`<div v-text.foo="bar" />`);

      const warnings = getWarnings(context);
      expect(
        warnings.some(
          (w) => w.message === "UNSUPPORTED_BUILTIN_DIRECTIVE_MODIFIER"
        )
      ).toBe(true);
    });

    it("warns when arguments are used on directives that forbid them", () => {
      const { context } = parse(`<div v-show:arg="visible" />`);

      const warnings = getWarnings(context);
      expect(
        warnings.some(
          (w) => w.message === "UNSUPPORTED_BUILTIN_DIRECTIVE_ARGUMENT"
        )
      ).toBe(true);
    });

    it("warns when values are provided to value-less directives", () => {
      const { context } = parse(`<div v-cloak="foo" />`);

      const warnings = getWarnings(context);
      expect(
        warnings.some(
          (w) => w.message === "UNSUPPORTED_BUILTIN_DIRECTIVE_VALUE"
        )
      ).toBe(true);
    });

    it("allows modifiers on directives that support them (v-model)", () => {
      const { context } = parse(`<input v-model.number.trim="val" />`);

      const warnings = getWarnings(context);
      expect(
        warnings.some((w) =>
          w.message?.toString().startsWith("UNSUPPORTED_BUILTIN_DIRECTIVE")
        )
      ).toBe(false);
    });

    it("emits v-model modifier guards", () => {
      const { result } = parse(`<input v-model.number.trim="count" />`);

      expect(normalize(result)).toContain(
        normalize(
          `({"number":true,"trim":true} satisfies ___VERTER___vModelModifiers<typeof ___VERTER___slotInstance,'value'>)`
        )
      );
    });
    it("emits v-model modifier guards", () => {
      const { result } = parse(`<Comp v-model.number.trim="count" />`);

      expect(normalize(result)).toContain(
        normalize(
          `({"number":true,"trim":true} satisfies ___VERTER___vModelModifiers<typeof ___VERTER___slotInstance,'modelValue'>)`
        )
      );
    });

    it("handles v-bind modifiers on props without warnings", () => {
      const { context } = parse(`<div v-bind:title.camel="label" />`);

      const warnings = getWarnings(context);
      expect(warnings.length).toBe(0);
    });

    // @ai-generated - Tests event option modifiers on native elements (issue #78)
    it("allows v-on event option modifiers on native elements", () => {
      const { context, result } = parse(
        `<button @click.capture.passive.once="handler" />`
      );

      expect(warningMessages(context)).toEqual([]);
      expect(result).not.toContain("runCustomDirective");
    });

    it("skips built-in directives in the custom directive runner output", () => {
      const { result, context } = parse(`<div v-show="visible" />`);

      expect(result).not.toContain("runCustomDirective(");
      expect(result).not.toContain("directiveAccessor");
      expect(getWarnings(context).length).toBe(0);
    });

    it("emits only the expected warnings for multiple violations", () => {
      const { context } = parse(`<div v-show:arg.mod="visible" />`);

      const messages = warningMessages(context);
      expect(messages).toContain("UNSUPPORTED_BUILTIN_DIRECTIVE_ARGUMENT");
      expect(messages).toContain("UNSUPPORTED_BUILTIN_DIRECTIVE_MODIFIER");
      expect(messages.length).toBe(2);
    });

    it("does not warn for built-ins that allow modifiers (v-on)", () => {
      const { context, result } = parse(
        `<button v-on:click.stop="handler" v-foo="bar" />`
      );

      expect(warningMessages(context)).toEqual([]);
      // custom directive should still be emitted
      expect(result).toContain('___VERTER___directiveAccessor["vFoo"]');
      // built-in v-on should not be turned into runCustomDirective
      expect(result).not.toContain("vOn:click");
    });

    it("keeps custom directives working when a built-in on the same element warns", () => {
      const { result, context } = parse(`<div v-text.foo="bar" v-foo="baz" />`);

      expect(result).toContain('___VERTER___directiveAccessor["vFoo"]');
      expect(warningMessages(context)).toContain(
        "UNSUPPORTED_BUILTIN_DIRECTIVE_MODIFIER"
      );
    });

    it("injects modifier validators for built-in directives", () => {
      const { result } = parse(`<button v-on:click.once="handler" />`);

      const normalized = normalize(result);
      expect(normalized).toContain(
        normalize(
          "({\"once\":true}satisfies ___VERTER___vOnModifiers<typeof ___VERTER___slotInstance,'onClick'>)"
        )
      );
      expect(normalized).not.toContain("runCustomDirective(");
      expect(normalized).not.toContain("directiveAccessor");
    });

    it("keeps .prop modifier bound properties", () => {
      const { result } = parse(`<div :someProperty.prop="someObject"></div>`);
      const normalized = normalize(result);
      expect(normalized).toContain(
        normalize(
          "({\"prop\":true}satisfies ___VERTER___vBindModifiers<typeof ___VERTER___slotInstance,'someProperty'>)"
        )
      );
    });

    it("supports dot shorthand for prop modifier", () => {
      const { result } = parse(`<div .someProperty="someObject"></div>`);
      const normalized = normalize(result);
      expect(normalized).toContain(
        normalize(
          "({\"prop\":true}satisfies ___VERTER___vBindModifiers<typeof ___VERTER___slotInstance,'someProperty'>)"
        )
      );
    });
  });
});
