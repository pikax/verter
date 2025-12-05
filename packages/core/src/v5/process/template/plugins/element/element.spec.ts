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
      [...DefaultPlugins].filter((x) => x.name !== "VerterContext"),
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

  it("Comp", () => {
    const { result } = parse(`<Comp></Comp>`);

    expect(result).toContain(
      "<___VERTER___components.Comp></___VERTER___components.Comp>"
    );
  });

  it("Comp v-if", () => {
    const { result } = parse(`<Comp v-if="test"></Comp>`);

    expect(result).toContain(
      "if(___VERTER___ctx.test){<___VERTER___components.Comp ></___VERTER___components.Comp>}"
    );
  });

  it("Comp v-for", () => {
    const { result } = parse(`<Comp v-for="item in items"></Comp>`);

    expect(result).toContain(
      "___VERTER___renderList(___VERTER___ctx.items,(item)=>{  <___VERTER___components.Comp ></___VERTER___components.Comp>})"
    );
  });

  it('Comp v-for="item in items" v-if="test"', () => {
    const { result } = parse(`<Comp v-for="item in items" v-if="test"></Comp>`);

    expect(result).toContain(
      "<___VERTER___components.Comp  ></___VERTER___components.Comp>"
    );
  });

  it("Component.Comp", () => {
    const { result, s } = parse(`<Component.Comp></Component.Comp>`);

    expect(result).toContain(
      "const Componentl__verter__lComp=___VERTER___components.Component.Comp;<Componentl__verter__lComp>"
    );
  });

  it("v-if Component.Comp", () => {
    const { result } = parse(`<Component.Comp v-if="test"></Component.Comp>`);

    expect(result).toContain(
      "if(___VERTER___ctx.test){const Componentl__verter__lComp=___VERTER___components.Component.Comp;"
    );
  });

  it("Component.Comp v-for", () => {
    const { result } = parse(
      `<Component.Comp v-for="item in items"></Component.Comp>`
    );

    expect(result).toContain(
      "const Componentl__verter__lComp=___VERTER___components.Component.Comp;"
    );
  });

  it('Component.Comp v-for="item in items" v-if="test"', () => {
    const { result } = parse(
      `<Component.Comp v-for="item in items" v-if="test"></Component.Comp>`
    );

    expect(result).toContain(
      "const Componentl__verter__lComp=___VERTER___components.Component.Comp;"
    );
  });

  it("Component.Comp/>", () => {
    const { result } = parse(`<Component.Comp/>`);

    expect(result).toContain(
      "const Componentl__verter__lComp=___VERTER___components.Component.Comp;<Componentl__verter__lComp/>"
    );
  });

  it("v-if Component.Comp/>", () => {
    const { result } = parse(`<Component.Comp v-if="test"/>`);

    expect(result).toContain(
      "if(___VERTER___ctx.test){const Componentl__verter__lComp=___VERTER___components.Component.Comp;<Componentl__verter__lComp"
    );
  });

  it("Component.Comp v-for/>", () => {
    const { result } = parse(`<Component.Comp v-for="item in items"/>`);

    expect(result).toContain(
      "___VERTER___renderList(___VERTER___ctx.items,(item)=>{  const Componentl__verter__lComp=___VERTER___components.Component.Comp;<Componentl__verter__lComp />"
    );
  });

  it('Component.Comp v-for="item in items" v-if="test"/>', () => {
    const { result } = parse(
      `<Component.Comp v-for="item in items" v-if="test"/>`
    );

    expect(result).toContain(
      "const Componentl__verter__lComp=___VERTER___components.Component.Comp;<Componentl__verter__lComp"
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

    test(":is", () => {
      const { result } = parse(`<component :is="'div'"></component>`);

      expect(result).toContain(
        "<___VERTER___component_render ></___VERTER___component_render>"
      );
    });

    test(`:is="as || 'div'"`, () => {
      const { result } = parse(`<component :is="as || 'div'"></component>`);

      expect(result).toContain(
        "const ___VERTER___component_render=___VERTER___ctx.as || 'div';\n<___VERTER___component_render ></___VERTER___component_render>"
      );
    });
  });

  describe("name casing", () => {
    it("item-render", () => {
      const { result } = parse(`<item-render></item-render>`);

      expect(result).toContain(
        `const ITEMRENDER=___VERTER___components["item-render"];<ITEMRENDER>`
      );
    });

    it("handles mixed-case component names", () => {
      const { result } = parse(`
  <HelloMoto />
  <hello-moto />

  <helloMoto />
  <Hello-Moto />`);

      expect(result).toMatchInlineSnapshot(`
        "export function template(){
        <>
          <___VERTER___components.HelloMoto />
          {()=>{const HELLOMOTO=___VERTER___components[\"hello-moto\"];<HELLOMOTO />}}

          <___VERTER___components.helloMoto />
          {()=>{const HELLOMOTO=___VERTER___components[\"Hello-Moto\"];<HELLOMOTO />}}</>}"
      `);
    });
  });
});
