import { DefaultPlugins, processTemplate, TemplateContext } from "./index";
import { MagicString, parse as parseSFC } from "@vue/compiler-sfc";

import { parser } from "../../parser";
import { ParsedBlockTemplate } from "../../parser/types";

describe("process template", () => {
  function parse(content: string, options: Partial<TemplateContext> = {}) {
    const source = `<template>${content}</template>`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const templateBlock = parsed.blocks.find(
      (x) => x.type === "template"
    ) as ParsedBlockTemplate;

    const r = processTemplate(templateBlock.result.items, [...DefaultPlugins], {
      ...options,
      s,
      filename: "test.vue",
      blocks: parsed.blocks,
      block: templateBlock,
      blockNameResolver: (name) => name,
    });

    return r;
  }

  describe("props", () => {
    it("class with bindings", () => {
      const { result } = parse(
        `<div :style="{ color: 'red' }" class="color: green" :class="{ color: 'red', test, super: foo }" />`
      );
      expect(result).toMatchInlineSnapshot(
        `"function template(){<div style={___VERTER___normalizeStyle([{ color: 'red' }])}  class={___VERTER___normalizeClass([{ color: 'red', test: ___VERTER___ctx.test, super: ___VERTER___ctx.foo },"color: green"])} />}"`
      );
    });
  });

  describe("conditionals", () => {
    it("v-if", () => {
      const { result } = parse(
        `<div v-if="typeof test === 'string'" :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(
        `"function template(){{()=>{if(typeof ___VERTER___ctx.test === 'string'){<div  test={()=>___VERTER___ctx.test} />}}}}"`
      );
    });

    it("v-if > v-if & expect error", () => {
      const { result } = parse(
        `<div v-if="test === 'app'"> 
          <!-- @ts-expect-error no overlap -->
          <div v-if="test === 'foo'"> Error </div>
        </div>`
      );
      expect(result).toMatchInlineSnapshot(`
        "function template(){{()=>{if(___VERTER___ctx.test === 'app'){<div > 
                   {()=>{if(!((___VERTER___ctx.test === 'app'))) return;/* @ts-expect-error no overlap */
                 if(___VERTER___ctx.test === 'foo'){<div > {"Error"} </div>}}}
                </div>}}}}"
      `);
    });
  });

  describe("slot", () => {
    it.skip("slot with v-if", () => {
      const { result } = parse(`<slot v-if="false"/>`);
      expect(result).toMatchInlineSnapshot(`
      "{ (): any => {if(false){const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
      return <RENDER_SLOT />}}}"
    `);
    });

    it.skip("slot with parent v-if", () => {
      const { result } = parse(`<div v-if="false"> <slot /> </div>`);
      expect(result).toMatchInlineSnapshot(`
      "{ (): any => {if(false){<div > {()=>{
      if(!(false)) { return; } 
      const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
      return <RENDER_SLOT />}} </div>}}}"
    `);
    });

    it.skip("slot with parent v-else", () => {
      const { result } = parse(
        `<div v-if="false"> <slot /> </div><div v-else> <slot/> </div>`
      );
      expect(result).toMatchInlineSnapshot(`
      "{ (): any => {if(false){<div > {()=>{
      if(!(false)) { return; } 
      const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
      return <RENDER_SLOT />}} </div>}else{
      <div > {()=>{
      if(!((!false))) { return; } 
      const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
      return <RENDER_SLOT/>}} </div>
      }}}"
    `);
    });
    it.skip("slot with parent v-else-if", () => {
      const { result } = parse(
        `<div v-if="disableDrag"> <slot /> </div><div v-else-if="!disableDrag"> <slot/> </div><div v-else> <slot/> </div>`
      );
      expect(result).toMatchInlineSnapshot(`
      "{ (): any => {if(___VERTER___ctx.disableDrag){<div > {()=>{
      if(!(___VERTER___ctx.disableDrag)) { return; } 
      const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
      return <RENDER_SLOT />}} </div>}else if(!___VERTER___ctx.disableDrag){<div > {()=>{
      if(!((!___VERTER___ctx.disableDrag))) { return; } 
      const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
      return <RENDER_SLOT/>}} </div>}else{
      <div > {()=>{
      if(!((!(!___VERTER___ctx.disableDrag)))) { return; } 
      const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
      return <RENDER_SLOT/>}} </div>
      }}}"
    `);
    });

    it.skip("slot with v-for", () => {
      const { result } = parse(`<slot v-for="name in $slots" :name="name"/>`);
      expect(result).toMatchInlineSnapshot(`
      "{___VERTER___renderList(___VERTER___ctx.$slots,name   =>{ const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots[name]);
      return <RENDER_SLOT  />})}"
    `);
    });
  });
});
