import { MagicString, parse } from "@vue/compiler-sfc";
import { walk, walkAttribute, walkElement, walkRoot } from "./walk.js";
import { ElementNode } from "@vue/compiler-core";

describe("walk", () => {
  function parseAndWalk(source: string) {
    const parsed = parse(`<template>${source}</template>`);
    const str = new MagicString(source);
    const res = walkRoot(parsed.descriptor.template!.ast!, str);
    return res;
  }
  describe("walkElement", () => {
    it("simple", () => {
      const source = `<span></span>`;
      const res = parseAndWalk(source);
      expect(res).toMatchInlineSnapshot(`"<span ></span>"`);
    });

    it("component", () => {
      const source = `<my-comp />`;
      const res = parseAndWalk(source);
      expect(res).toMatchInlineSnapshot(`"<MyComp ></MyComp>"`);
    });

    describe("component", () => {
      describe("camelize", () => {
        it.each([
          ["my-comp", "MyComp"],
          ["MyComp", "MyComp"],
          ["myComp", "myComp"],
          ["Mycomp", "Mycomp"],
          ["my_comp", "my_comp"],
          ["my-Comp", "MyComp"],
          // NOTE maybe this is wrong!
          ["my--comp", "My-Comp"],
        ])("%s => %s", (input, expected) => {
          const source = `<${input} />`;
          const res = parseAndWalk(source);
          expect(res).toBe(`<${expected} ></${expected}>`);
        });
      });
      it("camelize", () => {
        const source = `<my-comp />`;
        const res = parseAndWalk(source);
        expect(res).toMatchInlineSnapshot(`"<MyComp ></MyComp>"`);
      });

      it("v-bind", () => {
        const source = `<my-comp v-bind="props" />`;
        const res = parseAndWalk(source);
        expect(res).toMatchInlineSnapshot(`"<MyComp {...props}></MyComp>"`);
      });

      it("props + v-bind", () => {
        const source = `<my-comp :props="props" v-bind="props" />`;
        const res = parseAndWalk(source);
        expect(res).toMatchInlineSnapshot(
          `"<MyComp props={props} {...props}></MyComp>"`
        );
      });

      it("v-bind + props", () => {
        const source = `<my-comp v-bind="props" :props="props"/>`;
        const res = parseAndWalk(source);
        expect(res).toMatchInlineSnapshot(
          `"<MyComp {...props} props={props}></MyComp>"`
        );
      });

      // describe("slots", () => {
      //   it("default slot", () => {
      //     const source = `<my-comp><span>foo</span></my-comp>`;
      //     const res = parseAndWalk(source);
      //     expect(res).toMatchInlineSnapshot(
      //       `"<MyComp ><span>foo</span></MyComp>"`
      //     );
      //   });

      //   it("slots with name", () => {
      //     const source = `<my-comp><template #foo><span>foo</span></template></my-comp>`;
      //     const res = parseAndWalk(source);
      //     expect(res).toMatchInlineSnapshot(
      //       `"<MyComp ><template slot='foo'><span>foo</span></template></MyComp>"`
      //     );
      //   });
      // });
    });

    describe("templates", () => {
      it("simple template", () => {
        const source = `<template><span>foo</span></template>`;
        const res = parseAndWalk(source);
        expect(res).toMatchInlineSnapshot(`"<span >foo</span>"`);
      });

      it('template with "v-if"', () => {
        const source = `<template v-if="true"><span>foo</span></template>`;
        const res = parseAndWalk(source);
        expect(res).toMatchInlineSnapshot(
          `"{ true ? <span >foo</span> : undefined }"`
        );
      });

      it("template with multiple", () => {
        const source = `<template><span>foo</span><span>bar</span></template>`;
        const res = parseAndWalk(source);
        expect(res).toMatchInlineSnapshot(`
          "[<span >foo</span>,
          <span >bar</span>]"
        `);
      });

      it("template multiple v-if", () => {
        const source = `<template v-if="true"><span>foo</span><span>foo2</span></template><template v-if="false"><span>bar</span></template>`;
        const res = parseAndWalk(source);
        expect(res).toMatchInlineSnapshot(`
          "{ true ? [<span >foo</span>,
          <span >foo2</span>] : undefined }
          { false ? <span >bar</span> : undefined }"
        `);
      });

      it('template with v-if + "v-else-if"', () => {
        const source = `<template v-if="true"><span>foo</span></template><template v-else-if="false"><span>bar</span></template>`;
        const res = parseAndWalk(source);
        expect(res).toMatchInlineSnapshot(`
          "{ true ? <span >foo</span> 
          : false ? <span >bar</span> : undefined }"
        `);
      });
    });
  });

  describe("walkAttribute", () => {
    describe("attribute", () => {
      it("simple", () => {
        const source = `<span class="foo"></span>`;
        const res = parseAndWalk(source);
        expect(res).toMatchInlineSnapshot(`"<span class="foo"></span>"`);
      });

      it("multiple", () => {
        const source = `<span class="foo" id="bar"></span>`;
        const res = parseAndWalk(source);
        expect(res).toMatchInlineSnapshot(
          `"<span class="foo" id="bar"></span>"`
        );
      });

      it("with '", () => {
        const source = `<span class='foo'></span>`;
        const res = parseAndWalk(source);
        expect(res).toMatchInlineSnapshot(`"<span class="foo"></span>"`);
      });
    });

    describe("directive", () => {
      describe("v-for", () => {
        it("simple", () => {
          const source = `<li v-for="item in items"></li>`;

          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"{ renderList(items, (item) => { <li ></li> }) }"`
          );
        });

        it("destructing", () => {
          const source = `<li v-for="{ message } in items"></li>`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"{ renderList(items, ({ message }) => { <li ></li> }) }"`
          );
        });

        it("index", () => {
          const source = `<li v-for="(item, index) in items"></li>`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"{ renderList(items, (item, index) => { <li ></li> }) }"`
          );
        });

        it("index + key", () => {
          const source = `<li v-for="(item, index) in items" :key="index + 'random'"></li>`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"{ renderList(items, (item, index) => { <li key={index + 'random'}></li> }) }"`
          );
        });

        it("destructing + index", () => {
          const source = `<li v-for="({ message }, index) in items"></li>`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"{ renderList(items, ({ message }, index) => { <li ></li> }) }"`
          );
        });

        it("nested", () => {
          const source = `<li v-for="item in items">
    <span v-for="childItem in item.children"></span>
</li>`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"{ renderList(items, (item) => { <li >{ renderList(item.children, (childItem) => { <span ></span> }) }</li> }) }"`
          );
        });

        it("of", () => {
          const source = `<li v-for="item of items"></li>`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"{ renderList(items, (item) => { <li ></li> }) }"`
          );
        });

        // object

        it("object", () => {
          const source = `<li v-for="value in myObject"></li>`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"{ renderList(myObject, (value) => { <li ></li> }) }"`
          );
        });

        it("object + key", () => {
          const source = `<li v-for="(value, key) in myObject"></li>`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"{ renderList(myObject, (value, key) => { <li ></li> }) }"`
          );
        });

        it("object + key + index", () => {
          const source = `<li v-for="(value, key, index) in myObject"></li>`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"{ renderList(myObject, (value, key, index) => { <li ></li> }) }"`
          );
        });

        // range
        it("range", () => {
          const source = `<li v-for="n in 10"></li>`;
          const res = parseAndWalk(source);

          expect(res).toMatchInlineSnapshot(
            `"{ renderList(10, (n) => { <li ></li> }) }"`
          );
        });

        // v-if has higher priority than v-for
        it("v-if", () => {
          const source = `<li v-for="i in n" v-if="n > 5"></li>`;
          const res = parseAndWalk(source);

          expect(res).toMatchInlineSnapshot(
            `"{ n > 5 ? renderList(n, (i) => { <li ></li> }) : undefined }"`
          );
        });
      });

      describe("conditional v-if", () => {
        it("v-if", () => {
          const source = `<li v-if="n > 5"></li>`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"{ n > 5 ? <li ></li> : undefined }"`
          );
        });

        it("v-if + v-else", () => {
          const source = `<li v-if="n > 5"></li><li v-else></li>`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(`
            "{ n > 5 ? <li ></li> 
            : <li ></li> }"
          `);
        });

        it("v-if + v-else-if", () => {
          const source = `<li v-if="n > 5"></li><li v-else-if="n > 3"></li>`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(`
            "{ n > 5 ? <li ></li> 
            : n > 3 ? <li ></li> : undefined }"
          `);
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
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(`
            "{ n === 1 ? <li ></li> 
            : n === 1 ? <li ></li> 
            : n === 1 ? <li ></li> 
            : n === 1 ? <li ></li> 
            : n === 1 ? <li ></li> 
            : n === 1 ? <li ></li> 
            : <li ></li> }"
          `);
        });

        describe("invalid conditions", () => {
          it("v-else without v-if", () => {
            const source = `<li v-else></li>`;
            expect(() => parseAndWalk(source)).throw(
              "v-else or v-else-if must be preceded by v-if"
            );
          });

          it("v-else-if without v-if", () => {
            const source = `<li v-else-if></li>`;
            expect(() => parseAndWalk(source)).throw(
              "v-else or v-else-if must be preceded by v-if"
            );
          });
          it("v-else after v-else", () => {
            const source = `<li v-if="true"></li><li v-else></li><li v-else></li>`;
            expect(() => parseAndWalk(source)).throw(
              "v-else or v-else-if must be preceded by v-if"
            );
          });

          it("v-else-if after v-else", () => {
            const source = `<li v-if="true"></li><li v-else></li><li v-else></li>`;
            expect(() => parseAndWalk(source)).throw(
              "v-else or v-else-if must be preceded by v-if"
            );
          });
        });
      });

      describe("v-bind", () => {
        it("v-bind", () => {
          const source = `<div v-bind="props" />`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(`"<div {...props}></div>"`);
        });
        it("v-bind name", () => {
          const source = `<div v-bind:name="props" />`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(`"<div name={props}></div>"`);
        });

        it("v-bind :short", () => {
          const source = `<div :name />`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(`"<div name={name}></div>"`);
        });

        it("v-bind :shorter", () => {
          const source = `<div :name="name" />`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(`"<div name={name}></div>"`);
        });
      });

      // NOTE waiting for https://github.com/vuejs/core/pull/3399
      describe.skip("v-model", () => {
        it("v-model", () => {
          const source = `<MyComp v-model="msg" />`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"<MyComp modelValue={msg} onUpdate:modelValue={(e) => msg = e} />"`
          );
        });

        it("v-model named", () => {
          const source = `<MyComp v-model:value="msg" />`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          );
        });

        it("v-model number", () => {
          const source = `<MyComp v-model.number="msg" />`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          );
        });

        it("v-model  number + trim", () => {
          const source = `<MyComp v-model.numbe.trim="msg" />`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          );
        });

        it("v-model named + number", () => {
          const source = `<MyComp v-model:value.number="msg" />`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          );
        });

        it("v-model named + number + trim", () => {
          const source = `<MyComp v-model:value.numbe.trim="msg" />`;
          const res = parseAndWalk(source);
          expect(res).toMatchInlineSnapshot(
            `"<MyComp value={msg} onUpdate:value={(e) => msg = e} />"`
          );
        });
      });
    });
  });
});
