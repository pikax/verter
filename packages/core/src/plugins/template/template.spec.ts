import { parse } from "@vue/compiler-sfc";
import TemplatePlugin from "./index.js";
import { ParseScriptContext } from "../types.js";

describe("template", () => {
  it("should work", () => {
    // const parsed = parse(`<template><span>{{1}}</span></template>`);
    // const parsed = parse(`
    // <template>
    //     <span>1</span>
    //     <input v-model="msg" />
    //     <span v-cloak>{{ msg }}</span>
    //     <my-comp test="1"></my-comp>
    //     <my-comp v-for="i in 5" :key="i" :test="i"></my-comp>
    // </template>`);
    const parsed = parse(`
    <template>
        <my-comp v-for="i in 5" :key="i" :test="i"></my-comp>
    </template>`);

    const res = TemplatePlugin.process({
      sfc: parsed,
      template: parsed.descriptor.template,
    } as ParseScriptContext);

    expect(res).toBe(
      "Array.from({ length: 5 }).map((i) => <MyComp key={i} test={i} />);"
    );

    expect(res).toBe(1);
  });

  //   describe("walk", () => {});

  //   describe('walkElement', () => {
  //   })

  //   describe('walkAttribute', () => {
  //   })

  //   describe('walkExpressionNode', () => {
  //   })
});
