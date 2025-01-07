import { parser } from "../../../../parser";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { processTemplate, TemplateContext } from "../../template";
import { MagicString } from "@vue/compiler-sfc";
import { BlockPlugin } from "./index";

describe("process template plugins narrow", () => {
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
        BlockPlugin,
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

  describe("condition", () => {
    it("v-if", () => {
      const { result } = parse(
        `<div v-if="typeof test === string" :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(
        `"{()=>{<div v-if="typeof test === string" :test="()=>test" />}}"`
      );
    });

    it("v-if v-else", () => {
      const { result } = parse(
        `<div v-if="typeof test === string" :test="()=>test" />
          <div v-else :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(
        `
        "{()=>{<div v-if="typeof test === string" :test="()=>test" />
                  <div v-else :test="()=>test" />}}"
      `
      );
    });

    it("v-if v-else-if v-else", () => {
      const { result } = parse(
        `<div v-if="typeof test === string" :test="()=>test" />
          <div v-else-if="typeof test === number" :test="()=>test" />
          <div v-else :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(
        `
        "{()=>{<div v-if="typeof test === string" :test="()=>test" />
                  <div v-else-if="typeof test === number" :test="()=>test" />
                  <div v-else :test="()=>test" />}}"
      `
      );
    });

    it("v-if v-else-if v-else with if", () => {
      const { result } = parse(
        `<div v-if="typeof test === string" :test="()=>test" />
          <div v-else-if="typeof test === number" :test="()=>test" />
          <div v-else :test="()=>test" />
          <div v-if="typeof test === string" :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(
        `
        "{()=>{<div v-if="typeof test === string" :test="()=>test" />
                  <div v-else-if="typeof test === number" :test="()=>test" />
                  <div v-else :test="()=>test" />
                  <div v-if="typeof test === string" :test="()=>test" />}}"
      `
      );
    });
  });

  describe("loop", () => {
    it("for", () => {
      const { result } = parse(
        `<div v-for="item in items" :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(
        `"{()=>{<div v-for="item in items" :test="()=>test" />}}"`
      );
    });

    it("for with v-if", () => {
      const { result } = parse(
        `<div v-for="item in items" v-if="item" :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(
        `"{()=>{<div v-for="item in items" v-if="item" :test="()=>test" />}}"`
      );
    });
  });

  describe("slot", () => {
    it("v-slot simple", () => {
      const { result } = parse(`<div v-slot><span>test</span></div>`);
      expect(result).toMatchInlineSnapshot(
        `"{()=>{<div v-slot><span>test</span></div>}}"`
      );
    });
    it("v-slot", () => {
      const { result } = parse(
        `<div v-slot="item" :test="()=>test"><span>test</span></div>`
      );
      expect(result).toMatchInlineSnapshot(
        `"{()=>{<div v-slot="item" :test="()=>test"><span>test</span></div>}}"`
      );
    });

    it("v-slot + v-if", () => {
      const { result } = parse(
        `<div v-slot="item" v-if="item" :test="()=>test"><span>test</span></div>`
      );
      expect(result).toMatchInlineSnapshot(
        `"{()=>{<div v-slot="item" v-if="item" :test="()=>test"><span>test</span></div>}}"`
      );
    });

    it("<template #slot>", () => {
      const { result } = parse(`<template #slot><span>test</span></template>`);
      expect(result).toMatchInlineSnapshot(
        `"{()=>{<template #slot><span>test</span></template>}}"`
      );
    });
  });

  describe("template", () => {
    it("<template></template>", () => {
      const { result } = parse(`<template></template>`);
      expect(result).toMatchInlineSnapshot(`"{()=>{<template></template>}}"`);
    });
    it("<template/>", () => {
      const { result } = parse(`<template/>`);
      expect(result).toMatchInlineSnapshot(`"{()=>{<template/>}}"`);
    });
  });
});
