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

  it("Comp", () => {
    const { result } = parse(`<Comp></Comp>`);

    expect(result).toMatchInlineSnapshot(
      `
      "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
      export function template(){const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};
      <><___VERTER___COMPONENT_CTX.Comp></___VERTER___COMPONENT_CTX.Comp>
      </>}"
    `
    );
  });

  it("Comp v-if", () => {
    const { result } = parse(`<Comp v-if="test"></Comp>`);

    expect(result).toMatchInlineSnapshot(
      `
      "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
      export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};if(___VERTER___ctx.test){
      <><___VERTER___COMPONENT_CTX.Comp ></___VERTER___COMPONENT_CTX.Comp>}}}
      </>}"
    `
    );
  });

  it("Comp v-for", () => {
    const { result } = parse(`<Comp v-for="item in items"></Comp>`);

    expect(result).toMatchInlineSnapshot(
      `
      "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
      export function template(){const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  
      <><___VERTER___COMPONENT_CTX.Comp ></___VERTER___COMPONENT_CTX.Comp>})}}
      </>}"
    `
    );
  });

  it('Comp v-for="item in items" v-if="test"', () => {
    const { result } = parse(`<Comp v-for="item in items" v-if="test"></Comp>`);

    expect(result).toMatchInlineSnapshot(
      `
      "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
      export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};if(___VERTER___ctx.test){{()=>{if(!((___VERTER___ctx.test))) return;___VERTER___renderList(___VERTER___ctx.items,(item)=>{  if(!((___VERTER___ctx.test))) return;
      <><___VERTER___COMPONENT_CTX.Comp  ></___VERTER___COMPONENT_CTX.Comp>})}}}}}
      </>}"
    `
    );
  });

  it("Component.Comp", () => {
    const { result } = parse(`<Component.Comp></Component.Comp>`);

    expect(result).toMatchInlineSnapshot(
      `
      "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
      export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;___VERTER___COMPONENT_CTX.Component.Comp;
      <><Componentl__verter__lComp></Componentl__verter__lComp>}}
      </>}"
    `
    );
  });

  it("v-if Component.Comp", () => {
    const { result } = parse(`<Component.Comp v-if="test"></Component.Comp>`);

    expect(result).toMatchInlineSnapshot(
      `
      "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
      export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};if(___VERTER___ctx.test){const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;___VERTER___COMPONENT_CTX.Component.Comp;
      <><Componentl__verter__lComp ></Componentl__verter__lComp>}}}
      </>}"
    `
    );
  });

  it("Component.Comp v-for", () => {
    const { result } = parse(
      `<Component.Comp v-for="item in items"></Component.Comp>`
    );

    expect(result).toMatchInlineSnapshot(
      `
      "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
      export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;___VERTER___COMPONENT_CTX.Component.Comp;
      <><Componentl__verter__lComp ></Componentl__verter__lComp>})}}}}
      </>}"
    `
    );
  });

  it('Component.Comp v-for="item in items" v-if="test"', () => {
    const { result } = parse(
      `<Component.Comp v-for="item in items" v-if="test"></Component.Comp>`
    );

    expect(result).toMatchInlineSnapshot(
      `
      "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
      export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};if(___VERTER___ctx.test){{()=>{if(!((___VERTER___ctx.test))) return;___VERTER___renderList(___VERTER___ctx.items,(item)=>{  if(!((___VERTER___ctx.test))) return;const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;___VERTER___COMPONENT_CTX.Component.Comp;
      <><Componentl__verter__lComp  ></Componentl__verter__lComp>})}}}}}
      </>}"
    `
    );
  });

  it("Component.Comp/>", () => {
    const { result } = parse(`<Component.Comp/>`);

    expect(result).toMatchInlineSnapshot(
      `
      "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
      export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;
      <><Componentl__verter__lComp/>}}
      </>}"
    `
    );
  });

  it("v-if Component.Comp/>", () => {
    const { result } = parse(`<Component.Comp v-if="test"/>`);

    expect(result).toMatchInlineSnapshot(
      `
      "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
      export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};if(___VERTER___ctx.test){const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;
      <><Componentl__verter__lComp />}}}
      </>}"
    `
    );
  });

  it("Component.Comp v-for/>", () => {
    const { result } = parse(`<Component.Comp v-for="item in items"/>`);

    expect(result).toMatchInlineSnapshot(
      `
      "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
      export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};{()=>{___VERTER___renderList(___VERTER___ctx.items,(item)=>{  const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;
      <><Componentl__verter__lComp />})}}}}
      </>}"
    `
    );
  });

  it('Component.Comp v-for="item in items" v-if="test"/>', () => {
    const { result } = parse(
      `<Component.Comp v-for="item in items" v-if="test"/>`
    );

    expect(result).toMatchInlineSnapshot(
      `
      "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
      export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};if(___VERTER___ctx.test){{()=>{if(!((___VERTER___ctx.test))) return;___VERTER___renderList(___VERTER___ctx.items,(item)=>{  if(!((___VERTER___ctx.test))) return;const Componentl__verter__lComp=___VERTER___COMPONENT_CTX.Component.Comp;
      <><Componentl__verter__lComp  />})}}}}}
      </>}"
    `
    );
  });

  describe("component", () => {
    test("is", () => {
      const { result } = parse(`<component is="div"></component>`);

      expect(result).toContain("<div ></div>");
    });
    test('is="div" tabindex="1"', () => {
      const { result } = parse(`<component is="div" tabindex="1"></component>`);

      expect(result).toContain('<div  tabindex={"1"}></div>');
    });

    test.only(":is", () => {
      const { result } = parse(`<component :is="'div'"></component>`);

      expect(result).toContain(
        "{ ()=> { const ___VERTER___component = <___VERTER___component></___VERTER___component> }"
      );
    });
  });

  describe("name casing", () => {
    it("item-render", () => {
      const { result } = parse(`<item-render></item-render>`);

      expect(result).toMatchInlineSnapshot(
        `
        "import { ___VERTER___TemplateBinding, ___VERTER___FullContext } from "./options";
        export function template(){{()=>{const ___VERTER___ctx = {...___VERTER___FullContext,...___VERTER___TemplateBinding};const ITEMRENDER=___VERTER___COMPONENT_CTX["item-render"];___VERTER___COMPONENT_CTX.item-render;
        <><ITEMRENDER></ITEMRENDER>}}
        </>}"
      `
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
