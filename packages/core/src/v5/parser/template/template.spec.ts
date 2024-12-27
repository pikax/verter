import { parseTemplate } from "./index.js";
import { parse as parseSFC } from "@vue/compiler-sfc";

describe("parser template", () => {
  function parse(content: string) {
    const source = `<template>${content}</template>`;
    const sfc = parseSFC(source, { templateParseOptions: {} });

    const template = sfc.descriptor.template;
    const ast = template ? template.ast : null;
    return parseTemplate(ast, source);
  }

  it("should parse template", () => {
    const result = parse("");
    expect(result).toMatchObject({});
  });

  describe("commments", () => {
    it("should extract comment", () => {
      const result = parse("<!-- comment -->");

      expect(result).toMatchObject({
        comments: [
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

      const [comment] = result.comments;

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
        comments: [
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

      const [comment] = result.comments;

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
        comments: [
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

      const [comment, commentDeep] = result.comments;

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
});
