import { parse as sfcParse, MagicString } from "@vue/compiler-sfc";
import { parse } from "./parse";
import { process } from "./process";
import fs from "fs";

describe("process", () => {
  function testSourceMaps(
    result: string,
    sourceMap: ReturnType<MagicString["generateMap"]>
  ) {
    fs.writeFileSync(
      "D:/Downloads/sourcemap-test/sourcemap-example.js",
      result,
      "utf-8"
    );
    fs.writeFileSync(
      "D:/Downloads/sourcemap-test/sourcemap-example.js.map",
      sourceMap.toString(),
      "utf-8"
    );
  }

  function doParseContent(content: string) {
    const source = `<template>${content}</template>`;
    const ast = sfcParse(source, {});
    const root = ast.descriptor.template!.ast!;

    return parse(root);
  }

  function wrappedInTemplate(content: string) {
    return `<template>${content}</template>`;
  }

  it("Hello vue", () => {
    const source = `<div>Hello vue</div>`;
    const parsed = doParseContent(source);

    const { magicString } = process(parsed);
    expect(magicString.toString()).toMatchInlineSnapshot(
      `"<template><div>{ "Hello vue" }</div></template>"`
    );
    expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
      `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,GAAG,CAAC,eAAS,EAAE,GAAG"}"`
    );
  });

  it("component", () => {
    const source = `<my-component/>`;

    const parsed = doParseContent(source);

    const { magicString } = process(parsed);
    expect(magicString.toString()).toMatchInlineSnapshot(
      `"<template><___VERTER__comp.MyComponent/></template>"`
    );
    expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
      `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY"}"`
    );
  });

  // // COPY TEST
  // {

  //     const parsed = doParseContent(source);

  //     const { magicString } = process(parsed);
  //     expect(magicString.toString()).toMatchInlineSnapshot();
  //     expect(magicString.generateMap().toString()).toMatchInlineSnapshot()
  //    // testSourceMaps(magicString.toString(), magicString.generateMap({ hires: true, includeContent: true }))
  // }

  describe("props", () => {
    it("props w/:", () => {
      const source = `<my-component :foo="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent foo={___VETER__ctx.bar}/></template>"`
      );
      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAE,IAAI,eAAC,GAAG,CAAC"}"`
      );
    });

    it("props w/v-bind:", () => {
      const source = `<my-component v-bind:foo="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent foo={___VETER__ctx.bar}/></template>"`
      );
      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAQ,IAAI,eAAC,GAAG,CAAC"}"`
      );
    });

    it("v-bind", () => {
      const source = `<my-component v-bind="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent {...___VETER__ctx.bar}/></template>"`
      );
      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAQ,CAAP,iBAAQ,GAAG,CAAC"}"`
      );
    });

    it("binding with :bind", () => {
      const source = `<my-component :bind="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent bind={___VETER__ctx.bar}/></template>"`
      );
      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAE,KAAK,eAAC,GAAG,CAAC"}"`
      );
    });

    it("binding with v-bind:bind", () => {
      const source = `<my-component v-bind:bind="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent bind={___VETER__ctx.bar}/></template>"`
      );
      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAQ,KAAK,eAAC,GAAG,CAAC"}"`
      );
    });

    it("v-bind + props", () => {
      const source = `<my-component v-bind="bar" foo="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent {...___VETER__ctx.bar} foo="bar"/></template>"`
      );
      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAQ,CAAP,iBAAQ,GAAG,CAAC,CAAC,GAAG"}"`
      );
    });

    it("props + v-bind", () => {
      const source = `<my-component foo="bar" v-bind="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent foo="bar" {...___VETER__ctx.bar}/></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAC,GAAG,OAAc,CAAP,iBAAQ,GAAG,CAAC"}"`
      );
    });

    it("should pass boolen on just props", () => {
      const source = `<span foo/>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><span foo/></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,IAAI,CAAC,GAAG"}"`
      );
    });

    it("should camelCase props", () => {
      const source = `<span supa-awesome-prop="hello"></span>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><span supaAwesomeProp="hello"></span></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,IAAI,CAAC,eAAiB,WAAW,IAAI"}"`
      );
    });
    it('should camelCase "on" event listeners', () => {
      const source = `<span @check-for-something="test"></span>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><span onCheckForSomething={___VETER__ctx.test}></span></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,IAAI,CAAC,EAAC,iBAAmB,CAAC,eAAC,IAAI,CAAC,GAAG,IAAI"}"`
      );
    });

    it("should camelCase bind", () => {
      const source = `<span :model-value="test"></span>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><span modelValue={___VETER__ctx.test}></span></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,IAAI,CAAE,UAAW,CAAC,eAAC,IAAI,CAAC,GAAG,IAAI"}"`
      );
    });

    it.todo("should do class merge", () => {
      const source = `<span class="foo" :class="['hello']"></span>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot();

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot();
    });

    it("simple", () => {
      const source = `<span class="foo"></span>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><span class="foo"></span></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,IAAI,CAAC,KAAK,SAAS,IAAI"}"`
      );
    });

    it("multiple", () => {
      const source = `<span class="foo" id="bar"></span>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><span class="foo" id="bar"></span></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,IAAI,CAAC,KAAK,OAAO,EAAE,SAAS,IAAI"}"`
      );
    });

    it("with '", () => {
      const source = `<span class='foo'></span>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><span class='foo'></span></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,IAAI,CAAC,KAAK,SAAS,IAAI"}"`
      );
    });

    describe("directive", () => {
      describe("v-for", () => {
        it("simple", () => {
          const source = `<li v-for="item in items"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{renderList(___VETER__ctx.items,(item)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,UAAM,eAAS,KAAJ,EAAJ,IAAa,IAAxB,CAAC,EAAE,CAAsB,GAAG,EAAE,IAAC"}"`
          );

          testSourceMaps(
            magicString.toString(),
            magicString.generateMap({ hires: true, includeContent: true })
          );
        });

        it("destructing", () => {
          const source = `<li v-for="{ message } in items"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{renderList(___VETER__ctx.items,({ message })=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,UAAM,eAAgB,KAAJ,EAAX,WAAoB,IAA/B,CAAC,EAAE,CAA6B,GAAG,EAAE,IAAC"}"`
          );
        });

        it("index", () => {
          const source = `<li v-for="(item, index) in items"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{renderList(___VETER__ctx.items,(item, index)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,UAAM,eAAkB,KAAJ,CAAb,aAAsB,GAAjC,CAAC,EAAE,CAA+B,GAAG,EAAE,IAAC"}"`
          );
        });

        it("index + key", () => {
          const source = `<li v-for="(item, index) in items" :key="index + 'random'"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{renderList(___VETER__ctx.items,(item, index)=>{<li  key={index + 'random'}></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,UAAM,eAAkB,KAAJ,CAAb,aAAsB,GAAjC,CAAC,EAAE,CAA+B,CAAE,IAAI,CAAC,gBAAgB,CAAC,GAAG,EAAE,IAAC"}"`
          );
        });

        it("destructing + index", () => {
          const source = `<li v-for="({ message }, index) in items"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{renderList(___VETER__ctx.items,({ message }, index)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,UAAM,eAAyB,KAAJ,CAApB,oBAA6B,GAAxC,CAAC,EAAE,CAAsC,GAAG,EAAE,IAAC"}"`
          );
        });

        it("nested", () => {
          const source = `<li v-for="item in items">
      <span v-for="childItem in item.children"></span>
  </li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(`
            "<template>{renderList(___VETER__ctx.items,(item)=>{<li >
                  {renderList(item.children,(childItem)=>{<span ></span>})}
              </li>})}</template>"
          `);

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,UAAM,eAAS,KAAJ,EAAJ,IAAa,IAAxB,CAAC,EAAE,CAAsB;AACnC,OAAY,UAAM,CAAc,aAAJ,EAAT,SAA0B,IAAvC,CAAC,IAAI,CAAmC,GAAG,IAAI,IAAC;AACtD,IAAI,EAAE,IAAC"}"`
          );
        });

        it("of", () => {
          const source = `<li v-for="item of items"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{renderList(___VETER__ctx.items,(item)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,UAAM,eAAS,KAAJ,EAAJ,IAAa,IAAxB,CAAC,EAAE,CAAsB,GAAG,EAAE,IAAC"}"`
          );
        });

        it("of with tab", () => {
          const source = `<li v-for="item   of items"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{renderList(___VETER__ctx.items,(item  )=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,UAAM,eAAW,KAAJ,EAAN,MAAe,IAA1B,CAAC,EAAE,CAAwB,GAAG,EAAE,IAAC"}"`
          );
        });

        it("of with tabs", () => {
          const source = `<li v-for="item   of     items"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{renderList(    ___VETER__ctx.items,(item  )=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,UAAM,CAAW,kBAAI,KAAR,EAAN,MAAmB,IAA9B,CAAC,EAAE,CAA4B,GAAG,EAAE,IAAC"}"`
          );
        });

        // object
        it("object", () => {
          const source = `<li v-for="value in myObject"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{renderList(___VETER__ctx.myObject,(value)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,UAAM,eAAU,QAAJ,EAAL,KAAiB,IAA5B,CAAC,EAAE,CAA0B,GAAG,EAAE,IAAC"}"`
          );
        });

        it("object + key", () => {
          const source = `<li v-for="(value, key) in myObject"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{renderList(___VETER__ctx.myObject,(value, key)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,UAAM,eAAiB,QAAJ,CAAZ,YAAwB,GAAnC,CAAC,EAAE,CAAiC,GAAG,EAAE,IAAC"}"`
          );
        });

        it("object + key + index", () => {
          const source = `<li v-for="(value, key, index) in myObject"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{renderList(___VETER__ctx.myObject,(value, key, index)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,UAAM,eAAwB,QAAJ,CAAnB,mBAA+B,GAA1C,CAAC,EAAE,CAAwC,GAAG,EAAE,IAAC"}"`
          );
        });

        // range
        it("range", () => {
          const source = `<li v-for="n in 10"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{renderList(10,(n)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,UAAM,CAAM,EAAJ,EAAD,CAAO,IAAlB,CAAC,EAAE,CAAgB,GAAG,EAAE,IAAC"}"`
          );
        });

        // v-if has higher priority than v-for
        it("v-if", () => {
          const source = `<li v-for="i in items" v-if="items > 5"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{(___VETER__ctx.items > 5)?renderList(___VETER__ctx.items,(i)=>{<li  ></li>}) : undefined}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAsC,eAAC,SAAS,CAAf,CAAnB,UAAM,eAAM,KAAJ,EAAD,CAAU,IAArB,CAAC,EAAE,CAAmB,CAAiB,GAAG,EAAE,gBAAC"}"`
          );

          testSourceMaps(
            magicString.toString(),
            magicString.generateMap({ hires: true, includeContent: true })
          );
        });
      });

      describe("conditional v-if", () => {
        it("v-if", () => {
          const source = `<li v-if="n > 5"></li>`;
          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{(___VETER__ctx.n > 5)?<li ></li> : undefined}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(`"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAmB,eAAC,KAAK,CAAX,CAAJ,CAAC,EAAE,CAAa,GAAG,EAAE,cAAC"}"`);
        });

        it("v-if + v-else", () => {
          const source = `<li v-if="n > 5" id="if"></li><li v-else id="else"></li>`;
          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{(___VETER__ctx.n > 5)?<li  id="if"></li>:<li  id="else"></li>}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
          `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAmB,eAAC,KAAK,CAAX,CAAJ,CAAC,EAAE,CAAa,CAAC,EAAE,QAAQ,EAAE,CAAK,CAAJ,CAAC,EAAE,CAAO,CAAC,EAAE,UAAU,EAAE,EAAC"}"`);
        });

        it("v-if + v-else-if", () => {
          const source = `<li v-if="n > 5"></li><li v-else-if="n > 3"></li>`;
          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{(___VETER__ctx.n > 5)?<li ></li>:(___VETER__ctx.n > 3)?<li ></li> : undefined}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAmB,eAAC,KAAK,CAAX,CAAJ,CAAC,EAAE,CAAa,GAAG,EAAE,CAAe,gBAAC,KAAK,CAAhB,CAAJ,CAAC,EAAE,CAAkB,GAAG,EAAE,cAAC"}"`
          );
        });

        it("multiple conditions", () => {
          const source = `
              <li v-if="n === 1"></li>
              <li v-else-if="n === 1"></li>
              <li v-else-if="n === 1"></li>
              <li v-else-if="n === 1"></li>
              <li v-else-if="n === 1"></li>
              <li v-else-if="n === 1"></li>
              <li v-else></li>`;
          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(`
            "<template>
                          {(___VETER__ctx.n === 1)?<li ></li>
                          :(___VETER__ctx.n === 1)?<li ></li>
                          :(___VETER__ctx.n === 1)?<li ></li>
                          :(___VETER__ctx.n === 1)?<li ></li>
                          :(___VETER__ctx.n === 1)?<li ></li>
                          :(___VETER__ctx.n === 1)?<li ></li>
                          :<li ></li>}</template>"
          `);

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA;AACA,eAAuB,eAAC,OAAO,CAAb,CAAJ,CAAC,EAAE,CAAe,GAAG,EAAE;AACrC,cAA4B,gBAAC,OAAO,CAAlB,CAAJ,CAAC,EAAE,CAAoB,GAAG,EAAE;AAC1C,cAA4B,gBAAC,OAAO,CAAlB,CAAJ,CAAC,EAAE,CAAoB,GAAG,EAAE;AAC1C,cAA4B,gBAAC,OAAO,CAAlB,CAAJ,CAAC,EAAE,CAAoB,GAAG,EAAE;AAC1C,cAA4B,gBAAC,OAAO,CAAlB,CAAJ,CAAC,EAAE,CAAoB,GAAG,EAAE;AAC1C,cAA4B,gBAAC,OAAO,CAAlB,CAAJ,CAAC,EAAE,CAAoB,GAAG,EAAE;AAC1C,cAAkB,CAAJ,CAAC,EAAE,CAAO,GAAG,EAAE,EAAC"}"`
          );
        });

        describe.skip("invalid conditions", () => {
          it("v-else without v-if", () => {
            const source = `<li v-else></li>`;
            // expect(() => build(doParseElement(source))).throw(
            //   "v-else or v-else-if must be preceded by v-if"
            // );
            const parsed = doParseContent(source);
            const { magicString } = process(parsed);
            expect(magicString.toString()).toMatchInlineSnapshot(
              `"<template>{(___VETER__ctx.items > 5)?renderList(___VETER__ctx.items,(i)=>{<li  ></li>}) : undefined}</template>"`
            );

            expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
              `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAsC,eAAC,SAAS,CAAf,CAAnB,UAAM,eAAM,KAAJ,EAAD,CAAU,IAArB,CAAC,EAAE,CAAmB,CAAiB,GAAG,EAAE,gBAAC"}"`
            );

            testSourceMaps(
              magicString.toString(),
              magicString.generateMap({ hires: true, includeContent: true })
            );
          });

          it("v-else-if without v-if", () => {
            const source = `<li v-else-if></li>`;
            // expect(() => build(doParseElement(source))).throw(
            //   "v-else or v-else-if must be preceded by v-if"
            // );
            const parsed = doParseContent(source);
            const { magicString } = process(parsed);
            expect(magicString.toString()).toMatchInlineSnapshot(
              `"<template>{(___VETER__ctx.items > 5)?renderList(___VETER__ctx.items,(i)=>{<li  ></li>}) : undefined}</template>"`
            );

            expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
              `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAsC,eAAC,SAAS,CAAf,CAAnB,UAAM,eAAM,KAAJ,EAAD,CAAU,IAArB,CAAC,EAAE,CAAmB,CAAiB,GAAG,EAAE,gBAAC"}"`
            );

            testSourceMaps(
              magicString.toString(),
              magicString.generateMap({ hires: true, includeContent: true })
            );
          });
          it("v-else after v-else", () => {
            const source = `<li v-if="true"></li><li v-else></li><li v-else></li>`;
            // expect(() => build(doParseElement(source))).throw(
            //   "v-else or v-else-if must be preceded by v-if"
            // );
            const parsed = doParseContent(source);
            const { magicString } = process(parsed);
            expect(magicString.toString()).toMatchInlineSnapshot(
              `"<template>{(___VETER__ctx.items > 5)?renderList(___VETER__ctx.items,(i)=>{<li  ></li>}) : undefined}</template>"`
            );

            expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
              `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAsC,eAAC,SAAS,CAAf,CAAnB,UAAM,eAAM,KAAJ,EAAD,CAAU,IAArB,CAAC,EAAE,CAAmB,CAAiB,GAAG,EAAE,gBAAC"}"`
            );

            testSourceMaps(
              magicString.toString(),
              magicString.generateMap({ hires: true, includeContent: true })
            );
          });

          it("v-else-if after v-else", () => {
            const source = `<li v-if="true"></li><li v-else></li><li v-else></li>`;
            // expect(() => build(doParseElement(source))).throw(
            //   "v-else or v-else-if must be preceded by v-if"
            // );
            const parsed = doParseContent(source);
            const { magicString } = process(parsed);
            expect(magicString.toString()).toMatchInlineSnapshot(
              `"<template>{(___VETER__ctx.items > 5)?renderList(___VETER__ctx.items,(i)=>{<li  ></li>}) : undefined}</template>"`
            );

            expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
              `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAsC,eAAC,SAAS,CAAf,CAAnB,UAAM,eAAM,KAAJ,EAAD,CAAU,IAArB,CAAC,EAAE,CAAmB,CAAiB,GAAG,EAAE,gBAAC"}"`
            );

            testSourceMaps(
              magicString.toString(),
              magicString.generateMap({ hires: true, includeContent: true })
            );
          });
        });
      });

      describe("v-bind", () => {
        it("v-bind", () => {
          const source = `<div v-bind="props" />`;
          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><div {...___VETER__ctx.props} /></template>"`);

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
          `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,GAAG,CAAQ,CAAP,iBAAQ,KAAK,CAAC"}"`);
        });
        it("v-bind name", () => {
          const source = `<div v-bind:name="props" />`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><div name={___VETER__ctx.props} /></template>"`);

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
          `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,GAAG,CAAQ,KAAK,eAAC,KAAK,CAAC"}"`);
        });

        it("v-bind :short", () => {
          const source = `<div :name />`;
          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template><div name={___VETER__ctx.name} /></template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,GAAG,CAAE,IAAD,qBAAK"}"`
          );
        });

        it("v-bind :shorter", () => {
          const source = `<div :name="name" />`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><div name={___VETER__ctx.name} /></template>"`);

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
          `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,GAAG,CAAE,KAAK,eAAC,IAAI,CAAC"}"`);
        });
      });

      // NOTE waiting for https://github.com/vuejs/core/pull/3399
      describe.skip("v-model", () => {
        it("v-model", () => {
          const source = `<MyComp v-model="msg" />`;
          //   const parsed = doParseElement(source);
          //   const built = build(parsed);
          //   expect(built).toMatchInlineSnapshot(
          //     `"<MyComp modelValue={msg} onUpdate:modelValue={(e) => msg = e} />"`
          //   );

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template><div {...props} /></template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,GAAG,CAAQ,CAAP,GAAQ,KAAK,CAAC"}"`
          );

          testSourceMaps(
            magicString.toString(),
            magicString.generateMap({ hires: true, includeContent: true })
          );
        });

        it("v-model named", () => {
          const source = `<MyComp v-model:value="msg" />`;
          //   const parsed = doParseElement(source);
          //   const built = build(parsed);
          //   expect(built).toMatchInlineSnapshot(
          //     `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          //   );

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template><div {...props} /></template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,GAAG,CAAQ,CAAP,GAAQ,KAAK,CAAC"}"`
          );

          testSourceMaps(
            magicString.toString(),
            magicString.generateMap({ hires: true, includeContent: true })
          );
        });

        it("v-model number", () => {
          const source = `<MyComp v-model.number="msg" />`;
          //   const parsed = doParseElement(source);
          //   const built = build(parsed);
          //   expect(built).toMatchInlineSnapshot(
          //     `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          //   );

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template><div {...props} /></template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,GAAG,CAAQ,CAAP,GAAQ,KAAK,CAAC"}"`
          );

          testSourceMaps(
            magicString.toString(),
            magicString.generateMap({ hires: true, includeContent: true })
          );
        });

        it("v-model  number + trim", () => {
          const source = `<MyComp v-model.numbe.trim="msg" />`;
          //   const parsed = doParseElement(source);
          //   const built = build(parsed);
          //   expect(built).toMatchInlineSnapshot(
          //     `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          //   );

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template><div {...props} /></template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,GAAG,CAAQ,CAAP,GAAQ,KAAK,CAAC"}"`
          );

          testSourceMaps(
            magicString.toString(),
            magicString.generateMap({ hires: true, includeContent: true })
          );
        });

        it("v-model named + number", () => {
          const source = `<MyComp v-model:value.number="msg" />`;
          //   const parsed = doParseElement(source);
          //   const built = build(parsed);
          //   expect(built).toMatchInlineSnapshot(
          //     `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          //   );

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template><div {...props} /></template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,GAAG,CAAQ,CAAP,GAAQ,KAAK,CAAC"}"`
          );

          testSourceMaps(
            magicString.toString(),
            magicString.generateMap({ hires: true, includeContent: true })
          );
        });

        it("v-model named + number + trim", () => {
          const source = `<MyComp v-model:value.numbe.trim="msg" />`;
          //   const parsed = doParseElement(source);
          //   const built = build(parsed);
          //   expect(built).toMatchInlineSnapshot(
          //     `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          //   );

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template><div {...props} /></template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAW,GAAG,CAAQ,CAAP,GAAQ,KAAK,CAAC"}"`
          );

          testSourceMaps(
            magicString.toString(),
            magicString.generateMap({ hires: true, includeContent: true })
          );
        });
      });
    });
  });
});
