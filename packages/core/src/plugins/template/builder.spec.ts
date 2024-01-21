import { ElementNode } from "@vue/compiler-core";
import { build } from "./builder.js";
import { parse as sfcParse } from "@vue/compiler-sfc";
import { parse } from "./parse.js";

describe("builder template", () => {
  function doParseElement(content: string) {
    const source = `<template>${content}</template>`;
    const ast = sfcParse(source, {});
    const root = ast.descriptor.template!.ast!;

    return parse(root);
  }
  it("Hello vue", () => {
    const source = `<div>Hello vue</div>`;

    const parsed = doParseElement(source);

    const built = build(parsed);
    expect(built).toMatchInlineSnapshot(`"<div>{ "Hello vue" }</div>"`);
  });
  it("component", () => {
    const source = `<my-component/>`;

    const parsed = doParseElement(source);

    const built = build(parsed);
    expect(built).toMatchInlineSnapshot(`"<MyComponent></MyComponent>"`);
  });

  describe("props", () => {
    it("props w/:", () => {
      const source = `<my-component :foo="bar"/>`;

      const parsed = doParseElement(source);

      const built = build(parsed);
      expect(built).toMatchInlineSnapshot(
        `"<MyComponent foo={bar}></MyComponent>"`
      );
    });

    it("props w/v-bind:", () => {
      const source = `<my-component v-bind:foo="bar"/>`;

      const parsed = doParseElement(source);

      const built = build(parsed);
      expect(built).toMatchInlineSnapshot(
        `"<MyComponent foo={bar}></MyComponent>"`
      );
    });

    it("v-bind", () => {
      const source = `<my-component v-bind="bar"/>`;

      const parsed = doParseElement(source);

      const built = build(parsed);
      expect(built).toMatchInlineSnapshot(
        `"<MyComponent {...bar}></MyComponent>"`
      );
    });

    it("binding with :bind", () => {
      const source = `<my-component :bind="bar"/>`;

      const parsed = doParseElement(source);

      const built = build(parsed);
      expect(built).toMatchInlineSnapshot(
        `"<MyComponent bind={bar}></MyComponent>"`
      );
    });

    it("binding with v-bind:bind", () => {
      const source = `<my-component v-bind:bind="bar"/>`;

      const parsed = doParseElement(source);

      const built = build(parsed);
      expect(built).toMatchInlineSnapshot(
        `"<MyComponent bind={bar}></MyComponent>"`
      );
    });

    it("v-bind + props", () => {
      const source = `<my-component v-bind="bar" foo="bar"/>`;

      const parsed = doParseElement(source);

      const built = build(parsed);
      expect(built).toMatchInlineSnapshot(
        `"<MyComponent {...bar} foo="bar"></MyComponent>"`
      );
    });

    it("props + v-bind", () => {
      const source = `<my-component foo="bar" v-bind="bar"/>`;

      const parsed = doParseElement(source);
      const built = build(parsed);
      expect(built).toMatchInlineSnapshot(
        `"<MyComponent foo="bar" {...bar}></MyComponent>"`
      );
    });

    it("simple", () => {
      const source = `<span class="foo"></span>`;
      const parsed = doParseElement(source);
      const built = build(parsed);
      expect(built).toMatchInlineSnapshot(`"<span class="foo"></span>"`);
    });

    it("multiple", () => {
      const source = `<span class="foo" id="bar"></span>`;
      const parsed = doParseElement(source);
      const built = build(parsed);
      expect(built).toMatchInlineSnapshot(
        `"<span class="foo" id="bar"></span>"`
      );
    });

    it("with '", () => {
      const source = `<span class='foo'></span>`;
      const parsed = doParseElement(source);
      const built = build(parsed);
      expect(built).toMatchInlineSnapshot(`"<span class="foo"></span>"`);
    });

    describe("directive", () => {
      describe("v-for", () => {
        it("simple", () => {
          const source = `<li v-for="item in items"></li>`;

          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ renderList(items, (item) => { <li></li> }) }"`
          );
        });

        it("destructing", () => {
          const source = `<li v-for="{ message } in items"></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ renderList(items, ({ message }) => { <li></li> }) }"`
          );
        });

        it("index", () => {
          const source = `<li v-for="(item, index) in items"></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ renderList(items, (item, index) => { <li></li> }) }"`
          );
        });

        it("index + key", () => {
          const source = `<li v-for="(item, index) in items" :key="index + 'random'"></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ renderList(items, (item, index) => { <li key={index + 'random'}></li> }) }"`
          );
        });

        it("destructing + index", () => {
          const source = `<li v-for="({ message }, index) in items"></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ renderList(items, ({ message }, index) => { <li></li> }) }"`
          );
        });

        it("nested", () => {
          const source = `<li v-for="item in items">
      <span v-for="childItem in item.children"></span>
  </li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ renderList(items, (item) => { <li>{ renderList(item.children, (childItem) => { <span></span> }) }</li> }) }"`
          );
        });

        it("of", () => {
          const source = `<li v-for="item of items"></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ renderList(items, (item) => { <li></li> }) }"`
          );
        });

        // object

        it("object", () => {
          const source = `<li v-for="value in myObject"></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ renderList(myObject, (value) => { <li></li> }) }"`
          );
        });

        it("object + key", () => {
          const source = `<li v-for="(value, key) in myObject"></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ renderList(myObject, (value, key) => { <li></li> }) }"`
          );
        });

        it("object + key + index", () => {
          const source = `<li v-for="(value, key, index) in myObject"></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ renderList(myObject, (value, key, index) => { <li></li> }) }"`
          );
        });

        // range
        it("range", () => {
          const source = `<li v-for="n in 10"></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);

          expect(built).toMatchInlineSnapshot(
            `"{ renderList(10, (n) => { <li></li> }) }"`
          );
        });

        // v-if has higher priority than v-for
        it("v-if", () => {
          const source = `<li v-for="i in n" v-if="n > 5"></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);

          //   `"{ n > 5 ? renderList(n, (i) => { <li ></li> }) : undefined }"`

          expect(built).toMatchInlineSnapshot(
            `"{ (n > 5) ?  renderList(n, (i) => { <li></li> })  : undefined }"`
          );
        });
      });

      describe("conditional v-if", () => {
        it("v-if", () => {
          const source = `<li v-if="n > 5"></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ (n > 5) ? <li></li> : undefined }"`
          );
        });

        it("v-if + v-else", () => {
          const source = `<li v-if="n > 5"></li><li v-else></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ (n > 5) ? <li></li> : <li></li>  }"`
          );
        });

        it("v-if + v-else-if", () => {
          const source = `<li v-if="n > 5"></li><li v-else-if="n > 3"></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ (n > 5) ? <li></li> : (n > 3) ? <li></li> : undefined }"`
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
              <li v-else="n === 1"></li>`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"{ (n === 1) ? <li></li> : (n === 1) ? <li></li> : (n === 1) ? <li></li> : (n === 1) ? <li></li> : (n === 1) ? <li></li> : (n === 1) ? <li></li> : <li></li>  }"`
          );
        });

        describe.skip("invalid conditions", () => {
          it("v-else without v-if", () => {
            const source = `<li v-else></li>`;
            expect(() => build(doParseElement(source))).throw(
              "v-else or v-else-if must be preceded by v-if"
            );
          });

          it("v-else-if without v-if", () => {
            const source = `<li v-else-if></li>`;
            expect(() => build(doParseElement(source))).throw(
              "v-else or v-else-if must be preceded by v-if"
            );
          });
          it("v-else after v-else", () => {
            const source = `<li v-if="true"></li><li v-else></li><li v-else></li>`;
            expect(() => build(doParseElement(source))).throw(
              "v-else or v-else-if must be preceded by v-if"
            );
          });

          it("v-else-if after v-else", () => {
            const source = `<li v-if="true"></li><li v-else></li><li v-else></li>`;
            expect(() => build(doParseElement(source))).throw(
              "v-else or v-else-if must be preceded by v-if"
            );
          });
        });
      });

      describe("v-bind", () => {
        it("v-bind", () => {
          const source = `<div v-bind="props" />`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(`"<div {...props}></div>"`);
        });
        it("v-bind name", () => {
          const source = `<div v-bind:name="props" />`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(`"<div name={props}></div>"`);
        });

        it("v-bind :short", () => {
          const source = `<div :name />`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(`"<div name={name}></div>"`);
        });

        it("v-bind :shorter", () => {
          const source = `<div :name="name" />`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(`"<div name={name}></div>"`);
        });
      });

      // NOTE waiting for https://github.com/vuejs/core/pull/3399
      describe.skip("v-model", () => {
        it("v-model", () => {
          const source = `<MyComp v-model="msg" />`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"<MyComp modelValue={msg} onUpdate:modelValue={(e) => msg = e} />"`
          );
        });

        it("v-model named", () => {
          const source = `<MyComp v-model:value="msg" />`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          );
        });

        it("v-model number", () => {
          const source = `<MyComp v-model.number="msg" />`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          );
        });

        it("v-model  number + trim", () => {
          const source = `<MyComp v-model.numbe.trim="msg" />`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          );
        });

        it("v-model named + number", () => {
          const source = `<MyComp v-model:value.number="msg" />`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          );
        });

        it("v-model named + number + trim", () => {
          const source = `<MyComp v-model:value.numbe.trim="msg" />`;
          const parsed = doParseElement(source);
          const built = build(parsed);
          expect(built).toMatchInlineSnapshot(
            `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          );
        });
      });
    });
  });

  it("full example", () => {
    const source = `<template>
    <ol>
      <li v-for="item in orderedItems" :key="getKey(item)">
        {{ getLabel(item) }}
      </li>
    </ol>
  </template>
  `;
    const parsed = doParseElement(source);
    const built = build(parsed);
    expect(built).toMatchInlineSnapshot(`"<template><ol>{ renderList(orderedItems, (item) => { <li key={getKey(item)}>NOT_KNOWN_INTERPOLATION</li> }) }</ol></template>"`);
  });
});
