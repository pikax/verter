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
});
