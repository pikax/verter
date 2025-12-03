import { MagicString } from "@vue/compiler-sfc";
import { ParsedBlockTemplate } from "../../../../parser/types.js";
import {
  DefaultPlugins,
  processTemplate,
  TemplateContext,
} from "../../index.js";
import { LoopPlugin } from "./index.js";
import { parser } from "../../../../parser/parser.js";

describe("process template plugins loop", () => {
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

  it('<div v-for="item in items"/>', () => {
    const { result } = parse(`<div v-for="item in items"/>`);

    expect(result).toContain(
      `{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  <div />})}}`
    );
  });

  it('<div v-for="item in items">{{ item }}</div>', () => {
    const { result } = parse(`<div v-for="item in items">{{ item }}</div>`);

    expect(result).toContain(
      `{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  <div >{ item }</div>})}}`
    );
  });

  it('<div v-for="item in items">{{ item + 1 }}</div>', () => {
    const { result } = parse(`<div v-for="item in items">{{ item + 1 }}</div>`);

    expect(result).toContain(
      `{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  <div >{ item + 1 }</div>})}}`
    );
  });

  it('<div v-for="item of items">{{ item + 1 }}</div>', () => {
    const { result } = parse(`<div v-for="item of items">{{ item + 1 }}</div>`);
    expect(result).toContain(
      `{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  <div >{ item + 1 }</div>})}}`
    );
  });

  it('<div v-for="(item, index) in items">{{ item + index }}</div>', () => {
    const { result } = parse(
      `<div v-for="(item, index) in items">{{ item + index }}</div>`
    );
    expect(result).toContain(
      `{()=>{___VERTER___renderList(___VERTER___ctx.items,(item, index)=>{  <div >{ item + index }</div>})}}`
    );
  });

  it('<div v-for="(item, index) of items">{{ item + index }}</div>', () => {
    const { result } = parse(
      `<div v-for="(item, index) of items">{{ item + index }}</div>`
    );
    expect(result).toContain(
      `{()=>{___VERTER___renderList(___VERTER___ctx.items,(item, index)=>{  <div >{ item + index }</div>})}}`
    );
  });

  it('<div v-for="{obj} in items">{{ test + obj }}</div>', () => {
    const { result } = parse(
      `<div v-for="{obj} in items">{{ test + obj }}</div>`
    );
    expect(result).toContain(
      `{()=>{___VERTER___renderList(___VERTER___ctx.items,({obj})=>{  <div >{ ___VERTER___ctx.test + obj }</div>})}}`
    );
  });

  it('<div v-for="{obj} of items">{{ test + obj }}</div>', () => {
    const { result } = parse(
      `<div v-for="{obj} of items">{{ test + obj}}</div>`
    );
    expect(result).toContain(
      `{()=>{___VERTER___renderList(___VERTER___ctx.items,({obj})=>{  <div >{ ___VERTER___ctx.test + obj}</div>})}}`
    );
  });

  it('<div v-for="(item, key, index) of items">{{ item + key + index }}</div>', () => {
    const { result } = parse(
      `<div v-for="(item, key, index) of items">{{ item + key + index }}</div>`
    );
    expect(result).toContain(
      `{()=>{___VERTER___renderList(___VERTER___ctx.items,(item, key, index)=>{  <div >{ item + key + index }</div>})}}`
    );
  });
  it('<div v-for="({obj}, key, index) in items">{{ item + obj + key + index }}</div>', () => {
    const { result } = parse(
      `<div v-for="({obj}, key, index) in items">{{ item + obj + key + index }}</div>`
    );
    expect(result).toContain(
      `{()=>{___VERTER___renderList(___VERTER___ctx.items,({obj}, key, index)=>{  <div >{ ___VERTER___ctx.item + obj + key + index }</div>})}}`
    );
  });

  it("nested", () => {
    const { result } = parse(`<li v-for="item in items">
  <span v-for="childItem in item.children">
    {{ item.message }} {{ childItem }}
  </span>
</li>`);
    expect(result)
      .toContain(`{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  <li >
  {()=>{___VERTER___renderList(item.children,(childItem)=>{  <span >
    { item.message } { childItem }
  </span>})}}
</li>})}}`);
  });

  it("with v-if", () => {
    // note item should be from binding, because v-if has priority
    const { result } = parse(
      `<li v-for="item in items" v-if="item.active"></li>`
    );

    expect(result).toContain(
      `{()=>{if(___VERTER___ctx.item.active){___VERTER___renderList(___VERTER___ctx.items,(item)=>{  if(!((___VERTER___ctx.item.active))) return;<li  ></li>})}}}`
    );

    expect(result).toContain("export function template(){\n<>");
  });

  it("template", () => {
    const { result } = parse(`<template v-for="item in items">
    <li>{{ item.msg }}</li>
    <li class="divider" role="presentation"></li>
</template>`);

    expect(result)
      .toContain(`{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  <template >
    <li>{ item.msg }</li>
    <li class="divider" role={"presentation"}></li>
</template>})}}`);
    expect(result).toContain("export function template(){\n<>");
  });

  it("parent v-if", () => {
    const { result } = parse(
      `<div v-if="show"><div v-for="item in items">{{ item }}</div></div>`
    );
    expect(result).toContain(
      `{()=>{if(___VERTER___ctx.show){<div >{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  if(!((___VERTER___ctx.show))) return;<div >{ item }</div>})}}</div>}}}`
    );
    expect(result).toContain("export function template(){\n<>");
  });

  it("parent to narrow type", () => {
    const { result } = parse(
      `<div v-if="items === undefined"><div v-for="item in items">{{ item }}</div></div>`
    );
    expect(result).toContain(
      `{()=>{if(___VERTER___ctx.items === undefined){<div >{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  if(!((___VERTER___ctx.items === undefined))) return;<div >{ item }</div>})}}</div>}}}`
    );

    expect(result).toContain("export function template(){\n<>");
  });

  it("v-for with slots dynamic", () => {
    const { result } = parse(`<Comp v-for="item in items" #[item]></Comp>`);
    expect(result).toContain(
      `___VERTER___renderList(___VERTER___ctx.items,(item)=>{`
    );
    expect(result).toContain(
      `<___VERTER___components.Comp`
    );
    expect(result).toContain(
      `v-slot={(___VERTER___slotInstance)=>{___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots[item])`
    );
  });

  it("v-for with slots children", () => {
    const { result } = parse(`<div v-for="item in items">
    <slot :name="item">
        <div>{{ item }}</div>
    </slot>
</div>`);
    expect(result).toContain(`___VERTER___renderList(___VERTER___ctx.items,(item)=>{`);
    expect(result).toMatch(/const ___VERTER___slotComponent\d+=___VERTER___\$slot\[item\]/);
  });
});
