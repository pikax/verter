import { parseTemplate } from "./index.js";
import { parse as parseSFC } from "@vue/compiler-sfc";
import { templateItemsToMap } from "./utils.js";

describe("parser template", () => {
  function parse(content: string) {
    const source = `<template>${content}</template>`;
    const sfc = parseSFC(source, { templateParseOptions: {} });

    const template = sfc.descriptor.template;
    const ast = template?.ast!;
    const r = parseTemplate(ast!, source);

    return {
      ...r,
      ...templateItemsToMap(r.items),
    };
  }

  it("should parse template", () => {
    const result = parse("");
    expect(result).toMatchObject({});
  });

  describe("commments", () => {
    it("should extract comment", () => {
      const result = parse("<!-- comment -->");

      expect(result).toMatchObject({
        Comment: [
          {
            content: " comment ",
            node: {
              content: " comment ",
              loc: {
                source: "<!-- comment -->",
              },
            },
          },
        ],
      });

      const [comment] = result.Comment;

      expect(
        result.source.slice(
          comment.node.loc.start.offset,
          comment.node.loc.end.offset
        )
      ).toBe(comment.node.loc.source);
    });

    it("comment in a component", () => {
      const result = parse(`<div> <!-- comment --> </div>`);

      expect(result).toMatchObject({
        Comment: [
          {
            content: " comment ",
            node: {
              content: " comment ",
              loc: {
                source: "<!-- comment -->",
              },
            },
          },
        ],
      });

      const [comment] = result.Comment;

      expect(
        result.source.slice(
          comment.node.loc.start.offset,
          comment.node.loc.end.offset
        )
      ).toBe(comment.node.loc.source);
    });

    it("multiple", () => {
      const result = parse(
        `<!-- comment --><div> <!-- comment deep --> </div>`
      );

      expect(result).toMatchObject({
        Comment: [
          {
            content: " comment ",
            node: {
              content: " comment ",
              loc: {
                source: "<!-- comment -->",
              },
            },
          },
          {
            content: " comment deep ",
            node: {
              content: " comment deep ",
              loc: {
                source: "<!-- comment deep -->",
              },
            },
          },
        ],
      });

      const [comment, commentDeep] = result.Comment;

      expect(
        result.source.slice(
          comment.node.loc.start.offset,
          comment.node.loc.end.offset
        )
      ).toBe(comment.node.loc.source);

      expect(
        result.source.slice(
          commentDeep.node.loc.start.offset,
          commentDeep.node.loc.end.offset
        )
      ).toBe(commentDeep.node.loc.source);
    });
  });

  describe("element", () => {
    it("<div></div>", () => {
      const result = parse(`<div></div>`);

      expect(result).toMatchObject({
        Element: [
          {
            type: "Element",
            tag: "div",
          },
        ],
      });
    });

    it("<div><div>test</div></div>", () => {
      const result = parse(`<div><div>test</div></div>`);

      expect(result).toMatchObject({
        Element: [
          {
            type: "Element",
            tag: "div",
          },
          {
            type: "Element",
            tag: "div",
          },
        ],
        Text: [
          {
            type: "Text",
            content: "test",
          },
        ],
      });
    });

    it('<div v-if="hello === false"> <div> {{ hello }}</div></div>', () => {
      const result = parse(
        `<div v-if="hello === false"> <div> {{ hello }}</div></div>`
      );

      expect(result).toMatchObject({
        Element: [
          {
            type: "Element",
            tag: "div",

            condition: {
              type: "Condition",
              bindings: [
                {
                  type: "Binding",
                  name: "hello",
                },
              ],
            },
            context: {
              conditions: [
                {
                  type: "Condition",
                  bindings: [
                    {
                      type: "Binding",
                      name: "hello",
                    },
                  ],
                },
              ],
            },
          },
          {
            type: "Element",
            tag: "div",
            context: {
              conditions: [
                {
                  type: "Condition",
                  bindings: [
                    {
                      type: "Binding",
                      name: "hello",
                    },
                  ],
                },
              ],
            },
          },
        ],
        Condition: [
          {
            type: "Condition",
            node: {
              name: "if",
            },
          },
        ],
        Binding: [
          {
            type: "Binding",
            name: "hello",
          },
          {
            type: "Binding",
            name: "hello",
          },
        ],
      });
    });

    it('<div v-for="item in items"> <div> {{ item }}</div></div>', () => {
      const result = parse(
        `<div v-for="item in items"> <div> {{ item }}</div></div>`
      );

      expect(result).toMatchObject({
        Element: [
          {
            type: "Element",
            tag: "div",

            loop: {
              type: "Loop",
              node: {
                name: "for",
              },
            },
            context: {
              inFor: true,
            },
          },
          {
            type: "Element",
            tag: "div",
            loop: null,
            context: {
              inFor: true,
            },
          },
        ],
        Loop: [
          {
            type: "Loop",
            node: {
              name: "for",
            },
            element: {
              props: [
                {
                  name: "for",
                },
              ],
            },
          },
        ],
      });
    });
  });

  // pug is not supported
  describe.skip("pug", () => {
    function parse(content: string) {
      const source = `<template lang="pug">\n${content}</template>`;
      const sfc = parseSFC(source, { templateParseOptions: {} });

      const template = sfc.descriptor.template;
      const ast = template?.ast!;
      const r = parseTemplate(ast!, source);

      return {
        ...r,
        ...templateItemsToMap(r.items),
      };
    }
    it("should parse pug", () => {
      const result = parse("p {{ msg }}");

      expect(result).toMatchObject({
        Element: [
          {
            type: "Element",
            tag: "div",
          },
        ],
      });
    });

    it("should parse pug with comments", () => {
      const result = parse("// comment\ndiv");

      expect(result).toMatchObject({
        Comment: [
          {
            content: " comment",
            node: {
              content: " comment",
            },
          },
        ],
        Element: [
          {
            type: "Element",
            tag: "div",
          },
        ],
      });
    });
  });
});
