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
      ],
      {
        ...options,
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        // narrow: true,
        block: templateBlock,
        blockNameResolver: (name) => name,
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
        `
        "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
        export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};if(typeof ___VERTER___ctx.test === ___VERTER___ctx.string){
        <><div  test={()=>___VERTER___ctx.test} />}}}
        </>}"
      `
      );
    });

    it("v-if v-else", () => {
      const { result } = parse(
        `<div v-if="typeof test === 'string'" :test="()=>test" />
          <div v-else :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(`
        "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
        export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};if(typeof ___VERTER___ctx.test === 'string'){
        <><div  test={()=>___VERTER___ctx.test} />}
                  else{<div  test={()=>___VERTER___ctx.test} />}}}
        </>}"
      `);
    });

    it("v-if v-else-if v-else", () => {
      const { result } = parse(
        `<div v-if="typeof test === 'string'" :test="()=>test" />
          <div v-else-if="typeof test === 'number'" :test="()=>test" />
          <div v-else :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(`
        "function template(){{()=>{if(typeof ___VERTER___ctx.test === 'string'){<div  test={()=>___VERTER___ctx.test} />}
                  else if(typeof ___VERTER___ctx.test === 'number'){<div  test={()=>___VERTER___ctx.test} />}
                  else{<div  test={()=>___VERTER___ctx.test} />}}}}"
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
        "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
        export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};if(typeof ___VERTER___ctx.test === 'string'){
        <><div  test={()=>___VERTER___ctx.test} />}
                  else if(typeof ___VERTER___ctx.test === 'number'){<div  test={()=>___VERTER___ctx.test} />}
                  else{<div  test={()=>___VERTER___ctx.test} />}}}
        </>}"
      `);
    });
  });

  describe("slot", () => {
    it("v-slot simple", () => {
      const { result } = parse(`<div v-slot><span>test</span></div>`);
      expect(result).toMatchInlineSnapshot(
        `
        "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
        export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};if(typeof ___VERTER___ctx.test === 'string'){
        <><div  test={()=>___VERTER___ctx.test} />}
                  else if(typeof ___VERTER___ctx.test === 'number'){<div  test={()=>___VERTER___ctx.test} />}
                  else{<div  test={()=>___VERTER___ctx.test} />}
                  if(typeof ___VERTER___ctx.test === 'string'){<div  test={()=>___VERTER___ctx.test} />}}}
        </>}"
      `
      );
    });
    it("v-slot", () => {
      const { result } = parse(
        `<div v-slot="item" :test="()=>test"><span>test</span></div>`
      );
      expect(result).toMatchInlineSnapshot(
        `
        "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
        export function template(){const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};
        <><div  v-slot={(___VERTER___slotInstance)=>{___VERTER___slotRender(___VERTER___slotInstance.$slots.default)(()=>{<span>test</span>})}}></div>
        </>}"
      `
      );
    });

    it("v-slot + v-if", () => {
      const { result } = parse(
        `<div v-slot="item" v-if="item" :test="()=>test"><span>test</span></div>`
      );
      expect(result).toMatchInlineSnapshot(
        `
        "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
        export function template(){const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};
        <><div  v-slot={(___VERTER___slotInstance)=>{___VERTER___slotRender(___VERTER___slotInstance.$slots.default)((item)=>{<span>test</span>})}} test={()=>___VERTER___ctx.test}></div>
        </>}"
      `
      );
    });

    it("<template #slot>", () => {
      const { result } = parse(
        `<div><template #slot><span>test</span></template></div>`
      );
      expect(result).toMatchInlineSnapshot(
        `
        "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
        export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};if(___VERTER___ctx.item){
        <><div  v-slot={(___VERTER___slotInstance)=>{___VERTER___slotRender(___VERTER___slotInstance.$slots.default)((item)=>{if(!((___VERTER___ctx.item))) return;<span>test</span>})}}  test={()=>___VERTER___ctx.test}></div>}}}
        </>}"
      `
      );
    });
  });

  describe("template", () => {
    it("<template></template>", () => {
      const { result } = parse(`<template></template>`);
      expect(result).toMatchInlineSnapshot(`
        "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
        export function template(){const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};
        <><div v-slot={(___VERTER___slotInstance)=>{ ___VERTER___slotRender(___VERTER___slotInstance.$slots.slot)(()=>{<template><span>test</span></template>});}}></div>
        </>}"
      `);
    });
    it("<template/>", () => {
      const { result } = parse(`<template/>`);
      expect(result).toMatchInlineSnapshot(`
        "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
        export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};
        <><template></template>}}
        </>}"
      `);
    });
  });
});
