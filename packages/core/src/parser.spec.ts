import { createContext, parseGeneric } from "./parser";

describe("parser", () => {
  describe("createContext", () => {
    it("should create context", () => {
      const context = createContext(
        `<template><div></div></template><script></script>`
      );
      expect(context).toMatchObject({
        filename: "temp.vue",
        isSetup: false,
        isAsync: false,
        generic: undefined,
      });
    });

    it("should add empty script", () => {
      const context = createContext(`<template><div></div></template>`);
      expect(context.sfc.script).toMatchObject({
        content: "",
      });
      expect(context.s.toString()).toContain("<script></script>");
    });

    it("should resolve the generic", () => {
      const context = createContext(`<script generic="T"></script>`);
      expect(context.generic).toMatchObject({
        source: "T",
        names: ["T"],
        sanitisedNames: ["___VERTER___TS_T"],
        declaration: "___VERTER___TS_T = any",
      });
    });

    it("should resolve the script setup", () => {
      const context = createContext(`<script setup></script>`);
      expect(context.isSetup).toBe(true);
    });

    it("should handle multiple scripts", () => {
      const context = createContext(`<script></script><script setup></script>`);

      expect(context.isSetup).toBe(true);
    });

    it.todo("should pass options to the parser", () => {});

    it("should be async", () => {
      const context = createContext(
        `<script setup>await Promise.resolve()</script>`
      );
      expect(context.isAsync).toBe(true);
    });

    describe("templateIdentifiers", () => {
      it("should return empty array if no identifiers", () => {
        const context = createContext(`<template></template>`);
        expect(context.templateIdentifiers).toEqual([]);
      });

      it("should return empty array if no template", () => {
        const context = createContext(`<script></script>`);
        expect(context.templateIdentifiers).toEqual([]);
      });

      describe("interpolation", () => {
        test("simple", () => {
          const source = `<template><div>{{ foo }}</div></template>`;
          const context = createContext(source);

          const identifier = context.templateIdentifiers[0];
          expect(source.slice(identifier.loc.start, identifier.loc.end)).toBe(
            "foo"
          );

          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 18,
                end: 21,
              },
              content: "foo",

              node: {
                content: "foo",
              },
            },
          ]);
        });

        test("simple + accessor", () => {
          const source = `<template><div>{{ foo.bar }}</div></template>`;
          const context = createContext(source);

          const identifier = context.templateIdentifiers[0];
          expect(source.slice(identifier.loc.start, identifier.loc.end)).toBe(
            "foo"
          );
          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 18,
                end: 21,
              },
              content: "foo",
              node: {
                content: "foo.bar",
              },
            },
          ]);
        });

        test("simple + deeper accessor", () => {
          const source = `<template><div>{{ foo.bar.baz }}</div></template>`;
          const context = createContext(source);

          const identifier = context.templateIdentifiers[0];
          expect(source.slice(identifier.loc.start, identifier.loc.end)).toBe(
            "foo"
          );

          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 18,
                end: 21,
              },
              content: "foo",
              node: {
                content: "foo.bar.baz",
              },
            },
          ]);
        });

        test("with calculation", () => {
          const source = `<template><div>{{ foo + 1 }}</div></template>`;
          const context = createContext(source);

          const identifier = context.templateIdentifiers[0];
          expect(source.slice(identifier.loc.start, identifier.loc.end)).toBe(
            "foo"
          );
          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 18,
                end: 21,
              },
              content: "foo",
              node: {
                content: "foo + 1",
              },
            },
          ]);
        });

        test("with calculation and accessor", () => {
          const source = `<template><div>{{ foo.bar + bar.x }}</div></template>`;
          const context = createContext(source);

          const identifierMatch = ["foo", "bar"];

          const identifiers = context.templateIdentifiers.map((i) =>
            source.slice(i.loc.start, i.loc.end)
          );

          expect(identifiers).toEqual(identifierMatch);

          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 18,
                end: 21,
              },
              content: "foo",
              node: {
                content: "foo.bar + bar.x",
              },
            },
            {
              loc: {
                start: 28,
                end: 31,
              },
              content: "bar",
              node: {
                content: "foo.bar + bar.x",
              },
            },
          ]);
        });

        test("static attribute", () => {
          const source = `<template><div class="foo"></div></template>`;
          const context = createContext(source);

          expect(context.templateIdentifiers).toHaveLength(0);
        });

        test("attribute", () => {
          const source = `<template><div :class="foo"></div></template>`;
          const context = createContext(source);

          const identifierMatch = ["foo"];

          const identifiers = context.templateIdentifiers.map((i) =>
            source.slice(i.loc.start, i.loc.end)
          );
          expect(identifiers).toEqual(identifierMatch);

          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 23,
                end: 26,
              },
              content: "foo",
              node: {
                content: "foo",
              },
            },
          ]);
        });

        test("dynamic attribute", () => {
          const source = `<template><div :[class]="foo"></div></template>`;
          const context = createContext(source);

          const identifierMatch = ["class", "foo"];

          const identifiers = context.templateIdentifiers.map((i) =>
            source.slice(i.loc.start, i.loc.end)
          );
          expect(identifiers).toEqual(identifierMatch);
          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 17,
                end: 22,
              },
              content: "class",
              node: {
                content: "class",
              },
            },
            {
              loc: {
                start: 25,
                end: 28,
              },
              content: "foo",
              node: {
                content: "foo",
              },
            },
          ]);
        });

        test("attribute complex", () => {
          const source = '<template><div :class="foo + bar"></div></template>';
          const context = createContext(source);

          const identifierMatch = ["foo", "bar"];

          const identifiers = context.templateIdentifiers.map((i) =>
            source.slice(i.loc.start, i.loc.end)
          );
          expect(identifiers).toEqual(identifierMatch);
          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 23,
                end: 26,
              },
              content: "foo",
              node: {
                content: "foo + bar",
              },
            },
            {
              loc: {
                start: 29,
                end: 32,
              },
              content: "bar",
              node: {
                content: "foo + bar",
              },
            },
          ]);
        });

        test("attribute function call", () => {
          const source = '<template><div :class="foo()"></div></template>';
          const context = createContext(source);

          const identifierMatch = ["foo"];

          const identifiers = context.templateIdentifiers.map((i) =>
            source.slice(i.loc.start, i.loc.end)
          );
          expect(identifiers).toEqual(identifierMatch);
          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 23,
                end: 26,
              },
              content: "foo",
              node: {
                content: "foo()",
              },
            },
          ]);
        });

        test("attribute function call with arguments", () => {
          const source = '<template><div :class="foo(bar)"></div></template>';
          const context = createContext(source);

          const identifierMatch = ["foo", "bar"];

          const identifiers = context.templateIdentifiers.map((i) =>
            source.slice(i.loc.start, i.loc.end)
          );
          expect(identifiers).toEqual(identifierMatch);
          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 23,
                end: 26,
              },
              content: "foo",
              node: {
                content: "foo(bar)",
              },
            },
            {
              loc: {
                start: 27,
                end: 30,
              },
              content: "bar",
              node: {
                content: "foo(bar)",
              },
            },
          ]);
        });

        test("attribute function with body", () => {
          const source =
            '<template><div :class="foo(bar => bar + 1)"></div></template>';
          const context = createContext(source);

          const identifierMatch = ["foo"];

          const identifiers = context.templateIdentifiers.map((i) =>
            source.slice(i.loc.start, i.loc.end)
          );
          expect(identifiers).toEqual(identifierMatch);
          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 23,
                end: 26,
              },
              content: "foo",
              node: {
                content: "foo(bar => bar + 1)",
              },
            },
          ]);
        });
        test("function $event", () => {
          const source =
            '<template><div @click="foo($event)"></div></template>';
          const context = createContext(source);
          const identifierMatch = ["foo"];

          const identifiers = context.templateIdentifiers.map((i) =>
            source.slice(i.loc.start, i.loc.end)
          );
          expect(identifiers).toEqual(identifierMatch);
          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 23,
                end: 26,
              },
              content: "foo",
              node: {
                content: "foo($event)",
              },
            },
          ]);
        });

        test("v-if", () => {
          const source = '<template><div v-if="foo"></div></template>';
          const context = createContext(source);
          const identifierMatch = ["foo"];

          const identifiers = context.templateIdentifiers.map((i) =>
            source.slice(i.loc.start, i.loc.end)
          );
          expect(identifiers).toEqual(identifierMatch);
          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 21,
                end: 24,
              },
              content: "foo",
              node: {
                content: "foo",
              },
            },
          ]);
        });

        test('v-for="item in foo"', () => {
          const source =
            '<template><div v-for="item in foo"> {{ item.bar }} </div></template>';
          const context = createContext(source);
          const identifierMatch = ["foo"];

          const identifiers = context.templateIdentifiers.map((i) =>
            source.slice(i.loc.start, i.loc.end)
          );
          expect(identifiers).toEqual(identifierMatch);
          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 30,
                end: 33,
              },
              content: "foo",
              node: {
                content: "foo",
              },
            },
          ]);
        });
        test('v-for="(item, index) in foo"', () => {
          const source =
            '<template><div v-for="(item, index) in foo"> {{ item.bar + index }} </div></template>';
          const context = createContext(source);
          const identifierMatch = ["foo"];

          const identifiers = context.templateIdentifiers.map((i) =>
            source.slice(i.loc.start, i.loc.end)
          );
          expect(identifiers).toEqual(identifierMatch);
          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 39,
                end: 42,
              },
              content: "foo",
              node: {
                content: "foo",
              },
            },
          ]);
        });

        test("v-slot", () => {
          const source =
            '<template><MyComponent v-slot="{ foo }">{{ foo.bar }}</MyComponent></template>';
          const context = createContext(source);

          expect(
            context.templateIdentifiers.filter((x) => x.type === "binding")
          ).toHaveLength(0);
        });
      });

      describe("components", () => {
        test("component", () => {
          const source = "<template><MyComponent  /></template>";
          const context = createContext(source);
          const identifierMatch = ["MyComponent"];

          const identifiers = context.templateIdentifiers.map((i) =>
            source.slice(i.loc.start, i.loc.end)
          );
          expect(identifiers).toEqual(identifierMatch);
          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 11,
                end: 22,
              },
              content: "MyComponent",
              node: {
                tag: "MyComponent",
              },
            },
          ]);
        });
        test("component with v-slot", () => {
          const source =
            '<template><MyComponent v-slot="{ foo }">{{ foo.bar }}</MyComponent></template>';
          const context = createContext(source);
          const identifierMatch = ["MyComponent"];

          const identifiers = context.templateIdentifiers.map((i) =>
            source.slice(i.loc.start, i.loc.end)
          );
          expect(identifiers).toEqual(identifierMatch);
          expect(context.templateIdentifiers).toMatchObject([
            {
              loc: {
                start: 11,
                end: 22,
              },
              content: "MyComponent",
              node: {
                tag: "MyComponent",
              },
            },
          ]);
        });
      });
    });
  });

  describe("parseGeneric", () => {
    it("should resolve generic", () => {
      expect(parseGeneric('T extends "foo" | "bar"', "__TS__")).toMatchObject({
        source: 'T extends "foo" | "bar"',
        names: ["T"],

        sanitisedNames: ["__TS__T"],

        declaration: '__TS__T extends "foo" | "bar" = any',
      });
    });

    it("should be undefined on empty string", () => {
      expect(parseGeneric("", "__TS__")).toBeUndefined();
    });

    it("undefinde if attribute is not a string", () => {
      expect(parseGeneric(true, "__TS__")).toBeUndefined();
    });
  });
});
