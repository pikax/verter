import { parser } from "../../../../parser";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { processTemplate, TemplateContext } from "../../template";
import { MagicString, parse as parseSFC } from "@vue/compiler-sfc";
import { DefaultPlugins } from "../..";

describe("process template plugins element", () => {
  function parse(
    content: string,
    options: Partial<TemplateContext> = {},
    post: string = ""
  ) {
    const source = `<template>${content}</template>${post}`;
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
        block: templateBlock,
      }
    );

    return r;
  }

  it("Comp", () => {
    const { result } = parse(`<Comp></Comp>`);

    expect(result).toMatchInlineSnapshot(
      `"<___VERTER___COMPONENT_CTX.Comp></___VERTER___COMPONENT_CTX.Comp>"`
    );
  });

  it("Comp v-if", () => {
    const { result } = parse(`<Comp v-if="test"></Comp>`);

    expect(result).toMatchInlineSnapshot(
      `"{()=>{if(___VERTER___ctx.test){<___VERTER___COMPONENT_CTX.Comp ></___VERTER___COMPONENT_CTX.Comp>}}}"`
    );
  });

  it("Comp v-for", () => {
    const { result } = parse(`<Comp v-for="item in items"></Comp>`);

    expect(result).toMatchInlineSnapshot(
      `"{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  <___VERTER___COMPONENT_CTX.Comp ></___VERTER___COMPONENT_CTX.Comp>})}}"`
    );
  });

  it('Comp v-for="item in items" v-if="test"', () => {
    const { result } = parse(`<Comp v-for="item in items" v-if="test"></Comp>`);

    expect(result).toMatchInlineSnapshot(
      `"{()=>{if(___VERTER___ctx.test){{()=>{if(!((___VERTER___ctx.test))) return;___VERTER___renderList(___VERTER___ctx.items,(item)=>{  if(!((___VERTER___ctx.test))) return;<___VERTER___COMPONENT_CTX.Comp  ></___VERTER___COMPONENT_CTX.Comp>})}}}}}"`
    );
  });

  it("Component.Comp", () => {
    const { result } = parse(`<Component.Comp></Component.Comp>`);

    expect(result).toMatchInlineSnapshot(
      `"{()=>{const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;___VERTER___COMPONENT_CTX.Component.Comp;<Componentl__verter__lComp></Componentl__verter__lComp>}}"`
    );
  });

  it("v-if Component.Comp", () => {
    const { result } = parse(`<Component.Comp v-if="test"></Component.Comp>`);

    expect(result).toMatchInlineSnapshot(
      `"{()=>{if(___VERTER___ctx.test){const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;___VERTER___COMPONENT_CTX.Component.Comp;<Componentl__verter__lComp ></Componentl__verter__lComp>}}}"`
    );
  });

  it("Component.Comp v-for", () => {
    const { result } = parse(
      `<Component.Comp v-for="item in items"></Component.Comp>`
    );

    expect(result).toMatchInlineSnapshot(
      `"{()=>{{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;___VERTER___COMPONENT_CTX.Component.Comp;<Componentl__verter__lComp ></Componentl__verter__lComp>})}}}}"`
    );
  });

  it('Component.Comp v-for="item in items" v-if="test"', () => {
    const { result } = parse(
      `<Component.Comp v-for="item in items" v-if="test"></Component.Comp>`
    );

    expect(result).toMatchInlineSnapshot(
      `"{()=>{if(___VERTER___ctx.test){{()=>{if(!((___VERTER___ctx.test))) return;___VERTER___renderList(___VERTER___ctx.items,(item)=>{  if(!((___VERTER___ctx.test))) return;const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;___VERTER___COMPONENT_CTX.Component.Comp;<Componentl__verter__lComp  ></Componentl__verter__lComp>})}}}}}"`
    );
  });

  it("Component.Comp/>", () => {
    const { result } = parse(`<Component.Comp/>`);

    expect(result).toMatchInlineSnapshot(
      `"{()=>{const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;<Componentl__verter__lComp/>}}"`
    );
  });

  it("v-if Component.Comp/>", () => {
    const { result } = parse(`<Component.Comp v-if="test"/>`);

    expect(result).toMatchInlineSnapshot(
      `"{()=>{if(___VERTER___ctx.test){const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;<Componentl__verter__lComp />}}}"`
    );
  });

  it("Component.Comp v-for/>", () => {
    const { result } = parse(`<Component.Comp v-for="item in items"/>`);

    expect(result).toMatchInlineSnapshot(
      `"{()=>{{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;<Componentl__verter__lComp />})}}}}"`
    );
  });

  it('Component.Comp v-for="item in items" v-if="test"/>', () => {
    const { result } = parse(
      `<Component.Comp v-for="item in items" v-if="test"/>`
    );

    expect(result).toMatchInlineSnapshot(
      `"{()=>{if(___VERTER___ctx.test){{()=>{if(!((___VERTER___ctx.test))) return;___VERTER___renderList(___VERTER___ctx.items,(item)=>{  if(!((___VERTER___ctx.test))) return;const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;<Componentl__verter__lComp  />})}}}}}"`
    );
  });

  describe("name casing", () => {
    it("item-render", () => {
      const { result } = parse(`<item-render></item-render>`);

      expect(result).toMatchInlineSnapshot(
        `"{()=>{const ITEMRENDER=___VERTER___COMPONENT_CTX["item-render"];___VERTER___COMPONENT_CTX.item-render;<ITEMRENDER></ITEMRENDER>}}"`
      );
    });

    it.skip(";test", () => {
      const { result } = parse(`
  <HelloMoto />
  <hello-moto />

  <helloMoto />
  <Hello-Moto />`);

      expect(result).toMatchInlineSnapshot(`
        "
          <___VERTER___COMPONENT_CTX.HelloMoto />
          {()=>{const HELLOMOTO=___VERTER___COMPONENT_CTX["hello-moto"];<HELLOMOTO />}}

          <___VERTER___COMPONENT_CTX.helloMoto />
          {()=>{const HELLOMOTO=___VERTER___COMPONENT_CTX["Hello-Moto"];<HELLOMOTO />}}"
      `);
    });
  });
});
