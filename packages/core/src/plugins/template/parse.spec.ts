import { DirectiveNode, ElementNode } from "@vue/compiler-core";
import { parse as sfcParse } from "@vue/compiler-sfc";
import {
  ParseContext,
  isWrapable,
  parseProps,
  parseElement,
  ParsedType,
} from "./parse.js";

describe("parse", () => {
  describe("isWrapable", () => {
    it.each([
      ["v-if", true],
      ["v-else-if", true],
      ["v-else", true],

      ["v-slot", false],
      ["v-random", false],
      ["v-model", false],
    ])('isWrapable("%s") === %s', (name, expected) => {
      // @ts-expect-error
      expect(isWrapable({ rawName: name })).toBe(expected);
    });
  });

  describe("parseProps", () => {
    function doParseProps(content: string) {
      const source = `<template><div ${content}></div></template>`;
      const ast = sfcParse(source, {});
      const root = ast.descriptor.template!.ast!;
      const parent = root.children[0] as ElementNode;
      return {
        context: {
          root,

          parent: undefined,

          next: undefined,
          prev: undefined,
        } satisfies ParseContext,
        parent: parent,
        props: parent.props,
      };
    }

    describe("attributes", () => {
      it("simple", () => {
        const { props, parent, context } = doParseProps('id="app"');

        expect(parseProps(props[0], parent, context)).toEqual({
          node: props[0],
          parent: parent,
          name: "id",
          value: "app",
        });
      });
      it("no value", () => {
        const { props, parent, context } = doParseProps("id");
        expect(parseProps(props[0], parent, context)).toEqual({
          node: props[0],
          parent: parent,
          name: "id",
          value: undefined,
        });
      });

      it("multiple", () => {
        const { props, parent, context } = doParseProps("id foo='app'");

        expect(parseProps(props[0], parent, context)).toEqual({
          node: props[0],
          parent: parent,
          name: "id",
          value: undefined,
        });
        expect(parseProps(props[1], parent, context)).toEqual({
          node: props[1],
          parent: parent,
          name: "foo",
          value: "app",
        });
      });
    });

    describe("directives", () => {
      describe("conditional", () => {
        it("v-if", () => {
          const { props, parent, context } = doParseProps('v-if="true"');

          expect(parseProps(props[0], parent, context)).toMatchObject({
            node: props[0],
            parent: parent,

            rawName: "v-if",
            wrap: true,
          });
        });
        it("v-else-if", () => {
          const { props, parent, context } = doParseProps('v-else-if="true"');

          expect(parseProps(props[0], parent, context)).toMatchObject({
            node: props[0],
            parent: parent,

            rawName: "v-else-if",
            wrap: true,
          });
        });
        it("v-else", () => {
          const { props, parent, context } = doParseProps("v-else");

          expect(parseProps(props[0], parent, context)).toMatchObject({
            node: props[0],
            parent: parent,

            rawName: "v-else",
            wrap: true,
          });
        });
      });

      describe("for", () => {
        it("v-for", () => {
          const { props, parent, context } = doParseProps(
            'v-for="item in items"'
          );

          expect(parseProps(props[0], parent, context)).toMatchObject({
            node: props[0],
            parent: parent,

            rawName: "v-for",
            wrap: true,
            children: undefined,
            for: (props[0] as DirectiveNode).forParseResult,
          });
        });
      });

      // TODO add more directives tests
    });

    describe("parseElement", () => {
      function doParseElement(content: string) {
        const source = `<template>${content}</template>`;
        const ast = sfcParse(source, {});
        const root = ast.descriptor.template!.ast!;
        const parent = root.children[0] as ElementNode;
        return {
          context: {
            root,

            parent: undefined,

            next: undefined,
            prev: undefined,
          } satisfies ParseContext,
          node: parent,
          children: root.children,
        };
      }

      it("div", () => {
        const { node, context } = doParseElement("<div></div>");

        expect(parseElement(node, context)).toEqual({
          type: ParsedType.Element,
          node: node,
          tag: "div",
          children: [],
          props: [],
        });
      });

      it("div with props", () => {
        const { node, context } = doParseElement('<div id="app"></div>');

        expect(parseElement(node, context)).toEqual({
          type: ParsedType.Element,
          node: node,
          tag: "div",
          children: [],
          props: [
            {
              node: node.props[0],
              parent: node,
              name: "id",
              value: "app",
            },
          ],
        });
      });

      it("div with child", () => {
        const { node, context } = doParseElement("<div><span></span></div>");

        expect(parseElement(node, context)).toEqual({
          type: ParsedType.Element,
          node: node,
          tag: "div",
          children: [
            {
              node: node.children[0],
              type: ParsedType.Element,
              tag: "span",
              children: [],
              props: [],
            },
          ],
          props: [],
        });
      });

      it("div with v-if", () => {
        const { node, context } = doParseElement('<div v-if="true"></div>');

        const r = parseElement(node, context);
        expect(r).toMatchObject({
          node: node.props[0],
          parent: node,
          children: [
            {
              node: node,
              tag: "div",
              children: [],
              props: [],
            },
          ],

          rawName: "v-if",
          wrap: true,
        });
      });

      it("div with v-for", () => {
        const { node, context } = doParseElement(
          '<div v-for="item in items"></div>'
        );

        const r = parseElement(node, context);
        expect(r).toMatchObject({
          node: node.props[0],
          parent: node,
          children: [
            {
              node: node,
              tag: "div",
              children: [],
              props: [],
            },
          ],

          rawName: "v-for",
          wrap: true,
        });
      });

      // mainly testing order
      describe("conditional + for", () => {
        it("div with v-for + v-if", () => {
          const { node, context } = doParseElement(
            '<div v-for="item in items" v-if="true"></div>'
          );

          const r = parseElement(node, context);
          expect(r).toMatchObject({
            node: node.props[1],
            parent: node,
            children: [
              {
                node: node.props[0],
                parent: node,
                children: [
                  {
                    node: node,
                    tag: "div",
                    children: [],
                    props: [],
                  },
                ],

                rawName: "v-for",
                wrap: true,
              },
            ],

            rawName: "v-if",
            wrap: true,
          });
        });

        it("div v-if + v-for", () => {
          const { node, context } = doParseElement(
            '<div v-if="true" v-for="item in items"></div>'
          );

          const r = parseElement(node, context);
          expect(r).toMatchObject({
            node: node.props[0],
            parent: node,
            children: [
              {
                node: node.props[1],
                parent: node,
                children: [
                  {
                    node: node,
                    tag: "div",
                    children: [],
                    props: [],
                  },
                ],

                rawName: "v-for",
                wrap: true,
              },
            ],

            rawName: "v-if",
            wrap: true,
          });
        });
        it("div v-else + v-for", () => {
          const { node, context } = doParseElement(
            '<div v-else v-for="item in items"></div>'
          );

          const r = parseElement(node, context);
          expect(r).toMatchObject({
            node: node.props[0],
            parent: node,
            children: [
              {
                node: node.props[1],
                parent: node,
                children: [
                  {
                    node: node,
                    tag: "div",
                    children: [],
                    props: [],
                  },
                ],

                rawName: "v-for",
                wrap: true,
              },
            ],

            rawName: "v-else",
            wrap: true,
          });
        });

        it("div v-for + v-else", () => {
          const { node, context } = doParseElement(
            '<div v-for="item in items" v-else></div>'
          );

          const r = parseElement(node, context);
          expect(r).toMatchObject({
            node: node.props[1],
            parent: node,
            children: [
              {
                node: node.props[0],
                parent: node,
                children: [
                  {
                    node: node,
                    tag: "div",
                    children: [],
                    props: [],
                  },
                ],

                rawName: "v-for",
                wrap: true,
              },
            ],

            rawName: "v-else",
            wrap: true,
          });
        });
      });
    });
  });

  describe("parseElement", () => {
    function doParseElement(content: string) {
      const source = `<template>${content}</template>`;
      const ast = sfcParse(source, {});
      const root = ast.descriptor.template!.ast!;
      const parent = root.children[0] as ElementNode;
      return {
        context: {
          root,

          parent: undefined,

          next: undefined,
          prev: undefined,
        } satisfies ParseContext,
        parent: parent,
        props: parent.props,
      };
    }
    it("if/else", () => {
      const source = `<div><span v-if="a===1">1</span><span v-else>2</span></div>`;
      const { parent, context } = doParseElement(source);
      const r = parseElement(parent, context);
      expect(r).toMatchObject({
        tag: "div",
        node: parent,
        props: [],

        children: [
          {
            type: ParsedType.Condition,
            children: [
              {
                rawName: "v-if",
                wrap: true,
                parent: parent.children[0],
                children: [
                  {
                    node: parent.children[0],
                    tag: "span",
                    children: [
                      {
                        type: ParsedType.Text,
                        content: "1",
                      },
                    ],
                    props: [],
                  },
                ],
              },
              {
                rawName: "v-else",
                wrap: true,
                parent: parent.children[1],
                children: [
                  {
                    node: parent.children[1],
                    tag: "span",
                    children: [
                      {
                        type: ParsedType.Text,
                        content: "2",
                      },
                    ],
                    props: [],
                  },
                ],
              },
            ],
          },
        ],
      });
    });

    it("multiple if/else", () => {
      const source = `<div><span v-if="a===1">1</span><span v-else-if="a===2">2</span><span v-if="a === 3">3</span></div>`;
      const el = doParseElement(source);
      const r = parseElement(el.parent, el.context);

      expect(r).toMatchObject({
        tag: "div",
        node: el.parent,
        props: [],

        children: [
          {
            type: ParsedType.Condition,
            children: [
              {
                rawName: "v-if",
                wrap: true,
                parent: el.parent.children[0],
                children: [
                  {
                    node: el.parent.children[0],
                    tag: "span",
                    children: [
                      {
                        type: ParsedType.Text,
                        content: "1",
                      },
                    ],
                    props: [],
                  },
                ],
              },
              {
                rawName: "v-else-if",
                wrap: true,
                parent: el.parent.children[1],
                children: [
                  {
                    node: el.parent.children[1],
                    tag: "span",
                    children: [
                      {
                        type: ParsedType.Text,
                        content: "2",
                      },
                    ],
                    props: [],
                  },
                ],
              },
            ],
          },
          {
            type: "condition",
            children: [
              {
                rawName: "v-if",
                wrap: true,
                parent: el.parent.children[2],
                children: [
                  {
                    node: el.parent.children[2],
                    tag: "span",
                    children: [
                      {
                        type: ParsedType.Text,
                        content: "3",
                      },
                    ],
                    props: [],
                  },
                ],
              },
            ],
          },
        ],
      });
    });

    it("if/else + comment", () => {
      const source = `<div><span v-if="true">1</span>
        <!--test-->
        <span v-else>LOL</span></div>`;
      const { parent, context } = doParseElement(source);
      const r = parseElement(parent, context);
      expect(r).toMatchObject({
        tag: "div",
        node: parent,
        props: [],

        children: [
          {
            type: ParsedType.Condition,
            children: [
              {
                rawName: "v-if",
                wrap: true,
                parent: parent.children[0],
                children: [
                  {
                    node: parent.children[0],
                    tag: "span",
                    children: [
                      {
                        type: ParsedType.Text,
                        content: "1",
                      },
                    ],
                    props: [],
                  },
                ],
              },
              {
                type: "comment",
                content: "test",
                node: parent.children[1],
              },
              {
                rawName: "v-else",
                wrap: true,
                parent: parent.children[2],
                children: [
                  {
                    node: parent.children[2],
                    tag: "span",
                    children: [
                      {
                        type: ParsedType.Text,
                        content: "LOL",
                      },
                    ],
                    props: [],
                  },
                ],
              },
            ],
          },
        ],
      });
    });

    it("component + template slots", () => {
      const source = `
        <my-comp>
          <template #test>
            <span>fallback</span>
          </template>
        </my-comp>`;
      const { parent, context } = doParseElement(source);
      const r = parseElement(parent, context);
      expect(r).toMatchObject({
        tag: "my-comp",
        node: parent,
        props: [],

        children: [
          {
            tag: ParsedType.Template,
            node: parent.children[0],
            props: [
              {
                directive: "slot",
                rawName: "#test",

                node: parent.children[0].props[0],
              },
            ],
            children: [
              {
                node: parent.children[0].children[0],
                tag: "span",
                children: [
                  {
                    type: ParsedType.Text,
                    content: "fallback",
                  },
                ],
                props: [],
              },
            ],
          },
        ],
      });
    });

    it("component + v-slot", () => {
      const source = `<my-comp v-slot="{ foo }"><span>{{foo}}</my-comp>`;

      const { parent, context } = doParseElement(source);
      const r = parseElement(parent, context);
      expect(r).toMatchObject({
        tag: "my-comp",
        node: parent,
        props: [],

        children: [
          {
            directive: "slot",
            rawName: "v-slot",
            wrap: false,

            node: parent.props[0],
            children: [
              {
                node: parent.children[0],
                tag: "span",
                children: [
                  {
                    node: parent.children[0].children[0],
                    type: "interpolation",
                    content: { content: "foo" },
                  },
                ],
                props: [],
              },
            ],
          },
        ],
      });
    });
  });
});
