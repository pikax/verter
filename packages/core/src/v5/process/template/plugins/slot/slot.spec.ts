import { parser } from "../../../../parser";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { processTemplate, TemplateContext } from "../../template";
import { BindingPlugin } from "../binding";
import { MagicString, parse as parseSFC } from "@vue/compiler-sfc";
import { SlotPlugin } from "./index";
import { PropPlugin } from "../prop";
import { DefaultPlugins } from "../..";

describe("process template plugins slot", () => {
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
        ...DefaultPlugins.filter((x) => x.name !== "VerterContext"),
        // clean template tag
        // {
        //   post: (s) => {
        //     s.update(0, "<template>".length, "");
        //     s.update(source.length - "</template>".length, source.length, "");
        //   },
        // },
      ],
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

  describe("declaration", () => {
    it("<slot/>", () => {
      const { result } = parse(`<slot/>`);
      expect(result).toMatch(
        /const ___VERTER___slotComponent\d+=___VERTER___\$slot\.default;<___VERTER___slotComponent\d+\/>/
      );
    });
    it('<slot name="test" />', () => {
      const { result } = parse(`<slot name="test"/>`);
      expect(result).toMatch(
        /const ___VERTER___slotComponent\d+=___VERTER___\$slot\["test"\];<___VERTER___slotComponent\d+\s\/>/
      );
    });

    it(`<slot :name="test"/>`, () => {
      const { result } = parse(`<slot :name="test"/>`);
      expect(result).toMatch(
        /const ___VERTER___slotComponent\d+=___VERTER___\$slot\[___VERTER___ctx\.test\];<___VERTER___slotComponent\d+\s\/>/
      );
    });

    it(`<slot :[msg]="test"/>`, () => {
      const { result } = parse(`<slot :[msg]="test"/>`);

      expect(result).toMatch(
        /const ___VERTER___slotComponent\d+=___VERTER___\$slot\.default;<___VERTER___slotComponent\d+ \{\.\.\.\{\[___VERTER___ctx\.msg\]:___VERTER___ctx\.test\}\}\/\>/
      );
    });

    it("with props", () => {
      const { result } = parse(
        `<slot :[msg]="test" :name="name" v-bind:onTest="()=> callMe()" @bind="bind"/>`
      );
      expect(result).toMatch(
        /const ___VERTER___slotComponent\d+=___VERTER___\$slot\[___VERTER___ctx\.name\];<___VERTER___slotComponent\d+ \{\.\.\.\{\[___VERTER___ctx\.msg\]:___VERTER___ctx\.test\}\}  onTest=\{\(\)=> ___VERTER___ctx\.callMe\(\)\}\s+onBind=\{___VERTER___ctx\.bind\}\/>/
      );
    });

    it("<slot v-if=\"test === 'app'\"/>", () => {
      const { result } = parse(`<slot v-if="test === 'app'"/>`);

      expect(result).toMatch(
        /if\(___VERTER___ctx\.test === 'app'\)\{const ___VERTER___slotComponent\d+=___VERTER___\$slot\.default;<___VERTER___slotComponent\d+ \/>\}/
      );
    });

    it("with parent v-if", () => {
      const { result } = parse(`<div v-if="test === 'app'"> <slot /> </div>`);

      expect(result).toMatch(
        /<div > \{\(\)=>\{const ___VERTER___slotComponent\d+=___VERTER___\$slot\.default;<___VERTER___slotComponent\d+ \/>\}\} <\/div>/
      );
    });

    it("<slot> <span>1</span> </slot>", () => {
      const { result } = parse(`<slot> <span>1</span> </slot>`);

      expect(result).toMatch(
        /const ___VERTER___slotComponent\d+=___VERTER___\$slot\.default;<___VERTER___slotComponent\d+> <span>1<\/span> <\/___VERTER___slotComponent\d+>/
      );
    });

    it('<slot v-for="name in names" :name="name"/>', () => {
      const { result } = parse(`<slot v-for="name in names" :name="name"/>`);

      expect(result).toMatch(
        /___VERTER___renderList\(___VERTER___ctx\.names,\(name\)=>\{\s+const ___VERTER___slotComponent\d+=___VERTER___\$slot\[name\];<___VERTER___slotComponent\d+\s*\/\>\}\)/
      );
    });

    it('multiple slots <slot name="foo"/><slot name="bar"/>', () => {
      const { result } = parse(`<slot name="foo"/><slot name="bar"/>`);
      
      // Extract the offset numbers from both slot variables
      const slotComponentPattern = /___VERTER___slotComponent(\d+)/;
      const matches = result.match(new RegExp(slotComponentPattern.source, 'g'));
      expect(matches).toHaveLength(4); // 2 slots × 2 occurrences (declaration + usage)
      
      const offsets = Array.from(new Set(matches.map(m => {
        const match = m.match(slotComponentPattern);
        if (!match) throw new Error(`Unexpected match format: ${m}`);
        return match[1];
      })));
      expect(offsets).toHaveLength(2); // Ensure two different offset values
    });
  });

  describe("render", () => {
    describe("element", () => {
      it("template v-slot:foo", () => {
        const { result } = parse(
          `<div><template v-slot:foo><span>1</span></template></div>`
        );

        expect(result).toContain(
          "<div v-slot={(___VERTER___slotInstance)=>{"
        );
        expect(result).toContain(
          "___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.foo)"
        );
      });

      it(`template v-if="test === 'app'" v-slot:foo`, () => {
        const { result } = parse(
          `<div><template v-if="test==='app'" v-slot:foo><span>1{{test}}</span></template></div>`
        );

        expect(result).toContain(
          "<div v-slot={(___VERTER___slotInstance)=>{"
        );
        expect(result).toContain(
          "if(___VERTER___ctx.test==='app'){ ___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.foo)"
        );
      });

      it("conditional", () => {
        const { result } = parse(
          `<div>
            <template v-if="foo === 'test'" v-slot:foo><span>1</span></template>
            <template v-else-if="foo === 'app'" v-slot:app><span>1</span></template>
            <template v-else-if="foo === 'baz'" v-slot:baz><span>1</span></template>
            <template v-else v-slot:bar><span>1</span></template>
          </div>`
        );

        expect(result).toContain(`<div v-slot={(___VERTER___slotInstance)=>{`);
        expect(result).toContain(`if(___VERTER___ctx.foo === 'test'){ ___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.foo)`);
        expect(result).toContain(`else if(___VERTER___ctx.foo === 'app'){ ___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.app)`);
        expect(result).toContain(`else if(___VERTER___ctx.foo === 'baz'){ ___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.baz)`);
        expect(result).toContain(`else{ ___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.bar)`);
      });

      it("template #foo", () => {
        const { result } = parse(
          `<div><template #foo><span>1</span></template></div>`
        );

        expect(result).toContain(
          "<div v-slot={(___VERTER___slotInstance)=>{"
        );
        expect(result).toContain(
          "___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.foo)"
        );
      });

      it("template #foo-bar", () => {
        const { result } = parse(
          `<div><template #foo-bar><span>1</span></template></div>`
        );

        expect(result).toContain(
          `<div v-slot={(___VERTER___slotInstance)=>{`
        );
        expect(result).toContain(
          `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots['foo-bar'])`
        );
      });

      it('template v-slot:foo="obj"', () => {
        const { result } = parse(
          `<div><template v-slot:foo="obj"><span>1 {{ obj.test }}</span></template></div>`
        );

        expect(result).toContain(
          `<div v-slot={(___VERTER___slotInstance)=>{`
        );
        expect(result).toContain(
          `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.foo)((obj)=>`
        );
      });
      it('template #foo="obj"', () => {
        const { result } = parse(
          `<div><template #foo="obj"><span>1 {{ obj.test }}</span></template></div>`
        );

        expect(result).toContain(
          `<div v-slot={(___VERTER___slotInstance)=>{`
        );
        expect(result).toContain(
          `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.foo)((obj)=>`
        );
      });

      it('template v-slot:foo="{bar}"', () => {
        const { result } = parse(
          `<div><template v-slot:foo="{bar}"><span>1{{bar}}</span></template></div>`
        );

        expect(result).toContain(
          `<div v-slot={(___VERTER___slotInstance)=>{`
        );
        expect(result).toContain(
          `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.foo)(({bar})=>`
        );
      });
      it('template #foo="{bar}"', () => {
        const { result } = parse(
          `<div><template #foo="{bar}"><span>1{{bar}}</span></template></div>`
        );

        expect(result).toContain(
          `<div v-slot={(___VERTER___slotInstance)=>{`
        );
        expect(result).toContain(
          `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.foo)(({bar})=>`
        );
      });

      it("template v-slot:[foo]", () => {
        const { result } = parse(
          `<div><template v-slot:[foo]><span>1</span></template></div>`
        );
        expect(result).toContain(
          `<div v-slot={(___VERTER___slotInstance)=>{`
        );
        expect(result).toContain(
          `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots[___VERTER___ctx.foo])`
        );
      });
      it("template #[foo]", () => {
        const { result } = parse(
          `<div><template #[foo]><span>1</span></template></div>`
        );
        expect(result).toContain(
          `<div v-slot={(___VERTER___slotInstance)=>{`
        );
        expect(result).toContain(
          `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots[___VERTER___ctx.foo])`
        );
      });

      it('template v-slot:[foo]="{obj}"', () => {
        const { result } = parse(
          `<div><template v-slot:[foo]="{obj}"><span>1{{obj}}</span></template></div>`
        );
        expect(result).toContain(
          `<div v-slot={(___VERTER___slotInstance)=>{`
        );
        expect(result).toContain(
          `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots[___VERTER___ctx.foo])(({obj})=>`
        );
      });
      it('template #[foo]="{obj}"', () => {
        const { result } = parse(
          `<div><template #[foo]="{obj}"><span>1{{obj}}</span></template></div>`
        );
        expect(result).toContain(
          `<div v-slot={(___VERTER___slotInstance)=>{`
        );
        expect(result).toContain(
          `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots[___VERTER___ctx.foo])(({obj})=>`
        );
      });

      it("parent conditional", () => {
        const { result } = parse(
          `<div v-if="test === 'foo'"><template #[test]>{{test}}</template></div>`
        );

        expect(result).toContain(
          `<div v-slot={(___VERTER___slotInstance)=>{`
        );
        expect(result).toContain(
          `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots[___VERTER___ctx.test])`
        );
      });

      it(`keep comments correct @ts-expect-error`, () => {
        const { result } = parse(
          `<div v-if="test === 'foo'">
          // @ts-expect-error invalid app
          <template v-if="test==='app'" v-slot:foo><span>1{{test}}</span></template></div>`
        );
        expect(result).toContain(`// @ts-expect-error invalid app`);
        expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.foo)`);
      });

      it("short no children", () => {
        const { result } = parse(`<Comp>
          <template #test></template>
</Comp>`);
        expect(result).toContain(
          `<___VERTER___components.Comp`
        );
        expect(result).toContain(
          `v-slot={(___VERTER___slotInstance)=>{`
        );
        expect(result).toContain(
          `___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.test)`
        );
      });
    });

    describe("v-slot", () => {
      describe("default", () => {
        it("no children", () => {
          const { result } = parse(`<Comp v-slot></Comp>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.default)`);
        });

        it("self-closing", () => {
          const { result } = parse(`<Comp v-slot/>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.default)`);
        });

        it("with children", () => {
          const { result } = parse(`<Comp v-slot>{{foo}}</Comp>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.default)`);
          expect(result).toContain(`{___VERTER___ctx.foo}`);
        });

        it('Comp v-slot="{foo}"', () => {
          const { result } = parse(`<Comp v-slot="{foo}">{{foo}}</Comp>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.default)(({foo})=>`);
        });
        it('#="{foo}"', () => {
          const { result } = parse(`<Comp #="{foo}">{{foo}}</Comp>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.default)(({foo})=>`);
        });

        it('v-if="test === \'app\'" v-slot="{foo}"', () => {
          const { result } = parse(
            `<Comp v-if="test === 'app'" v-slot="{foo}">{{foo}}</Comp>`
          );
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.default)(({foo})=>`);
        });
      });

      describe("named", () => {
        it("no children", () => {
          const { result } = parse(`<Comp v-slot:test></Comp>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.test)`);
        });

        it("self-closing", () => {
          const { result } = parse(`<Comp v-slot:test/>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.test)`);
        });

        it("with children", () => {
          const { result } = parse(`<Comp v-slot:test>{{foo}}</Comp>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.test)`);
          expect(result).toContain(`{___VERTER___ctx.foo}`);
        });

        it('Comp v-slot:test="{foo}"', () => {
          const { result } = parse(`<Comp v-slot:test="{foo}">{{foo}}</Comp>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.test)(({foo})=>`);
        });
        it('#test="{foo}"', () => {
          const { result } = parse(`<Comp #test="{foo}">{{foo}}</Comp>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.test)(({foo})=>`);
        });

        it('v-if="test === \'app\'" v-slot:test="{foo}"', () => {
          const { result } = parse(
            `<Comp v-if="test === 'app'" v-slot:test="{foo}">{{foo}}</Comp>`
          );
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.test)(({foo})=>`);
        });
      });

      describe("dynamic", () => {
        it("no children", () => {
          const { result } = parse(`<Comp v-slot:[test]></Comp>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots[___VERTER___ctx.test])`);
        });

        it("self-closing", () => {
          const { result } = parse(`<Comp v-slot:[test]/>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots[___VERTER___ctx.test])`);
        });

        it("with children", () => {
          const { result } = parse(`<Comp v-slot:[test]>{{foo}}</Comp>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots[___VERTER___ctx.test])`);
          expect(result).toContain(`{___VERTER___ctx.foo}`);
        });

        it('Comp v-slot:[test]="{foo}"', () => {
          const { result } = parse(
            `<Comp v-slot:[test]="{foo}">{{foo}}</Comp>`
          );
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots[___VERTER___ctx.test])(({foo})=>`);
        });
        it('#[test]="{foo}"', () => {
          const { result } = parse(`<Comp #[test]="{foo}">{{foo}}</Comp>`);
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots[___VERTER___ctx.test])(({foo})=>`);
        });

        it('v-if="test === \'app\'" v-slot:[test]="{foo}"', () => {
          const { result } = parse(
            `<Comp v-if="test === 'app'" v-slot:[test]="{foo}">{{foo}}</Comp>`
          );
          expect(result).toContain(`<___VERTER___components.Comp`);
          expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
          expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots[___VERTER___ctx.test])(({foo})=>`);
        });
      });

      it('Comp v-if="test === \'app\'" v-slot:[test]="{foo}"', () => {
        const { result } = parse(
          `<Comp v-if="test === 'app'" v-slot:[test]="{foo}">{{foo}}</Comp>`
        );
        expect(result).toContain(`<___VERTER___components.Comp`);
        expect(result).toContain(`v-slot={(___VERTER___slotInstance)=>{`);
        expect(result).toContain(`___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots[___VERTER___ctx.test])(({foo})=>`);
      });
    });
  });

  describe("edge cases", () => {
    it("renders slot content with nested conditionals", () => {
      const { result } = parse(`<div> 
      <template #default="{ value, type }">
        <div v-if="value">
          <a :href="\`tel:\${value}\`"> {{ value }}</a> <span> ({{ type }})</span>
        </div>
        <span v-else>—</span>
      </template></div>`);
      expect(result).toContain("renderSlotJSX(___VERTER___slotInstance.$slots.default)");
      expect(result).toContain("tel:${value}");
      expect(result).toContain("<span> ({ type })</span>");
    });
  });

  /*
  describe("slot", () => {


      it("with parent v-if", () => {
        const { result } = transpile(`<div v-if="false"> <slot /> </div>`);
        expect(result).toMatchInlineSnapshot(`
          "{ (): any => {if(false){<div > {()=>{
          if(!(false)) { return; } 
          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
          return <RENDER_SLOT />}} </div>}}}"
        `);
      });

      it("with parent v-else", () => {
        const { result } = transpile(
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
      it("with parent v-else-if", () => {
        const { result } = transpile(
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

      it("with v-for", () => {
        const { result } = transpile(
          `<slot v-for="name in $slots" :name="name"/>`
        );
        expect(result).toMatchInlineSnapshot(`
          "{___VERTER___renderList(___VERTER___ctx.$slots,name   =>{ const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots[name]);
          return <RENDER_SLOT  />})}"
        `);
      });

      it("element + slot", () => {
        const { result } = transpile(`<my-comp><slot/></my-comp>`);
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComp v-slot={(___VERTER___slotInstance): any=>{
          const $slots = ___VERTER___slotInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{ <>

          {()=>{

          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
          return <RENDER_SLOT/>}}
          </>})}

          }}></___VERTER___comp.MyComp>"
        `);
      });
    });
    describe("with children", () => {
      it("parse slot", () => {
        const { result } = transpile(`<slot>{{ 'hello' }}</slot>`);
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
          return <RENDER_SLOT>{{ 'hello' }}</RENDER_SLOT>}}"
        `);
      });

      it("parse slot with name", () => {
        const { result } = transpile(`<slot name="test">{{ 'hello' }}</slot>`);
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots["test"]);
          return <RENDER_SLOT >{{ 'hello' }}</RENDER_SLOT>}}"
        `);
      });

      it("parse slot with name expression", () => {
        const { result } = transpile(`<slot :name="test">{{ 'hello' }}</slot>`);
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots[___VERTER___ctx.test]);
          return <RENDER_SLOT >{{ 'hello' }}</RENDER_SLOT>}}"
        `);
      });

      it("with v-bind should be default", () => {
        const { result } = transpile(
          `<slot :[msg]="test">{{ 'hello' }}</slot>`
        );
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
          return <RENDER_SLOT [___VERTER___ctx.msg]={___VERTER___ctx.test}>{{ 'hello' }}</RENDER_SLOT>}}"
        `);
      });

      it("with v-if", () => {
        const { result } = transpile(`<slot v-if="false">{{ 'hello' }}</slot>`);
        expect(result).toMatchInlineSnapshot(`
          "{ (): any => {if(false){const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
          return <RENDER_SLOT >{{ 'hello' }}</RENDER_SLOT>}}}"
        `);
      });

      it("with v-for", () => {
        const { result } = transpile(
          `<slot v-for="name in $slots" :name="name">{{ 'hello' }}</slot>`
        );
        expect(result).toMatchInlineSnapshot(`
          "{___VERTER___renderList(___VERTER___ctx.$slots,name   =>{ const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots[name]);
          return <RENDER_SLOT  >{{ 'hello' }}</RENDER_SLOT>})}"
        `);
      });

      it("nested + v-if", () => {
        const { result } = transpile(`<slot v-if="disableDrag" :name="selected">
  <slot :tab="item" />
</slot>`);
        expect(result).toMatchInlineSnapshot(`
          "{ (): any => {if(___VERTER___ctx.disableDrag){const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots[___VERTER___ctx.selected]);
          return <RENDER_SLOT  >
            {()=>{
          if(!(___VERTER___ctx.disableDrag)) { return; } 
          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
          return <RENDER_SLOT tab={___VERTER___ctx.item} />}}
          </RENDER_SLOT>}}}"
        `);
      });

      it("nested + v-if + typescript", () => {
        const { result } =
          transpile(`<slot v-if="disableDrag" :name="selected as T">
  <slot :tab="item" />
</slot>`);
        expect(result).toMatchInlineSnapshot(`
          "{ (): any => {if(___VERTER___ctx.disableDrag){const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots[___VERTER___ctx.selected as T]);
          return <RENDER_SLOT  >
            {()=>{
          if(!(___VERTER___ctx.disableDrag)) { return; } 
          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
          return <RENDER_SLOT tab={___VERTER___ctx.item} />}}
          </RENDER_SLOT>}}}"
        `);
      });

      it("nested + default", () => {
        const { result } = transpile(
          '<slot :name="`page-${p?.name || p}`" :page="p">\n' +
            '<slot v-bind="p" />\n' +
            "</slot>"
        );
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots[\`page-\${___VERTER___ctx.p?.name || ___VERTER___ctx.p}\`]);
          return <RENDER_SLOT  page={___VERTER___ctx.p}>
          {()=>{

          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
          return <RENDER_SLOT {...___VERTER___ctx.p} />}}
          </RENDER_SLOT>}}"
        `);
      });

      it("element + nested + default", () => {
        const { result } = transpile(
          '<my-comp><slot :name="`page-${p?.name || p}`" :page="p">\n' +
            '<slot v-bind="p" />\n' +
            "</slot></my-comp>"
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComp v-slot={(___VERTER___slotInstance): any=>{
          const $slots = ___VERTER___slotInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{ <>

          {()=>{

          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots[\`page-\${___VERTER___ctx.p?.name || ___VERTER___ctx.p}\`]);
          return <RENDER_SLOT  page={___VERTER___ctx.p}>
          {()=>{

          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
          return <RENDER_SLOT {...___VERTER___ctx.p} />}}
          </RENDER_SLOT>}}
          </>})}

          }}></___VERTER___comp.MyComp>"
        `);
      });

      it("named with a v-if child", () => {
        const { result } = transpile(`<MyComp>
      <template #foo>
         <div></div>
        <div v-if="foo.n">
          {{foo.n}}
        </div>
        <Comp></Comp>
      </template>
      </MyComp>`);
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComp v-slot={(___VERTER___slotInstance): any=>{
          const $slots = ___VERTER___slotInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.foo)(()=>{<>

          <___VERTER___template >
                   <div></div>
                  { (): any => {if(___VERTER___ctx.foo.n){<div >
                    {{foo.n}}
                  </div>}}}
                  <___VERTER___comp.Comp></___VERTER___comp.Comp>
                </___VERTER___template>
          </>})}

          }}>
                
                </___VERTER___comp.MyComp>"
        `);
      });

      it("complex slots usage", () => {
        const { result } = transpile(`<div>
  <Foo>
    <slot name="bar">
      <div>
          <div v-if="true"> bar </div>
          <slot v-else name="title">
            <div> baz title </div>
          </slot>
      </div>
    </slot>
  </Foo>
  <div>
    <slot />
  </div>
  <template v-if="noFoo" />
  <div v-else>
   Foo
  </div>
</div>
`);
        expect(result).toMatchInlineSnapshot(`
          "<div>
            <___VERTER___comp.Foo v-slot={(___VERTER___slotInstance): any=>{
          const $slots = ___VERTER___slotInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{ <>

          {()=>{

          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots["bar"]);
          return <RENDER_SLOT >
                <div>
                    { (): any => {if(true){<div > bar </div>}
                    else{
          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots["title"]);
          return <RENDER_SLOT  >
                      <div> baz title </div>
                    </RENDER_SLOT>
          }}}
                </div>
              </RENDER_SLOT>}}
          </>})}

          }}>
              
            </___VERTER___comp.Foo>
            <div>
              {()=>{

          const RENDER_SLOT = ___VERTER___SLOT_TO_COMPONENT(___VERTER___ctx.$slots.default);
          return <RENDER_SLOT />}}
            </div>
            { (): any => {if(___VERTER___ctx.noFoo){<___VERTER___template  />}
            else{
          <div >
             Foo
            </div>
          }}}
          </div>
          "
        `);
      });
    });
  });
  */
});
