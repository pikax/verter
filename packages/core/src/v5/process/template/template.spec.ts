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

    const r = processTemplate(
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

    return r;
  }

  test("empty", () => {
    const { result } = parse(``);
    expect(result).toContain("export function template()");
    expect(result).toContain("<></>}");
  });

  describe("props", () => {
    it("class with bindings", () => {
      const { result } = parse(
        `<div :style="{ color: 'red' }" class="color: green" :class="{ color: 'red', test, super: foo }" />`
      );
      expect(result).toContain(
        `style={___VERTER___normalizeStyle([{ color: 'red' }])}`
      );
      expect(result).toContain(
        `class={___VERTER___normalizeClass([{ color: 'red', test: ___VERTER___ctx.test, super: ___VERTER___ctx.foo },"color: green"])}`
      );
    });
  });

  describe("conditionals", () => {
    it("v-if", () => {
      const { result } = parse(
        `<div v-if="typeof test === 'string'" :test="()=>test" />`
      );
      expect(result).toContain(
        `if(typeof ___VERTER___ctx.test === 'string'){<div  test={()=>___VERTER___ctx.test} />}`
      );
    });

    it("v-if > v-if & expect error", () => {
      const { result } = parse(
        `<div v-if="test === 'app'"> 
          <!-- @ts-expect-error no overlap -->
          <div v-if="test === 'foo'"> Error </div>
        </div>`
      );
      expect(result).toContain(`if(___VERTER___ctx.test === 'app'){<div >`);
      expect(result).toContain(`/* @ts-expect-error no overlap */`);
      expect(result).toContain(
        `if(___VERTER___ctx.test === 'foo'){<div > Error </div>}`
      );
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

  // GitHub Issues - Full template transformation
  describe("GitHub Issues", () => {
    describe("issue #52 - template context incorrectly applied", () => {
      test("TypeScript 'as' keyword should not be prefixed", () => {
        const { result } = parse(`{{
          () => {
            let a = {} as {
              foo: 1;
            };
            a;
          }
        }}`);
        // 'as' should NOT be prefixed with ___VERTER___ctx
        // object properties in type annotation should NOT be prefixed
        expect(result).not.toContain("___VERTER___ctx.as");
        expect(result).not.toContain("___VERTER___ctx.foo");
      });
      test.only("TypeScript 'as' and then usage keyword should not be prefixed", () => {
        const { result } = parse(`{{
          () => {
            let a = {} as {
              foo: 1;
            };
            a.
          }
        }}`);
        // 'as' should NOT be prefixed with ___VERTER___ctx
        // object properties in type annotation should NOT be prefixed
        expect(result).not.toContain("___VERTER___ctx.as");
        expect(result).not.toContain("___VERTER___ctx.foo");
      });
    });

    describe("issue #47 - arrow function parameters", () => {
      test("arrow function parameters should not be prefixed", () => {
        const { result } = parse(`{{
          (foo:string)=> {
            foo.toLowerCase();
          }
        }}`);
        // 'foo' parameter and its usage should NOT be prefixed
        expect(result).not.toContain("___VERTER___ctx.foo");
      });

      test("arrow function with event parameter", () => {
        const { result } = parse(`{{ (event) => { event.target } }}`);
        // 'event' parameter should not be prefixed
        expect(result).not.toContain("___VERTER___ctx.event");
      });
    });

    describe("issue #49 - event handlers with spread operators", () => {
      test("function with spread should not wrap in event callback", () => {
        const { result } = parse(`<div @click="function (...args) {}" />`);
        // Should be like: onClick={function (...args) {}}
        // NOT: onClick={function (...___VERTER___ctx.args) {}}
        expect(result).not.toContain("___VERTER___ctx.args");
        expect(result).toContain("function (...args)");
      });

      test("arrow function with spread should not wrap in event callback", () => {
        const { result } = parse(`<div @input="(...args) => {}" />`);
        // Should be like: onInput={(...args) => {}}
        // NOT: onInput={(...___VERTER___ctx.args) => {}}
        expect(result).not.toContain("___VERTER___ctx.args");
        expect(result).toContain("(...args) =>");
      });

      test("arrow function with parameter should not wrap in event callback", () => {
        const { result } = parse(`<div @touchmove="(event) => { event; }" />`);
        // Should be like: onTouchmove={(event) => { event; }}
        // NOT: onTouchmove={(...___VERTER___eventArgs)=>___VERTER___eventCallbacks(...)}
        expect(result).not.toContain("___VERTER___eventArgs");
        expect(result).not.toContain("___VERTER___eventCallbacks");
        expect(result).toContain("(event) => { event; }");
      });
    });
  });
});
