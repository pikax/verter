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

    it("props + binding on array", () => {
      const source = `<my-component :foo="[bar]" />`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent foo={[___VERTER__ctx.bar]} /></template>"`
      );
    });

    it("props + binding complex", () => {
      const source = `<my-component :foo="isFoo ? { myFoo: foo } : undefined" />`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);

      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent foo={___VERTER__ctx.isFoo ? { myFoo: ___VERTER__ctx.foo } : undefined} /></template>"`
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
    });

    it("should keep casing on binding", () => {
      const source = `<my-component :aria-autocomplete="'bar'"/>`;

      const parsed = doParseContent(source);

      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><___VERTER__comp.MyComponent aria-autocomplete={'bar'}/></template>"`
      );

      expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
        `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,2BAAW,WAAY,CAAE,kBAAkB,CAAC,KAAK,CAAC"}"`
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

    it("should append ctx inside of functions", () => {
      const source = `<span :check-for-something="e=> { foo = e }"></span>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><span checkForSomething={e=> { ___VERTER__ctx.foo = e }}></span></template>"`
      );
    });

    it("should  append ctx inside a string interpolation", () => {
      const source = '<span :check-for-something="`foo=${bar}`"></span>';

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><span checkForSomething={\`foo=\${___VERTER__ctx.bar}\`}></span></template>"`
      );
    });

    describe("events", () => {
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

      it("should append ctx inside of functions", () => {
        const source = `<span @check-for-something="e=> { foo = e }"></span>`;

        const parsed = doParseContent(source);
        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span onCheckForSomething={e=> { ___VERTER__ctx.foo = e }}></span></template>"`
        );
      });

      it("should add event correctly", () => {
        const source = `<span @back="navigateToSession(null)"/>`;

        const parsed = doParseContent(source);
        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span onBack={___VERTER__ctx.navigateToSession(null)}/></template>"`
        );
      });
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

    describe("bind complex", () => {
      it("should do class merge", () => {
        const source = `<span class="foo" :class="['hello']"></span>`;

        const parsed = doParseContent(source);
        const { magicString } = process(parsed);

        testSourceMaps(
          magicString.toString(),
          magicString.generateMap({ hires: true, includeContent: true })
        );
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span  class={__VERTER__normalizeClass([['hello'],"foo"])}></span></template>"`
        );
      });
      it("should do class merge with v-bind", () => {
        const source = `<span class="foo" :class="['hello']" v-bind:class="{'oi': true}"></span>`;

        const parsed = doParseContent(source);
        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span  class={__VERTER__normalizeClass([['hello'],"foo",{'oi': true}])} ></span></template>"`
        );

        testSourceMaps(
          magicString.toString(),
          magicString.generateMap({ hires: true, includeContent: true })
        );
      });

      it("should do class merge with v-bind with attributes in between", () => {
        const source = `<span class="foo" don-t :class="['hello']" v-bind:class="{'oi': true}"></span>`;

        const parsed = doParseContent(source);
        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span  don-t class={__VERTER__normalizeClass([['hello'],"foo",{'oi': true}])} ></span></template>"`
        );
      });

      it("should still append context accessor", () => {
        const source = `<span :class="foo" don-t :class="['hello', bar]" v-bind:class="{'oi': true, sup}"></span>`;

        const parsed = doParseContent(source);

        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span class={__VERTER__normalizeClass([___VERTER__ctx.foo,['hello', ___VERTER__ctx.bar],{'oi': true, sup:___VERTER__ctx.sup}])} don-t  ></span></template>"`
        );
      });

      it("complex class", () => {
        const source = `
        <span  :class="[
          noBackdrop || (isBackdropAnimationDone && shouldClose)
            ? 'bg-transparent'
            : ' bg-black bg-opacity-70',
          !isBackdropAnimationDone && !noBackdrop ? 'animate-modal-backdrop' : '',
          from === 'bottom' ? 'justify-end' : 'justify-center',
          shouldClose && !noBackdrop ? 'animate-modal-backdrop-close' : '',
        ]"></span>`;

        const parsed = doParseContent(source);

        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(`
          "<template>
                  <span  class={[
                    ___VERTER__ctx.noBackdrop || (___VERTER__ctx.isBackdropAnimationDone && ___VERTER__ctx.shouldClose)
                      ? 'bg-transparent'
                      : ' bg-black bg-opacity-70',
                    !___VERTER__ctx.isBackdropAnimationDone && !___VERTER__ctx.noBackdrop ? 'animate-modal-backdrop' : '',
                    ___VERTER__ctx.from === 'bottom' ? 'justify-end' : 'justify-center',
                    ___VERTER__ctx.shouldClose && !___VERTER__ctx.noBackdrop ? 'animate-modal-backdrop-close' : '',
                  ]}></span></template>"
        `);
      });

      it("should do style merge", () => {
        const source = `<span style="foo" :style="['hello']"></span>`;

        const parsed = doParseContent(source);
        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span  style={__VERTER__normalizeStyle([['hello'],"foo"])}></span></template>"`
        );
      });
      it("should do style merge with v-bind ", () => {
        const source = `<span style="foo" :style="['hello']" v-bind:style="{'oi': true}"></span>`;

        const parsed = doParseContent(source);
        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span  style={__VERTER__normalizeStyle([['hello'],"foo",{'oi': true}])} ></span></template>"`
        );
      });
      it("should do style merge with v-bind with attributes in between", () => {
        const source = `<span style="foo" don-t :style="['hello']" v-bind:style="{'oi': true}"></span>`;

        const parsed = doParseContent(source);
        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span  don-t style={__VERTER__normalizeStyle([['hello'],"foo",{'oi': true}])} ></span></template>"`
        );
      });

      it("should still append context accessor style", () => {
        const source = `<span :style="foo" don-t :style="['hello', bar]" v-bind:style="{'oi': true, sup}"></span>`;

        const parsed = doParseContent(source);

        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span style={__VERTER__normalizeStyle([___VERTER__ctx.foo,['hello', ___VERTER__ctx.bar],{'oi': true, sup:___VERTER__ctx.sup}])} don-t  ></span></template>"`
        );
      });

      it("should append context on objects shothand", () => {
        const source = `<span :style="{colour}"></span>`;

        const parsed = doParseContent(source);
        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span style={{colour:___VERTER__ctx.colour}}></span></template>"`
        );
      });

      it("should append context on objects", () => {
        const source = `<span :style="{colour: myColour}"></span>`;

        const parsed = doParseContent(source);
        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span style={{colour: ___VERTER__ctx.myColour}}></span></template>"`
        );
      });

      it("should append context on dynamic accessor", () => {
        const source = `<span :style="{[colour]: true}"></span>`;

        const parsed = doParseContent(source);
        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span style={{[colour]: true}}></span></template>"`
        );
      });

      it("should append context on dynamic accessor + value", () => {
        const source = `<span :style="{[colour]: myColour}"></span>`;

        const parsed = doParseContent(source);
        const { magicString } = process(parsed);
        expect(magicString.toString()).toMatchInlineSnapshot(
          `"<template><span style={{[colour]: ___VERTER__ctx.myColour}}></span></template>"`
        );
      });
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
            `"<template>{(___VERTER__ctx.items > 5)?__VERTER__renderList(___VERTER__ctx.items,(i)=>{!((___VERTER__ctx.items > 5)) ? undefined : <li  ></li>}) : undefined}</template>"`
          );

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAsC,gBAAC,SAAS,CAAf,CAAnB,oBAAM,gBAAM,KAAJ,EAAD,CAAU,gDAArB,IAAsB,CAAiB,GAAG,EAAE,gBAAC"}"`
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

        it("v-if + >", () => {
          const source = `<div v-if="getData.length > 0"> </div>`;
          const parsed = doParseContent(source);
          const { magicString } = process(parsed);
          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{(___VERTER__ctx.getData.length > 0)?<div > </div> : undefined}</template>"`
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
          expect(magicString.toString()).toMatchInlineSnapshot(`
            "<template>{((() => {
                        let ii = '0';
                        return ___VERTER__ctx.ii === ___VERTER__ctx.ii
                      })())?<div >{ " t4est " }</div>
                      :<div >{ " else " }</div>}</template>"
          `);

          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAoB,CAAC;AACrB;AACA,kCAAmB,sBAAO;AAC1B,cAAc,CAHC,CAAL,KAGK,CAAC,aAEN,EAAE,GAAG;AACf,UAAe,CAAL,KAAW,CAAC,YAEZ,EAAE,GAAG,EAAC"}"`
          );
        });

        it("should narrow", () => {
          const source = `<li v-if="n === true" :key="n"></li><li v-else :key="n"/>`;
          const parsed = doParseContent(source);
          const { magicString } = process(parsed);

          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template>{(___VERTER__ctx.n === true)?<li  key={___VERTER__ctx.n}></li>:<li  key={___VERTER__ctx.n}/>}</template>"`
          );
          expect(magicString.generateMap().toString()).toMatchInlineSnapshot(
            `"{"version":3,"sources":[""],"names":[],"mappings":"AAAA,WAAmB,gBAAC,UAAU,CAAhB,CAAJ,IAAqB,CAAE,IAAI,gBAAC,CAAC,CAAC,GAAG,EAAE,CAAK,CAAJ,IAAU,CAAE,IAAI,gBAAC,CAAC,CAAC,GAAE"}"`
          );
        });

        describe("narrow", () => {
          it("arrow function", () => {
            const source = `<li v-if="n.n.n === true" :onClick="()=> n.n.n === false ? 1 : undefined"></li><li v-else :onClick="()=> n.n.n === true ? 1 : undefined"/>`;
            const parsed = doParseContent(source);
            const { magicString } = process(parsed);

            // NOTE the resulted snapshot should give an error with typescript in the correct environment
            expect(magicString.toString()).toMatchInlineSnapshot(
              `"<template>{(___VERTER__ctx.n.n.n === true)?<li  onClick={()=> !((___VERTER__ctx.n.n.n === true)) ? undefined : ___VERTER__ctx.n.n.n === false ? 1 : undefined}></li>:<li  onClick={()=> (___VERTER__ctx.n.n.n === true) ? undefined : ___VERTER__ctx.n.n.n === true ? 1 : undefined}/>}</template>"`
            );
          });
          it("arrow function with return", () => {
            const source = `<li v-if="n.n.n === true" :onClick="()=> { return  n.n.n === false ? 1 : undefined }" ></li><li v-else :onClick="()=>{ return n.n.n === true ? 1 : undefined}"/>`;
            const parsed = doParseContent(source);
            const { magicString } = process(parsed);

            // NOTE the resulted snapshot should give an error with typescript in the correct environment
            expect(magicString.toString()).toMatchInlineSnapshot(
              `"<template>{(___VERTER__ctx.n.n.n === true)?<li  onClick={()=> { if(!((___VERTER__ctx.n.n.n === true))) { return; } return  ___VERTER__ctx.n.n.n === false ? 1 : undefined }} ></li>:<li  onClick={()=>{ if((___VERTER__ctx.n.n.n === true)) { return; } return ___VERTER__ctx.n.n.n === true ? 1 : undefined}}/>}</template>"`
            );
          });

          it("function", () => {
            const source = `<li v-if="n.n.n === true" :onClick="function() { return  n.n.n === false ? 1 : undefined } "></li><li v-else :onClick="function(){ return n.n.n === true ? 1 : undefined}"/>`;
            const parsed = doParseContent(source);
            const { magicString } = process(parsed);

            // NOTE the resulted snapshot should give an error with typescript in the correct environment
            expect(magicString.toString()).toMatchInlineSnapshot(
              `"<template>{(___VERTER__ctx.n.n.n === true)?<li  onClick={function() { if(!((___VERTER__ctx.n.n.n === true))) { return; } return  ___VERTER__ctx.n.n.n === false ? 1 : undefined } }></li>:<li  onClick={function(){ if((___VERTER__ctx.n.n.n === true)) { return; } return ___VERTER__ctx.n.n.n === true ? 1 : undefined}}/>}</template>"`
            );
          });

          it("narrow on conditional arrow function", () => {
            const source = `<li v-if="n.n.n === true" :onClick="n.n.a === true ? (()=> n.n.n === false || n.n.a === false) : undefined "></li>`;

            const parsed = doParseContent(source);
            const { magicString } = process(parsed);

            // NOTE the resulted snapshot should give an error with typescript in the correct environment
            expect(magicString.toString()).toMatchInlineSnapshot(
              `"<template>{(___VERTER__ctx.n.n.n === true)?<li  onClick={___VERTER__ctx.n.n.a === true ? (()=> !((___VERTER__ctx.n.n.n === true) && ___VERTER__ctx.n.n.a === true) ? undefined : ___VERTER__ctx.n.n.n === false || ___VERTER__ctx.n.n.a === false) : undefined }></li> : undefined}</template>"`
            );
          });

          it("narrow with v-for", () => {
            /**
             * To test, check with
             * ```ts
             * declare const r: { n: true, items: string[] } | { n: false, items: number[] };
             * ```
             */
            const source = `<div v-for="item in r.items" v-if="r.n === false" :key="r.n === true ? 1 : false"></div>`;

            const parsed = doParseContent(source);
            const { magicString } = process(parsed);

            // NOTE the resulted snapshot should give an error with typescript in the correct environment
            expect(magicString.toString()).toMatchInlineSnapshot(
              `"<template>{(___VERTER__ctx.r.n === false)?__VERTER__renderList(___VERTER__ctx.r.items,(item)=>{!((___VERTER__ctx.r.n === false)) ? undefined : <div   key={___VERTER__ctx.r.n === true ? 1 : false}></div>}) : undefined}</template>"`
            );
          });
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

        it("bind arrow function", () => {
          const source = `<div :name="()=>name" />`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);

          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template><div name={()=>___VERTER__ctx.name} /></template>"`
          );
        });

        it("bind arrow function with return ", () => {
          const source = `<div :name="()=>{ return name }" />`;

          const parsed = doParseContent(source);
          const { magicString } = process(parsed);

          expect(magicString.toString()).toMatchInlineSnapshot(
            `"<template><div name={()=>{ return ___VERTER__ctx.name }} /></template>"`
          );
        });

        // it.only("bind arrow function with returnX ", () => {
        //   const source = `<div :onName="()=>{ return name }" />`;

        //   const parsed = doParseContent(source);
        //   const { magicString } = process(parsed);

        //   expect(magicString.toString()).toMatchInlineSnapshot(
        //     `"<template><div name={()=>{ return ___VERTER__ctx.name }} /></template>"`
        //   );
        // });
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
    it("should keep", () => {
      const source = `<div>
      <!-- @ts-expect-error -->
      {{ bar }}
    </div>`;
      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(`
        "<template><div>
              { /* @ts-expect-error */ }
              { ___VERTER__ctx.bar }
            </div></template>"
      `);
    });

    it("should add accessor on opening and closing tag", () => {
      const source = `<awesome-component>
        <span></span>
      </awesome-component>
      `;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(`
        "<template><___VERTER__comp.AwesomeComponent>
                <span></span>
              </___VERTER__comp.AwesomeComponent>
              </template>"
      `);
    });

    it("should add accessor on closing tag even with spaces before < ", () => {
      const source = `<awesome-component>
        <span></span>
      </   awesome-component>
      `;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(`
          "<template><___VERTER__comp.AwesomeComponent>
                  <span></span>
                </   ___VERTER__comp.AwesomeComponent>
                </template>"
        `);
    });
    it("should add accessor on closing tag even with spaces before >", () => {
      const source = `<awesome-component>
        <span></span>
      </awesome-component     >
      `;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(`
          "<template><___VERTER__comp.AwesomeComponent>
                  <span></span>
                </___VERTER__comp.AwesomeComponent     >
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

    it("support argument", () => {
      const source = `<div>{{ format(message.date, 'test', message.user.id) }}</div>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);

      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><div>{ ___VERTER__ctx.format(___VERTER__ctx.message.date, 'test', ___VERTER__ctx.message.user.id) }</div></template>"`
      );
    });

    it("array access", () => {
      const source = `<div>{{ foo[index] }}</div>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);

      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><div>{ ___VERTER__ctx.foo[___VERTER__ctx.index] }</div></template>"`
      );
    });

    it.todo("more complex", () => {
      const source = `<div v-if="!!floor || !!door" class="c3-address-floor">
      (<span v-if="!!floor">{{ $t('user.floor') }}: {{ floor }}{{ !!door ? ', ' : '' }}</span><span v-if="!!door">{{ $t('user.door') }}: {{ door }}</span>)
  </div>`;
      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot();
    });
  });

  describe("slots", () => {
    it("declaring slots", () => {
      const source = `<div><slot /><div>`;
      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(`
        "<template><div>{()=>{
        const Comp = ___VERTER_SLOT_COMP.default 
        return <Comp />
        }}<div></template>"
      `);
    });
    it("declaring slots named", () => {
      const source = `<div><slot name="foo"/><div>`;
      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(`
        "<template><div>{()=>{
        const Comp = ___VERTER_SLOT_COMP .foo
        return <Comp />
        }}<div></template>"
      `);
    });

    it("declaring slots binding name", () => {
      const source = `<div><slot :name="'test'" /><div>`;
      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(`
        "<template><div>{()=>{
        const Comp = ___VERTER_SLOT_COMP ['test']
        return <Comp  />
        }}<div></template>"
      `);
    });

    it("declaring slots binding name short", () => {
      const source = `<div><slot :name /><div>`;
      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(`
        "<template><div>{()=>{
        const Comp = ___VERTER_SLOT_COMP [___VERTER__ctx.name]
        return <Comp  />
        }}<div></template>"
      `);
    });

    it("declaring slots with attributes", () => {
      const source = `<div><slot foo="foo"/><div>`;
      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(`
        "<template><div>{()=>{
        const Comp = ___VERTER_SLOT_COMP.default 
        return <Comp foo="foo"/>
        }}<div></template>"
      `);
    });

    it("declaring slots with attributes and name", () => {
      const source = `<div><slot foo="foo" :name="bar"/><div>`;
      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(`
        "<template><div>{()=>{
        const Comp = ___VERTER_SLOT_COMP [___VERTER__ctx.bar]
        return <Comp foo="foo" />
        }}<div></template>"
      `);
    });
    it("declaring slots with children", () => {
      const source = `<div><slot foo="foo" name="foo"> <span> default </span></slot><div>`;
      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(
        `
        "<template><div>{()=>{
        const Comp = ___VERTER_SLOT_COMP .foo
        return <Comp foo="foo" > <span> default </span></Comp>
        }}<div></template>"
      `
      );
    });

    it("declaring slots with empty children", () => {
      const source = `<div><slot foo="foo" name="foo"> </slot><div>`;
      const parsed = doParseContent(source);
      const { magicString } = process(parsed);
      expect(magicString.toString()).toMatchInlineSnapshot(`
        "<template><div>{()=>{
        const Comp = ___VERTER_SLOT_COMP .foo
        return <Comp foo="foo" > </Comp>
        }}<div></template>"
      `);
    });
  });

  describe("extra", () => {
    it("should not parse as text <", () => {
      const source = `<div> < </div>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);

      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><div> < </div></template>"`
      );
    });

    it("should not parse as text <my-test-component", () => {
      const source = `<div> <my-test-component </div>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);

      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><div> <___VERTER__comp.MyTestComponent </div></template>"`
      );
    });

    it("should not parse as text <div", () => {
      const source = `<div> <div </div>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);

      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><div> <div </div></template>"`
      );
    });

    it("should not append ctx", () => {
      const source = `<input v-model="msg" :onClick="function (){ 
        console.log('hello there')
      }" />`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);

      expect(magicString.toString()).toMatchInlineSnapshot(`
        "<template><input v-model="msg" onClick={function (){ 
                console.log('hello there')
              }} /></template>"
      `);
    });

    // TODO handle attribute on partial
    it.todo("should handle attributes", () => {
      const source = `<div> <div content='test' </div>`;

      const parsed = doParseContent(source);
      const { magicString } = process(parsed);

      expect(magicString.toString()).toMatchInlineSnapshot(
        `"<template><div> <div content{'test'} </div></template>"`
      );
    });

    it.todo("should handle comments between conditions", () => {
      const source = `<div v-if="true"></div>
<!-- this should parsed -->
<div v-else></div>`;
    });
  });
});
