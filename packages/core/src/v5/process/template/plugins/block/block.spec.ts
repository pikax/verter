import { parser } from "../../../../parser";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { processTemplate, TemplateContext } from "../../template";
import { MagicString } from "@vue/compiler-sfc";
import { BlockPlugin } from "./index";
import { DefaultPlugins } from "../..";

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
        // BlockPlugin,
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
        // narrow: true,
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
        `"{()=>{if(typeof ___VERTER___ctx.test === ___VERTER___ctx.string){<div  test={()=>___VERTER___ctx.test} />}}}"`
      );
    });

    it("v-if v-else", () => {
      const { result } = parse(
        `<div v-if="typeof test === 'string'" :test="()=>test" />
          <div v-else :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(`
        "{()=>{if(typeof ___VERTER___ctx.test === 'string'){<div  test={()=>___VERTER___ctx.test} />}
                  else{<div  test={()=>___VERTER___ctx.test} />}}}"
      `);
    });

    it("v-if v-else-if v-else", () => {
      const { result } = parse(
        `<div v-if="typeof test === 'string'" :test="()=>test" />
          <div v-else-if="typeof test === 'number'" :test="()=>test" />
          <div v-else :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(`
        "{()=>{if(typeof ___VERTER___ctx.test === 'string'){<div  test={()=>___VERTER___ctx.test} />}
                  else if(typeof ___VERTER___ctx.test === 'number'){<div  test={()=>___VERTER___ctx.test} />}
                  else{<div  test={()=>___VERTER___ctx.test} />}}}"
      `);
    });

    it("v-if v-else-if v-else with if", () => {
      const { result } = parse(
        `<div v-if="typeof test === 'string'" :test="()=>test" />
          <div v-else-if="typeof test === 'number'" :test="()=>test" />
          <div v-else :test="()=>test" />
          <div v-if="typeof test === 'string'" :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(`
        "{()=>{if(typeof ___VERTER___ctx.test === 'string'){<div  test={()=>___VERTER___ctx.test} />}
                  else if(typeof ___VERTER___ctx.test === 'number'){<div  test={()=>___VERTER___ctx.test} />}
                  else{<div  test={()=>___VERTER___ctx.test} />}
                  if(typeof ___VERTER___ctx.test === 'string'){<div  test={()=>___VERTER___ctx.test} />}}}"
      `);
    });
  });

  describe("slot", () => {
    it("v-slot simple", () => {
      const { result } = parse(`<div v-slot><span>test</span></div>`);
      expect(result).toMatchInlineSnapshot(
        `"<div  v-slot={(___VERTER___slotInstance)=>{___VERTER___slotRender(___VERTER___slotInstance.$slots.default)(()=>{<span>{"test"}</span>})}}></div>"`
      );
    });
    it("v-slot", () => {
      const { result } = parse(
        `<div v-slot="item" :test="()=>test"><span>test</span></div>`
      );
      expect(result).toMatchInlineSnapshot(
        `"<div  v-slot={(___VERTER___slotInstance)=>{___VERTER___slotRender(___VERTER___slotInstance.$slots.default)((item)=>{<span>{"test"}</span>})}} test={()=>___VERTER___ctx.test}></div>"`
      );
    });

    it("v-slot + v-if", () => {
      const { result } = parse(
        `<div v-slot="item" v-if="item" :test="()=>test"><span>test</span></div>`
      );
      expect(result).toMatchInlineSnapshot(
        `"{()=>{if(___VERTER___ctx.item){<div  v-slot={(___VERTER___slotInstance)=>{___VERTER___slotRender(___VERTER___slotInstance.$slots.default)((item)=>{if(!((___VERTER___ctx.item))) return;<span>{"test"}</span>})}}  test={()=>___VERTER___ctx.test}></div>}}}"`
      );
    });

    it("<template #slot>", () => {
      const { result } = parse(
        `<div><template #slot><span>test</span></template></div>`
      );
      expect(result).toMatchInlineSnapshot(
        `"<div v-slot={(___VERTER___slotInstance)=>{ ___VERTER___slotRender(___VERTER___slotInstance.$slots.slot)(()=>{<template><span>{"test"}</span></template>});}}></div>"`
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
