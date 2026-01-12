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
  it("emits runCustomDirective with value, arg and modifiers", () => {
    const { result } = parse(`<div v-test:foo.bar="baz" />`);

    expect(result).toContain(
      `v-directive={(___VERTER___slotInstance)=>{declare const ___VERTER___directiveElement: ___VERTER___ExtractLeafElement<typeof ___VERTER___slotInstance>;___VERTER___runCustomDirective(___VERTER___directiveElement, ___VERTER___directiveAccessor.vTest)(___VERTER___directiveElement, ___VERTER___ctx.baz, "foo", { bar: true });}}`
    );
  });

  it("handles directives without arg and modifiers", () => {
    const { result } = parse(`<div v-focus="val" />`);

    expect(result).toContain(
      `___VERTER___runCustomDirective(___VERTER___directiveElement, ___VERTER___directiveAccessor.vFocus)(___VERTER___directiveElement, ___VERTER___ctx.val, undefined, {});`
    );
  });

  it("supports multiple directives on the same element", () => {
    const { result } = parse(`<div v-foo="a" v-bar:arg="b" />`);

    expect(result).toContain(
      `___VERTER___runCustomDirective(___VERTER___directiveElement, ___VERTER___directiveAccessor.vFoo)(___VERTER___directiveElement, ___VERTER___ctx.a, undefined, {});`
    );
    expect(result).toContain(
      `___VERTER___runCustomDirective(___VERTER___directiveElement, ___VERTER___directiveAccessor.vBar)(___VERTER___directiveElement, ___VERTER___ctx.b, "arg", {});`
    );
  });

  // @ai-generated - Tests fallback value/arg/modifier defaults
  it("defaults directive value to true and arg/modifiers to undefined/{}", () => {
    const { result } = parse(`<div v-flag />`);

    expect(result).toContain(
      `___VERTER___runCustomDirective(___VERTER___directiveElement, ___VERTER___directiveAccessor.vFlag)(___VERTER___directiveElement, true, undefined, {});`
    );
  });

  // @ai-generated - Tests dynamic args and multiple modifiers
  it("passes dynamic argument expression and modifier map", () => {
    const { result } = parse(`<div v-pin:[side].round.exact="distance" />`);

    expect(result).toMatch(
      /___VERTER___runCustomDirective\(___VERTER___directiveElement, ___VERTER___directiveAccessor\.vPin\)\(___VERTER___directiveElement, ___VERTER___ctx\.distance, *\[?___VERTER___ctx\.side\]?, *\{ round: true, exact: true \}\);/
    );
  });

  // @ai-generated - Tests directive call order preservation
  it("preserves directive call order per element", () => {
    const { result } = parse(`<div v-first="a" v-second="b" v-third="c" />`);

    const first = result.indexOf("directiveAccessor.vFirst");
    const second = result.indexOf("directiveAccessor.vSecond");
    const third = result.indexOf("directiveAccessor.vThird");

    expect(first).toBeGreaterThanOrEqual(0);
    expect(second).toBeGreaterThan(first);
    expect(third).toBeGreaterThan(second);
  });

  // @ai-generated - Tests directives alongside DOM event handlers
  it("coexists with event listeners on elements", () => {
    const { result } = parse(`<button v-foo="handler" @click="onClick" />`);

    expect(result).toContain(
      `___VERTER___runCustomDirective(___VERTER___directiveElement, ___VERTER___directiveAccessor.vFoo)(___VERTER___directiveElement, ___VERTER___ctx.handler, undefined, {});`
    );
    expect(result).toContain(`onClick={___VERTER___ctx.onClick}`);
  });

  // @ai-generated - Tests directives inside slot templates with v-slot usage
  it("applies directives within slotted content", () => {
    const { result } = parse(
      `<MyList><template #default><button v-bar="baz">{{ baz }}</button></template></MyList>`
    );

    expect(result).toContain(
      `___VERTER___runCustomDirective(___VERTER___directiveElement, ___VERTER___directiveAccessor.vBar)(___VERTER___directiveElement, ___VERTER___ctx.baz, undefined, {});`
    );
    expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.default)`);
  });

  // @ai-generated - Tests directives on components using inline v-slot syntax
  it("runs directives on components with v-slot shorthand", () => {
    const { result } = parse(
      `<MyList v-foo.bar:arg="value" v-slot="{ value }"><span>{{ value }}</span></MyList>`
    );

    expect(result).toContain(
      `___VERTER___runCustomDirective(___VERTER___directiveElement, ___VERTER___directiveAccessor.vFoo)(___VERTER___directiveElement, ___VERTER___ctx.value, undefined, { "bar:arg": true });`
    );
    expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
    expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.default)`);
  });

  // @ai-generated - Tests directives with named template slot content
  it("runs directives on components with named template slots", () => {
    const { result } = parse(
      `<MyList v-foo.bar:arg="value"><template #test><span>{{ value }}</span></template></MyList>`
    );

    expect(result).toContain(
      `___VERTER___runCustomDirective(___VERTER___directiveElement, ___VERTER___directiveAccessor.vFoo)(___VERTER___directiveElement, ___VERTER___ctx.value, undefined, { "bar:arg": true });`
    );
    expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.test)`);
  });
});
