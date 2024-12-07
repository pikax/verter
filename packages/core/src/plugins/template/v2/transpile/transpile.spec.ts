import { MagicString, parse as sfcParse } from "@vue/compiler-sfc";
import { transpile } from "./";

describe("transpile", () => {
  function doTranspile(content: string) {
    const source = `<template>${content}</template>`;

    const sfc = sfcParse(source);

    const template = sfc.descriptor.template;
    const ast = template.ast!;
    const s = new MagicString(ast.source);
    const c = transpile(ast, s);

    return {
      sfc,
      s,
      c,

      original: s.original,
      result: s.toString(),
    };
  }

  describe("simple", () => {
    it("root", () => {
      const { result } = doTranspile("");

      expect(result).toMatchInlineSnapshot(`"<template></template>"`);
    });

    it("Hello vue", () => {
      const source = `<div>Hello vue</div>`;
      const { result } = doTranspile(source);

      expect(result).toMatchInlineSnapshot(`"<template><div>{ "Hello vue" }</div></template>"`);
    });

    it("component self closing", () => {
      const source = `<my-component/>`;

      const { result } = doTranspile(source);

      expect(result).toMatchInlineSnapshot(
        `"<template><___VERTER___comp.MyComponent/></template>"`
      );
    });

    it("comment", () => {
      const source = `<!-- this is comment -->`;

      const { result } = doTranspile(source);

      expect(result).toMatchInlineSnapshot(`"<template>{/* this is comment */}</template>"`);
    });

    it("partial", () => {
      const source = `<div> < </div>`;
      const { result } = doTranspile(source);
      expect(result).toMatchInlineSnapshot(`"<template><div> < </div></template>"`);
    });

    it("v-if with expression", () => {
      const { result } = doTranspile(`<div v-if="(() => {
            let ii = '0';
            return ii === ii
          })()">
            t4est
          </div>
          <div v-else>
            else
          </div>`);

      expect(result).toMatchInlineSnapshot(`
        "<template>{ (): any => {if((() => {
                    let ii = '0';
                    return ii === ii
                  })()){<div >{ "t4est" }</div>}
                  else{
        <div >{ "else" }</div>
        }}}</template>"
      `);
    });
  });
});
