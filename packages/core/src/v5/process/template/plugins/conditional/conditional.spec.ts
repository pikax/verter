import { parser } from "../../../../parser";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { processTemplate, TemplateContext } from "../../template";
import { MagicString } from "@vue/compiler-sfc";
import { ConditionalPlugin } from "./index";
import { BlockPlugin } from "../block";

describe("process template plugins conditional", () => {
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
        ConditionalPlugin,
        BlockPlugin,
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
        narrow: true,
        block: templateBlock,
        blockNameResolver: (name) => name,
      }
    );

    return r;
  }

  it("v-if", () => {
    const { result } = parse(`<div v-if="typeof test === 'string'" />`);
    expect(result).toMatchInlineSnapshot(
      `"{()=>{if(typeof test === 'string'){<div  />}}}"`
    );
  });

  it("v-if v-else-if", () => {
    const { result } = parse(
      `<div v-if="typeof test === 'string'" />
        <div v-else-if="typeof test === number" />`
    );
    expect(result).toMatchInlineSnapshot(`
      "{()=>{if(typeof test === 'string'){<div  />}
              else if(typeof test === number){<div  />}}}"
    `);
  });

  it("v-if v-else", () => {
    const { result } = parse(
      `<div v-if="typeof test === 'string'" />
        <div v-else />`
    );
    expect(result).toMatchInlineSnapshot(`
      "{()=>{if(typeof test === 'string'){<div  />}
              else{<div  />}}}"
    `);
  });

  describe("narrow", () => {
    it("v-if narrow prop", () => {
      const { result } = parse(
        `<div v-if="typeof test === 'string'" :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(
        `"{()=>{if(typeof test === 'string'){<div  :test="()=>!((typeof test === 'string'))? undefined :test" />}}}"`
      );
    });

    it("v-else-if", () => {
      const { result } = parse(
        `<div v-if="typeof test === 'object'"></div><div v-else-if="typeof test === 'string'" :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(
        `"{()=>{if(typeof test === 'object'){<div ></div>}else if(typeof test === 'string'){<div  :test="()=>!(!((typeof test === 'object')) && (typeof test === 'string'))? undefined :test" />}}}"`
      );
    });

    it("v-else", () => {
      const { result } = parse(
        `<div v-if="typeof test === 'object'"></div><div v-else :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(
        `"{()=>{if(typeof test === 'object'){<div ></div>}else{<div  :test="()=>!(!((typeof test === 'object')))? undefined :test" />}}}"`
      );
    });

    it("v-if v-else-if v-else", () => {
      const { result } = parse(
        `<div v-if="typeof test === 'string'" :test="()=>test" />

        <div v-else-if="typeof test === 'number'" :test="()=>test" />

        <div v-else :test="()=>test" />`
      );
      expect(result).toMatchInlineSnapshot(`
        "{()=>{if(typeof test === 'string'){<div  :test="()=>!((typeof test === 'string'))? undefined :test" />}

                else if(typeof test === 'number'){<div  :test="()=>!(!((typeof test === 'string')) && (typeof test === 'number'))? undefined :test" />}

                else{<div  :test="()=>!(!((typeof test === 'string')) && !((typeof test === 'number')))? undefined :test" />}}}"
      `);
    });
  });

  it("deep", () => {
    const { result } = parse(
      `<div v-if="typeof test === 'string'">
          <div v-if="test === 'app'" :test="()=> test">app</div>
        </div>
        <div v-else-if="typeof test === 'number'"/>
        <div v-else :test="()=>test" />`
    );
    expect(result).toMatchInlineSnapshot(`
      "{()=>{if(typeof test === 'string'){<div >
                {()=>{if(!((typeof test === 'string'))) return;if(test === 'app'){<div  :test="()=> !((typeof test === 'string') && (test === 'app'))? undefined :test">app</div>}}}
              </div>}
              else if(typeof test === 'number'){<div />}
              else{<div  :test="()=>!(!((typeof test === 'string')) && !((typeof test === 'number')))? undefined :test" />}}}"
    `);
  });
});
