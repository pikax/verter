import { ElementNode, NodeTypes } from "@vue/compiler-core";
import {
  parse as parseSFC,
  compileTemplate,
  extractRuntimeProps,
} from "@vue/compiler-sfc";
import { handleProps, PropsContext } from "./index.js";
import { TemplateTypes } from "../../types.js";

describe("parser template element props", () => {
  function parse(
    content: string,
    context: PropsContext = { ignoredIdentifiers: [] }
  ) {
    const source = `<template>${content}</template>`;

    const sfc = parseSFC(source, {});

    const template = sfc.descriptor.template;
    const ast = template?.ast!;

    const result = handleProps(ast.children[0] as any, context);

    return {
      source,
      result,
    };
  }

  function expectMapping(result: any, source: string) {
    for (const p of result) {
      expect(source.slice(p.node.loc.start.offset, p.node.loc.end.offset)).toBe(
        p.node.loc.source
      );
    }
  }

  it("return null if not element", () => {
    const node = {
      type: NodeTypes.COMMENT,
    } as any as ElementNode;

    // @ts-expect-error
    expect(parse(node, {}).result).toBeNull();
  });

  it("return [] if no props", () => {
    const { result } = parse(`<div />`);

    expect(result).toHaveLength(0);
  });

  describe("attributes", () => {
    it("simple", () => {
      const { result, source } = parse(`<div prop="value" />`);

      expect(result).toMatchObject([
        {
          type: TemplateTypes.Prop,
          name: "prop",
          value: "value",
          node: {
            type: NodeTypes.ATTRIBUTE,
          },
        },
      ]);

      expectMapping(result, source);
    });

    it("boolean", () => {
      const { result, source } = parse(`<div prop />`);

      expect(result).toMatchObject([
        {
          type: TemplateTypes.Prop,
          name: "prop",
          value: null,
          node: {
            type: NodeTypes.ATTRIBUTE,
          },
        },
      ]);
      expectMapping(result, source);
    });

    it("empty", () => {
      const { result, source } = parse(`<div prop="" />`);

      expect(result).toMatchObject([
        {
          type: TemplateTypes.Prop,
          name: "prop",
          value: "",
          node: {
            type: NodeTypes.ATTRIBUTE,
          },
        },
      ]);
      expectMapping(result, source);
    });

    it("multiple", () => {
      const { result, source } = parse(`<div prop="value" foo="bar" baz />`);

      expect(result).toMatchObject([
        {
          type: TemplateTypes.Prop,
          name: "prop",
          value: "value",
          node: {
            type: NodeTypes.ATTRIBUTE,
          },
        },
        {
          type: TemplateTypes.Prop,
          name: "foo",
          value: "bar",
          node: {
            type: NodeTypes.ATTRIBUTE,
          },
        },
        {
          type: TemplateTypes.Prop,
          name: "baz",
          value: null,
          node: {
            type: NodeTypes.ATTRIBUTE,
          },
        },
      ]);
      expectMapping(result, source);
    });

    it("kebab-case", () => {
      const { result, source } = parse(`<div kebab-case="value" />`);

      expect(result).toMatchObject([
        {
          type: TemplateTypes.Prop,
          name: "kebab-case",
          value: "value",
          node: {
            type: NodeTypes.ATTRIBUTE,
          },
        },
      ]);
      expectMapping(result, source);
    });

    it("slot named prop", () => {
      const { result, source } = parse(`<div slot="name" />`);

      expect(result).toMatchObject([
        {
          type: TemplateTypes.Prop,
          name: "slot",
          value: "name",
          node: {
            type: NodeTypes.ATTRIBUTE,
          },
        },
      ]);
      expectMapping(result, source);
    });
  });

  describe("directives", () => {
    describe("bind", () => {
      it("props w/:", () => {
        const { result, source } = parse(`<span :foo="bar"/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "foo",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],

            node: {
              name: "bind",
              rawName: ":foo",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "foo",
            ignore: true,
          },

          {
            type: TemplateTypes.Binding,
            name: "bar",
            ignore: false,
          },
        ]);
        expectMapping(result, source);
      });

      it("props w/v-bind:", () => {
        const { result, source } = parse(`<span v-bind:foo="bar"/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "foo",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],

            node: {
              name: "bind",
              rawName: "v-bind:foo",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "foo",
            ignore: true,
          },

          {
            type: TemplateTypes.Binding,
            name: "bar",
            ignore: false,
          },
        ]);
        expectMapping(result, source);
      });

      it("v-bind", () => {
        const { result, source } = parse(`<span v-bind="bar"/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: null,
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],
            node: {
              name: "bind",
              rawName: "v-bind",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "bar",
            ignore: false,
          },
        ]);
        expectMapping(result, source);
      });

      it('v-bind:[foo]="bar"', () => {
        const { result, source } = parse(`<span v-bind:[foo]="bar"/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "foo",
                ignore: false,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],

            node: {
              name: "bind",
              rawName: "v-bind:[foo]",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "foo",
            ignore: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "bar",
            ignore: false,
          },
        ]);
        expectMapping(result, source);
      });
      it(':[foo]="bar"', () => {
        const { result, source } = parse(`<span :[foo]="bar"/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "foo",
                ignore: false,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],

            node: {
              name: "bind",
              rawName: ":[foo]",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "foo",
            ignore: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "bar",
            ignore: false,
          },
        ]);
        expectMapping(result, source);
      });

      it("binding with :bind", () => {
        const { result, source } = parse(`<span :bind="bar"/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "bind",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],

            node: {
              name: "bind",
              rawName: ":bind",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "bind",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "bar",
            ignore: false,
          },
        ]);

        expectMapping(result, source);
      });
      it("binding multi-line", () => {
        const { result, source } = parse(`<span :bind="
            i == 1 ? false : true
            "/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "bind",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "i",
                ignore: false,
              },
            ],

            node: {
              name: "bind",
              rawName: ":bind",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "bind",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "i",
            ignore: false,
          },
          {
            type: TemplateTypes.Literal,
            content: "1",
            value: 1,
          },
          {
            type: TemplateTypes.Literal,
            content: "false",
            value: false,
          },
          {
            type: TemplateTypes.Literal,
            content: "true",
            value: true,
          },
        ]);

        expectMapping(result, source);
      });

      it("binding boolean", () => {
        const { result, source } = parse(`<span :bind="false"/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "bind",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "false",
                ignore: false,
              },
            ],

            node: {
              name: "bind",
              rawName: ":bind",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "bind",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "false",
            ignore: false,
          },
        ]);

        expectMapping(result, source);
      });

      it("binding with v-bind:bind", () => {
        const { result, source } = parse(`<span v-bind:bind="bar"/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "bind",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],

            node: {
              name: "bind",
              rawName: "v-bind:bind",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "bind",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "bar",
            ignore: false,
          },
        ]);

        expectMapping(result, source);
      });

      it('shorthand :foo', ()=>{
        const { result, source } = parse(`<span :foo/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "foo",
                ignore: false,
              },
            ],
            // exp: null,

            node: {
              name: "bind",
              rawName: ":foo",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "foo",
            ignore: false,
          },
        ]);

        expectMapping(result, source);
      })

      it("v-bind + props", () => {
        const { result, source } = parse(`<span v-bind="bar" foo="bar"/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: null,
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],

            node: {
              name: "bind",
              rawName: "v-bind",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "bar",
            ignore: false,
          },
          {
            type: TemplateTypes.Prop,
            name: "foo",
            value: "bar",
            node: {
              type: NodeTypes.ATTRIBUTE,
            },
          },
        ]);

        expectMapping(result, source);
      });

      it("props + v-bind", () => {
        const { result, source } = parse(`<span foo="bar" v-bind="bar"/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            name: "foo",
            value: "bar",
            node: {
              type: NodeTypes.ATTRIBUTE,
            },
          },
          {
            type: TemplateTypes.Prop,
            arg: null,
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],

            node: {
              name: "bind",
              rawName: "v-bind",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "bar",
            ignore: false,
          },
        ]);

        expectMapping(result, source);
      });

      it("props + binding on array", () => {
        const { result, source } = parse(`<span :foo="[bar]" />`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "foo",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],

            node: {
              name: "bind",
              rawName: ":foo",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "foo",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "bar",
            ignore: false,
          },
        ]);

        expectMapping(result, source);
      });

      it("props + binding complex", () => {
        const { result, source } = parse(
          `<span :foo="isFoo ? { myFoo: foo } : undefined" />`
        );

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "foo",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "isFoo",
                ignore: false,
              },
              {
                type: TemplateTypes.Binding,
                name: "foo",
                ignore: false,
              },
              {
                type: TemplateTypes.Binding,
                name: "undefined",
                ignore: true,
              },
            ],

            node: {
              name: "bind",
              rawName: ":foo",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "foo",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "isFoo",
            ignore: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "foo",
            ignore: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "undefined",
            ignore: true,
          },
        ]);

        expectMapping(result, source);
      });

      it("should keep casing on binding", () => {
        const { result, source } = parse(`<span :aria-autocomplete="'bar'"/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "aria-autocomplete",
                ignore: true,
              },
            ],
            exp: [],

            node: {
              name: "bind",
              rawName: ":aria-autocomplete",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "aria-autocomplete",
            ignore: true,
          },
          {
            type: TemplateTypes.Literal,
            value: "bar",
            content: "bar",
          },
        ]);

        expectMapping(result, source);
      });

      it("v-bind : without name", () => {
        const { result, source } = parse(`<div : />`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: null,
            exp: null,
          },
        ]);

        expectMapping(result, source);
      });

      it("v-bind :short", () => {
        const { result, source } = parse(`<div :name />`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: false,
              },
            ],
            exp: null,
            node: {
              name: "bind",
              rawName: ":name",
            },
            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: false,
          },
        ]);

        expectMapping(result, source);
      });
      it("v-bind :short camelise", () => {
        const { result, source } = parse(`<MyComp :foo-bar />`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "foo-bar",
                ignore: false,
              },
            ],
            exp: null,
            node: {
              name: "bind",
              rawName: ":foo-bar",
            },
            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "foo-bar",
            ignore: false,
          },
        ]);

        expectMapping(result, source);
      });

      it("v-bind :shorter", () => {
        const { result, source } = parse(`<div :name="name" />`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: false,
              },
            ],
            node: {
              name: "bind",
              rawName: ":name",
            },
            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: false,
          },
        ]);

        expectMapping(result, source);
      });

      it("bind arrow function", () => {
        const { result, source } = parse(`<div :name="()=>name" />`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: false,
              },
            ],
            node: {
              name: "bind",
              rawName: ":name",
            },
            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: true,
          },
          {
            type: TemplateTypes.Function,
            node: {
              type: "ArrowFunctionExpression",
            },
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: false,
          },
        ]);

        expectMapping(result, source);
      });

      it("bind arrow function with return ", () => {
        const { result, source } = parse(`<div :name="()=>{ return name }" />`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: false,
              },
            ],
            node: {
              name: "bind",
              rawName: ":name",
            },
            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: true,
          },
          {
            type: TemplateTypes.Function,
            node: {
              type: "ArrowFunctionExpression",
            },
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: false,
          },
        ]);

        expectMapping(result, source);
      });

      it("bind with ?.", () => {
        const { result, source } = parse(`<div :name="test?.random" />`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "test",
                ignore: false,
              },
            ],
            node: {
              name: "bind",
              rawName: ":name",
            },
            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "test",
            ignore: false,
          },
        ]);

        expectMapping(result, source);
      });

      it("should append ctx inside of functions", () => {
        const { result, source } = parse(
          `<span :check-for-something="e=> { foo = e }"></span>`
        );

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "check-for-something",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "e",
                ignore: true,
              },
              {
                type: TemplateTypes.Binding,
                name: "foo",
                ignore: false,
              },
              {
                type: TemplateTypes.Binding,
                name: "e",
                ignore: true,
              },
            ],
            node: {
              name: "bind",
              rawName: ":check-for-something",
            },
            static: false,
            context: {
              ignoredIdentifiers: [],
            },
          },
          {
            type: TemplateTypes.Binding,
            name: "check-for-something",
            ignore: true,
          },
          {
            type: TemplateTypes.Function,
            node: {
              type: "ArrowFunctionExpression",
            },
          },
          {
            type: TemplateTypes.Binding,
            name: "e",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "foo",
            ignore: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "e",
            ignore: true,
          },
        ]);

        expectMapping(result, source);
      });

      it("should  append ctx inside a string interpolation", () => {
        const { result, source } = parse(
          '<span :check-for-something="`foo=${bar}`"></span>'
        );

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "check-for-something",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],
            node: {
              name: "bind",
              rawName: ":check-for-something",
            },
            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "check-for-something",
            ignore: true,
          },
          {
            type: TemplateTypes.Literal,
            content: "",
            value: undefined,
            node: {
              type: "TemplateLiteral",
            },
          },
          {
            type: TemplateTypes.Binding,
            name: "bar",
            ignore: false,
          },
        ]);

        expectMapping(result, source);
      });

      it("should ignore if is ignoredIdentifier", () => {
        const { result, source } = parse(`<span :item="item"/>`, {
          ignoredIdentifiers: ["item"],
        });

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "item",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "item",
                ignore: true,
              },
            ],
            node: {
              name: "bind",
              rawName: ":item",
            },
            static: false,
          },

          {
            type: TemplateTypes.Binding,
            name: "item",
            ignore: true,
          },

          {
            type: TemplateTypes.Binding,
            name: "item",
            ignore: true,
          },
        ]);

        expectMapping(result, source);
      });
      it("should not append if is ignoredIdentifier followed by a dot", () => {
        const { result, source } = parse(`<span :item="item."/>`, {
          ignoredIdentifiers: ["item"],
        });

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "item",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "item",
                ignore: true,
              },
            ],
            node: {
              name: "bind",
              rawName: ":item",
            },
            static: false,
          },

          {
            type: TemplateTypes.Binding,
            name: "item",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "item",
            ignore: true,
          },
        ]);

        expectMapping(result, source);
      });

      // editor
      it("partial as", () => {
        const { result, source } = parse(`<span :item="item as "/>`, {
          ignoredIdentifiers: ["as"],
        });
        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "item",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "item",
                ignore: false,
              },
              {
                type: TemplateTypes.Binding,
                name: "as",
                ignore: true,
              },
            ],
            node: {
              name: "bind",
              rawName: ":item",
            },
            static: false,
          },

          {
            type: TemplateTypes.Binding,
            name: "item",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "item",
            ignore: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "as",
            ignore: true,
          },
        ]);
      });
    });

    describe("v-on", () => {
      it("@back='bar'", () => {
        const { result, source } = parse(`<span @back="bar"/>`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "back",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],

            node: {
              name: "on",
              rawName: "@back",
            },

            static: false,
          },
          {
            type: TemplateTypes.Binding,
            name: "back",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "bar",
            ignore: false,
          },
        ]);
      });
    });

    it("v-once", () => {
      const { result } = parse(`<span v-once />`);

      expect(result).toMatchObject([
        {
          type: TemplateTypes.Prop,

          name: "once",
          exp: null,
          arg: null,

          context: {
            ignoredIdentifiers: [],
          },
        },
      ]);
    });

    it("v-pre", () => {
      const { result } = parse(`<span v-pre>{{ test }}</span>`);

      expect(result).toMatchObject([
        {
          type: TemplateTypes.Prop,

          name: "pre",
          exp: null,
          arg: null,

          context: {
            ignoredIdentifiers: [],
          },
        },
      ]);
    });

    it("v-cloak", () => {
      const { result } = parse(`<span v-cloak />`);
      expect(result).toMatchObject([
        {
          type: TemplateTypes.Prop,

          name: "cloak",
          exp: null,
          arg: null,
          context: {
            ignoredIdentifiers: [],
          },
        },
      ]);
    });

    it("v-model", () => {
      const { result } = parse(`<span v-model="foo" />`);
      expect(result).toMatchObject([
        {
          type: TemplateTypes.Directive,

          name: "model",
          arg: null,
          exp: [
            {
              type: TemplateTypes.Binding,
              name: "foo",
              ignore: false,
            },
          ],
          context: {
            ignoredIdentifiers: [],
          },
        },

        {
          type: TemplateTypes.Binding,
          name: "foo",
          ignore: false,
        },
      ]);
    });

    it("v-show", () => {
      const { result } = parse(`<span v-show="foo" />`);

      expect(result).toMatchObject([
        {
          type: TemplateTypes.Prop,

          name: "show",
          arg: null,
          exp: [
            {
              type: TemplateTypes.Binding,
              name: "foo",
              ignore: false,
            },
          ],
          context: {
            ignoredIdentifiers: [],
          },
        },

        {
          type: TemplateTypes.Binding,
          name: "foo",
          ignore: false,
        },
      ]);
    });

    it("v-html", () => {
      const { result } = parse(`<span v-html="foo" />`);

      expect(result).toMatchObject([
        {
          type: TemplateTypes.Prop,

          name: "html",
          arg: null,
          exp: [
            {
              type: TemplateTypes.Binding,
              name: "foo",
              ignore: false,
            },
          ],
          context: {
            ignoredIdentifiers: [],
          },
        },

        {
          type: TemplateTypes.Binding,
          name: "foo",
          ignore: false,
        },
      ]);
    });

    it("v-text", () => {
      const { result } = parse(`<span v-text="foo" />`);

      expect(result).toMatchObject([
        {
          type: TemplateTypes.Prop,

          name: "text",
          arg: null,
          exp: [
            {
              type: TemplateTypes.Binding,
              name: "foo",
              ignore: false,
            },
          ],
          context: {
            ignoredIdentifiers: [],
          },
        },

        {
          type: TemplateTypes.Binding,
          name: "foo",
          ignore: false,
        },
      ]);
    });

    // special directives
    describe("Not-handled directives", () => {
      it("v-slot", () => {
        const { result } = parse(`<span v-slot="foo" />`);

        expect(result).toHaveLength(0);
      });

      it("v-if", () => {
        const { result } = parse(`<span v-if="foo" />`);

        expect(result).toHaveLength(0);
      });

      it("v-else-if", () => {
        const { result } = parse(`<span v-else-if="foo" />`);

        expect(result).toHaveLength(0);
      });

      it("v-else", () => {
        const { result } = parse(`<span v-else />`);

        expect(result).toHaveLength(0);
      });

      it("v-for", () => {
        const { result } = parse(`<span v-for="foo in bar" />`);

        expect(result).toHaveLength(0);
      });
    });
  });

  describe("class & style", () => {
    describe("class", () => {
      it("simple", () => {
        const { result, source } = parse(`<div class="foo bar" />`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            node: null,
            name: "class",
            props: [
              {
                type: TemplateTypes.Prop,
                name: "class",
                value: "foo bar",
                node: {
                  type: NodeTypes.ATTRIBUTE,
                },
              },
            ],
          },
        ]);

        for (const { props } of result as any) {
          expectMapping(props, source);
        }
      });
      it("multiple", () => {
        const { result, source } = parse(
          `<div class="foo bar" :class="baz" />`
        );

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            node: null,
            name: "class",
            context: {
              ignoredIdentifiers: [],
            },
            props: [
              {
                type: TemplateTypes.Prop,
                name: "class",
                value: "foo bar",
                node: {
                  type: NodeTypes.ATTRIBUTE,
                },
              },
              {
                type: TemplateTypes.Prop,
                arg: [{ name: "class", ignore: true }],
                exp: [
                  {
                    type: TemplateTypes.Binding,
                    name: "baz",
                    ignore: false,
                  },
                ],
                node: {
                  name: "bind",
                  rawName: ":class",
                },
                static: false,
              },
            ],
          },
          {
            type: TemplateTypes.Binding,
            name: "class",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "baz",
            ignore: false,
          },
        ]);
      });
    });

    describe("style", () => {
      it("simple", () => {
        const { result, source } = parse(`<div style="foo bar" />`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            node: null,
            name: "style",
            context: {
              ignoredIdentifiers: [],
            },
            props: [
              {
                type: TemplateTypes.Prop,
                name: "style",
                value: "foo bar",
                node: {
                  type: NodeTypes.ATTRIBUTE,
                },
              },
            ],
          },
        ]);

        for (const { props } of result as any) {
          expectMapping(props, source);
        }
      });
      it("multiple", () => {
        const { result } = parse(`<div style="foo bar" :style="baz" />`);

        expect(result).toMatchObject([
          {
            type: TemplateTypes.Prop,
            node: null,
            name: "style",
            context: {
              ignoredIdentifiers: [],
            },
            props: [
              {
                type: TemplateTypes.Prop,
                name: "style",
                value: "foo bar",
                node: {
                  type: NodeTypes.ATTRIBUTE,
                },
              },
              {
                type: TemplateTypes.Prop,
                arg: [{ name: "style", ignore: true }],
                exp: [
                  {
                    type: TemplateTypes.Binding,
                    name: "baz",
                    ignore: false,
                  },
                ],
                node: {
                  name: "bind",
                  rawName: ":style",
                },
                static: false,
              },
            ],
          },
          {
            type: TemplateTypes.Binding,
            name: "style",
            ignore: true,
          },
          {
            type: TemplateTypes.Binding,
            name: "baz",
            ignore: false,
          },
        ]);
      });
    });
  });
});
