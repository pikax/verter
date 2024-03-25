import { parse as sfcParse, MagicString } from "@vue/compiler-sfc";
import { parse } from "../parse/parse";
import { process as processFn } from "./process";
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

  function process(parsed: ReturnType<typeof parse>) {
    const s = new MagicString(parsed.node.source);

    const r = processFn(parsed, s);
    return {
      ...r,
      magicString: s,
    };
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
      `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,eAAe,eAAS,EAAE,GAAG"}"`
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
        `"<template><___VERTER__comp.MyComponent foo={___VERTER__ctx.bar}/></template>"`
      );
      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAE,IAAI,gBAAC,GAAG,CAAC"}"`
      );
    });

    it("props w/v-bind:", () => {
      const source = `<my-component v-bind:foo="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent foo={___VERTER__ctx.bar}/></template>"`
      );
      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAQ,IAAI,gBAAC,GAAG,CAAC"}"`
      );
    });

    it("v-bind", () => {
      const source = `<my-component v-bind="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent {...___VERTER__ctx.bar}/></template>"`
      );
      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAQ,CAAP,kBAAQ,GAAG,CAAC"}"`
      );
    });

    it("binding with :bind", () => {
      const source = `<my-component :bind="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent bind={___VERTER__ctx.bar}/></template>"`
      );
      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAE,KAAK,gBAAC,GAAG,CAAC"}"`
      );
    });

    it("binding boolean", () => {
      const source = `<my-component :bind="false"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent bind={false}/></template>"`
      );
      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAE,KAAK,CAAC,KAAK,CAAC"}"`
      );
    });

    it("binding with v-bind:bind", () => {
      const source = `<my-component v-bind:bind="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent bind={___VERTER__ctx.bar}/></template>"`
      );
      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAQ,KAAK,gBAAC,GAAG,CAAC"}"`
      );
    });

    it("v-bind + props", () => {
      const source = `<my-component v-bind="bar" foo="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent {...___VERTER__ctx.bar} foo="bar"/></template>"`
      );
      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAQ,CAAP,kBAAQ,GAAG,CAAC"}"`
      );
    });

    it("props + v-bind", () => {
      const source = `<my-component foo="bar" v-bind="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent foo="bar" {...___VERTER__ctx.bar}/></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,WAAkB,CAAP,kBAAQ,GAAG,CAAC"}"`
      );
    });

    it("should keep casing", () => {
      const source = `<my-component aria-autocomplete="bar"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent aria-autocomplete="bar"/></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY"}"`
      );
      // testSourceMaps(magicString.toString(), magicString.generateMap({ hires: true, includeContent: true }))
    });

    it("should keep casing on binding", () => {
      const source = `<my-component :aria-autocomplete="'bar'"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent aria-autocomplete={'bar'}/></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY"}"`
      );
    });

    it("should pass boolean on just props", () => {
      const source = `<span foo/>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><span foo/></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA"}"`
      );
    });

    it("should not camelCase props", () => {
      const source = `<span supa-awesome-prop="hello"></span>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><span supa-awesome-prop="hello"></span></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,4CAA4C,IAAI"}"`
      );
    });
    it('should camelCase "on" event listeners', () => {
      const source = `<span @check-for-something="test"></span>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><span onCheckForSomething={___VERTER__ctx.test}></span></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,gBAAgB,EAAC,iBAAmB,CAAC,gBAAC,IAAI,CAAC,GAAG,IAAI"}"`
      );
    });

    it("should camelCase bind", () => {
      const source = `<span :model-value="test"></span>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><span modelValue={___VERTER__ctx.test}></span></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,gBAAiB,UAAW,CAAC,gBAAC,IAAI,CAAC,GAAG,IAAI"}"`
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
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,8BAA8B,IAAI"}"`
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
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,uCAAuC,IAAI"}"`
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
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,8BAA8B,IAAI"}"`
      );
    });

    describe("directive", () => {
      describe("v-for", () => {
        it("simple", () => {
          const source = `<li v-for="item in items"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{__VERTER__renderList(___VERTER__ctx.items,(item)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,oBAAM,gBAAS,KAAJ,EAAJ,IAAa,IAAxB,IAAyB,GAAG,EAAE,IAAC"}"`
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
            `"<template>{__VERTER__renderList(___VERTER__ctx.items,({ message })=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,oBAAM,gBAAgB,KAAJ,EAAX,WAAoB,IAA/B,IAAgC,GAAG,EAAE,IAAC"}"`
          );
        });

        it("index", () => {
          const source = `<li v-for="(item, index) in items"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{__VERTER__renderList(___VERTER__ctx.items,(item, index)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,oBAAM,gBAAkB,KAAJ,CAAb,aAAsB,GAAjC,IAAkC,GAAG,EAAE,IAAC"}"`
          );
        });

        it("index + key", () => {
          const source = `<li v-for="(item, index) in items" :key="index + 'random'"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{__VERTER__renderList(___VERTER__ctx.items,(item, index)=>{<li  key={index + 'random'}></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,oBAAM,gBAAkB,KAAJ,CAAb,aAAsB,GAAjC,IAAkC,CAAE,IAAI,CAAC,gBAAgB,CAAC,GAAG,EAAE,IAAC"}"`
          );
        });

        it("destructing + index", () => {
          const source = `<li v-for="({ message }, index) in items"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{__VERTER__renderList(___VERTER__ctx.items,({ message }, index)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,oBAAM,gBAAyB,KAAJ,CAApB,oBAA6B,GAAxC,IAAyC,GAAG,EAAE,IAAC"}"`
          );
        });

        it("nested", () => {
          const source = `<li v-for="item in items">
      <span v-for="childItem in item.children"></span>
  </li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(`
            "<template>{__VERTER__renderList(___VERTER__ctx.items,(item)=>{<li >
                  {__VERTER__renderList(item.children,(childItem)=>{<span ></span>})}
              </li>})}</template>"
          `);

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,oBAAM,gBAAS,KAAJ,EAAJ,IAAa,IAAxB,IAAyB;AACnC,OAAY,oBAAM,CAAc,aAAJ,EAAT,SAA0B,IAAvC,MAAwC,GAAG,IAAI,IAAC;AACtD,IAAI,EAAE,IAAC"}"`
          );
        });

        it("of", () => {
          const source = `<li v-for="item of items"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{__VERTER__renderList(___VERTER__ctx.items,(item)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,oBAAM,gBAAS,KAAJ,EAAJ,IAAa,IAAxB,IAAyB,GAAG,EAAE,IAAC"}"`
          );
        });

        it("of with tab", () => {
          const source = `<li v-for="item   of items"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{__VERTER__renderList(___VERTER__ctx.items,(item  )=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,oBAAM,gBAAW,KAAJ,EAAN,MAAe,IAA1B,IAA2B,GAAG,EAAE,IAAC"}"`
          );
        });

        it("of with tabs", () => {
          const source = `<li v-for="item   of     items"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{__VERTER__renderList(    ___VERTER__ctx.items,(item  )=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,oBAAM,CAAW,mBAAI,KAAR,EAAN,MAAmB,IAA9B,IAA+B,GAAG,EAAE,IAAC"}"`
          );
        });

        // object
        it("object", () => {
          const source = `<li v-for="value in myObject"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{__VERTER__renderList(___VERTER__ctx.myObject,(value)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,oBAAM,gBAAU,QAAJ,EAAL,KAAiB,IAA5B,IAA6B,GAAG,EAAE,IAAC"}"`
          );
        });

        it("object + key", () => {
          const source = `<li v-for="(value, key) in myObject"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{__VERTER__renderList(___VERTER__ctx.myObject,(value, key)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,oBAAM,gBAAiB,QAAJ,CAAZ,YAAwB,GAAnC,IAAoC,GAAG,EAAE,IAAC"}"`
          );
        });

        it("object + key + index", () => {
          const source = `<li v-for="(value, key, index) in myObject"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{__VERTER__renderList(___VERTER__ctx.myObject,(value, key, index)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,oBAAM,gBAAwB,QAAJ,CAAnB,mBAA+B,GAA1C,IAA2C,GAAG,EAAE,IAAC"}"`
          );
        });

        // range
        it("range", () => {
          const source = `<li v-for="n in 10"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{__VERTER__renderList(10,(n)=>{<li ></li>})}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAc,oBAAM,CAAM,EAAJ,EAAD,CAAO,IAAlB,IAAmB,GAAG,EAAE,IAAC"}"`
          );
        });

        // v-if has higher priority than v-for
        it("v-if", () => {
          const source = `<li v-for="i in items" v-if="items > 5"></li>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{(___VERTER__ctx.items > 5)?__VERTER__renderList(___VERTER__ctx.items,(i)=>{<li  ></li>}) : undefined}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAsC,gBAAC,SAAS,CAAf,CAAnB,oBAAM,gBAAM,KAAJ,EAAD,CAAU,IAArB,IAAsB,CAAiB,GAAG,EAAE,gBAAC"}"`
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
            `"<template>{(___VERTER__ctx.n > 5)?<li ></li> : undefined}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAmB,gBAAC,KAAK,CAAX,CAAJ,IAAgB,GAAG,EAAE,cAAC"}"`
          );
        });

        it("v-if + v-else", () => {
          const source = `<li v-if="n > 5" id="if"></li><li v-else id="else"></li>`;
          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{(___VERTER__ctx.n > 5)?<li  id="if"></li>:<li  id="else"></li>}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAmB,gBAAC,KAAK,CAAX,CAAJ,IAAgB,WAAW,EAAE,CAAK,CAAJ,IAAU,aAAa,EAAE,EAAC"}"`
          );
        });

        it("v-if + v-else-if", () => {
          const source = `<li v-if="n > 5"></li><li v-else-if="n > 3"></li>`;
          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{(___VERTER__ctx.n > 5)?<li ></li>:(___VERTER__ctx.n > 3)?<li ></li> : undefined}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAmB,gBAAC,KAAK,CAAX,CAAJ,IAAgB,GAAG,EAAE,CAAe,iBAAC,KAAK,CAAhB,CAAJ,IAAqB,GAAG,EAAE,cAAC"}"`
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
                          {(___VERTER__ctx.n === 1)?<li ></li>
                          :(___VERTER__ctx.n === 1)?<li ></li>
                          :(___VERTER__ctx.n === 1)?<li ></li>
                          :(___VERTER__ctx.n === 1)?<li ></li>
                          :(___VERTER__ctx.n === 1)?<li ></li>
                          :(___VERTER__ctx.n === 1)?<li ></li>
                          :<li ></li>}</template>"
          `);

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA;AACA,eAAuB,gBAAC,OAAO,CAAb,CAAJ,IAAkB,GAAG,EAAE;AACrC,cAA4B,iBAAC,OAAO,CAAlB,CAAJ,IAAuB,GAAG,EAAE;AAC1C,cAA4B,iBAAC,OAAO,CAAlB,CAAJ,IAAuB,GAAG,EAAE;AAC1C,cAA4B,iBAAC,OAAO,CAAlB,CAAJ,IAAuB,GAAG,EAAE;AAC1C,cAA4B,iBAAC,OAAO,CAAlB,CAAJ,IAAuB,GAAG,EAAE;AAC1C,cAA4B,iBAAC,OAAO,CAAlB,CAAJ,IAAuB,GAAG,EAAE;AAC1C,cAAkB,CAAJ,IAAU,GAAG,EAAE,EAAC"}"`
          );
        });

        it("v-if with expression", () => {
          const source = `<div v-if="(() => {
            let ii = '0';
            return ii === ii
          })()">
            t4est
          </div>
          <div v-else>
            else
          </div>`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `
            "<template>{((() => {
                        let ii = '0';
                        return ii === ii
                      })())?<div >{ " t4est " }</div>
                      :<div >{ " else " }</div>}</template>"
          `
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAoB,CAAC;AACrB;AACA;AACA,cAAc,CAHC,CAAL,KAGK,CAAC,aAEN,EAAE,GAAG;AACf,UAAe,CAAL,KAAW,CAAC,YAEZ,EAAE,GAAG,EAAC"}"`
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
              `"<template>{(___VERTER__ctx.items > 5)?__VERTER__renderList(___VERTER__ctx.items,(i)=>{<li  ></li>}) : undefined}</template>"`
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
              `"<template>{(___VERTER__ctx.items > 5)?__VERTER__renderList(___VERTER__ctx.items,(i)=>{<li  ></li>}) : undefined}</template>"`
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
              `"<template>{(___VERTER__ctx.items > 5)?__VERTER__renderList(___VERTER__ctx.items,(i)=>{<li  ></li>}) : undefined}</template>"`
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
              `"<template>{(___VERTER__ctx.items > 5)?__VERTER__renderList(___VERTER__ctx.items,(i)=>{<li  ></li>}) : undefined}</template>"`
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
            `"<template><div {...___VERTER__ctx.props} /></template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,eAAsB,CAAP,kBAAQ,KAAK,CAAC"}"`
          );
        });
        it("v-bind name", () => {
          const source = `<div v-bind:name="props" />`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template><div name={___VERTER__ctx.props} /></template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,eAAsB,KAAK,gBAAC,KAAK,CAAC"}"`
          );
        });

        it("v-bind :short", () => {
          const source = `<div :name />`;
          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template><div name={___VERTER__ctx.name} /></template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,eAAgB,IAAD,sBAAK"}"`
          );
        });

        it("v-bind :shorter", () => {
          const source = `<div :name="name" />`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template><div name={___VERTER__ctx.name} /></template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,eAAgB,KAAK,gBAAC,IAAI,CAAC"}"`
          );
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

  describe("comment", () => {
    it("should parse the comment", () => {
      const source = `<!-- This is a comment -->`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template>{ /* This is a comment */ }</template>"`
      );
    });

    it("ts-expect-error", () => {
      const source = `<!-- @ts-expect-error this is expected to fail -->
      <this-does-not-exist />
      `;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(`
        "<template>{ /* @ts-expect-error this is expected to fail */ }
              <___VERTER__comp.ThisDoesNotExist />
              </template>"
      `);
    });
  });

  describe("interpolation", () => {
    it("should interpolate", () => {
      const source = `<div>{{ foo}}</div>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><div>{ ___VERTER__ctx.foo}</div></template>"`
      );
    });

    it("complex", () => {
      const source = `<div>{{ foo + 'myString' + document.width }}</div>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><div>{ ___VERTER__ctx.foo + 'myString' + ___VERTER__ctx.document.width }</div></template>"`
      );
    });
  });
});
