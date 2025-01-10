import { MagicString } from "@vue/compiler-sfc";
import { ParsedBlockTemplate } from "../../../../parser/types.js";
import { DefaultPlugins, processTemplate, TemplateContext } from "../../index.js";
import { LoopPlugin  } from "./index.js";
import { parser } from "../../../../parser/parser.js";

describe('process tempalte plugins loop', () => {
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
      }
    );

    return r;
  }


    it('<div v-for="item in items"/>', () => {
        const { result } = parse(`<div v-for="item in items"/>`);
    })

    it('<div v-for="item in items">{{ item }}</div>', () => {
        const { result } = parse(`<div v-for="item in items">{{ item }}</div>`);
    })

    it('<div v-for="item in items">{{ item + 1 }}</div>', () => {
        const { result } = parse(`<div v-for="item in items">{{ item + 1 }}</div>`);
    }) 

    it('<div v-for="item of items">{{ item + 1 }}</div>', () => {
        const { result } = parse(`<div v-for="item of items">{{ item + 1 }}</div>`);
    })

    it('<div v-for="(item, index) in items">{{ item + index }}</div>', () => {
        const { result } = parse(`<div v-for="(item, index) in items">{{ item + index }}</div>`);
    })

    it('<div v-for="(item, index) of items">{{ item + index }}</div>', () => {
        const { result } = parse(`<div v-for="(item, index) of items">{{ item + index }}</div>`);
    })

    it('<div v-for="{test} in items">{{ test }}</div>', () => {
        const { result } = parse(`<div v-for="{test} in items">{{ test }}</div>`);
    })

    it('<div v-for="{test} of items">{{ test }}</div>', () => {
        const { result } = parse(`<div v-for="{test} of items">{{ test }}</div>`);
    })

    it('<div v-for="(item, key, index) of items">{{ item + key + index }}</div>', () => {
        const { result } = parse(`<div v-for="(item, key, index) of items">{{ item + key + index }}</div>`);
    })
    it('<div v-for="({item}, key, index) in items">{{ item + key + index }}</div>', () => {
        const { result } = parse(`<div v-for="({item}, key, index) in items">{{ item + key + index }}</div>`);
    })

    it('nested', ()=> {
        const {result} = parse(`<li v-for="item in items">
  <span v-for="childItem in item.children">
    {{ item.message }} {{ childItem }}
  </span>
</li>`);
    })

    it('with v-if', ()=> {
        // note item should be from binding, because v-if has priority
        const {result} = parse(`<li v-for="item in items" v-if="item.active"></li>`);

    })

    it('template', ()=> {
        const {result} = parse(`<template v-for="item in items">
    <li>{{ item.msg }}</li>
    <li class="divider" role="presentation"></li>
</template>`)
    })

    it('parent v-if', ()=> {
        const {result} = parse(`<div v-if="show"><div v-for="item in items">{{ item }}</div></div>`)
    })

    it('v-for with slots dynamic', ()=> {
        const {result} = parse(`<Comp v-for="item in items" #[item]></Comp>`) 
    })

    it('v-for with slots children', ()=> {
        const {result} = parse(`<div v-for="item in items">
    <slot :name="item">
        <div>{{ item }}</div>
    </slot>
</div>`) 
    })

})