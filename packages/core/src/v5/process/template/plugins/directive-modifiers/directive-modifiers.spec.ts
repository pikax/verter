/**
 * @ai-generated - This test file was generated with AI assistance.
 * Validates directive and event modifier map collection for future implementation.
 */
import { MagicString } from "@vue/compiler-sfc";
import { DefaultPlugins } from "../..";
import { DirectiveModifiersPlugin } from "./directive-modifiers";
import { parser } from "../../../../parser";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { processTemplate, TemplateContext } from "../../template";

type DirectiveModifierMapEntry = {
  name: string;
  type: "event" | "custom";
  modifiers: unknown[];
  node: unknown;
  parent: unknown;
};

describe("process template plugins directive modifiers", () => {
  function parse(content: string, options: Partial<TemplateContext> = {}) {
    const source = `<template>${content}</template>`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const templateBlock = parsed.blocks.find(
      (x) => x.type === "template"
    ) as ParsedBlockTemplate;

    const r = processTemplate(templateBlock.result.items, [...DefaultPlugins.filter(x => x.name !== "VerterContext")], {
      ...options,
      s,
      filename: "test.vue",
      blocks: parsed.blocks,
      block: templateBlock,
      blockNameResolver: (name) => name,
    });

    return r;
  }

  it("captures event modifiers", () => {
    const { context } = parse(`<button @click.stop.prevent="onClick" />`);
    expect(DirectiveModifiersPlugin.directives instanceof Map).toBe(true);
    expect(DirectiveModifiersPlugin.directives.size).toBe(1);

    const entry = DirectiveModifiersPlugin.directives.get("@click") as
      | DirectiveModifierMapEntry
      | undefined;

    expect(entry?.type).toBe("event");
    expect(entry?.modifiers.length).toBe(2);
  });

  it("captures custom directive modifiers", () => {
    const { context } = parse(`<div v-focus.lazy.once="value" />`);
    expect(DirectiveModifiersPlugin.directives instanceof Map).toBe(true);
    expect(DirectiveModifiersPlugin.directives.size).toBe(1);

    const entry = DirectiveModifiersPlugin.directives.get("v-focus") as
      | DirectiveModifierMapEntry
      | undefined;

    expect(entry?.type).toBe("custom");
    expect(entry?.modifiers.length).toBe(2);
  });

  it("captures multiple entries per template", () => {
    parse(`<form v-on:submit.prevent="onSubmit" v-focus.once />`);

    expect(DirectiveModifiersPlugin.directives.size).toBe(2);
    const entries = Array.from(DirectiveModifiersPlugin.directives.values());

    const event = entries.find((e) => e.type === "event");
    const custom = entries.find((e) => e.type === "custom");

    expect((event?.modifiers ?? []).length).toBeGreaterThan(0);
    expect((custom?.modifiers ?? []).length).toBeGreaterThan(0);
  });

  it("resets the map between parses", () => {
    parse(`<button @click.stop="onClick" />`);
    expect(DirectiveModifiersPlugin.directives.size).toBe(1);

    parse(`<div v-focus.lazy />`);
    expect(DirectiveModifiersPlugin.directives.size).toBe(1);

    const entry = DirectiveModifiersPlugin.directives.values().next().value as
      | DirectiveModifierMapEntry
      | undefined;
    expect(entry?.name).toContain("v-focus");
  });

  it("ignores directives without modifiers", () => {
    parse(`<button @click="onClick" v-focus />`);
    expect(DirectiveModifiersPlugin.directives.size).toBe(0);
  });
});
